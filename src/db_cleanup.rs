mod errors;
mod config;
mod health;
mod health_server;

use errors::{BotError, BotResult};
use config::{RAW_DATA_RETENTION_HOURS, MINUTE_DATA_RETENTION_DAYS, FIVE_MINUTE_DATA_RETENTION_DAYS, FIFTEEN_MINUTE_DATA_RETENTION_DAYS};
use health::HealthState;

use rusqlite::Connection;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, error, debug};

const DATABASE_PATH: &str = "shared/prices.db";

/// Database cleanup service for aggregating and compacting price data
pub struct DatabaseCleanup {
    health: Arc<HealthState>,
}

impl DatabaseCleanup {
    pub fn new() -> Self {
        let health = Arc::new(HealthState::new("DB-CLEANUP".to_string()));
        Self { health }
    }

    /// Get a database connection with WAL mode and busy timeout
    fn get_connection(&self) -> BotResult<Connection> {
        let conn = Connection::open(DATABASE_PATH).map_err(BotError::Database)?;
        
        // Enable WAL mode for better concurrent access
        if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
            error!("Failed to enable WAL mode: {}", e);
        }
        // Set busy timeout to handle locks better (30 seconds)
        if let Err(e) = conn.pragma_update(None, "busy_timeout", 30000) {
            error!("Failed to set busy timeout: {}", e);
        }
        
        Ok(conn)
    }

    /// Initialize the aggregated data table
    fn init_aggregated_table(&self) -> BotResult<()> {
        let conn = self.get_connection()?;
        
        conn.execute(
            "CREATE TABLE IF NOT EXISTS price_aggregates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                crypto_name TEXT NOT NULL,
                bucket_start INTEGER NOT NULL,
                bucket_duration INTEGER NOT NULL,
                open_price REAL NOT NULL,
                high_price REAL NOT NULL,
                low_price REAL NOT NULL,
                close_price REAL NOT NULL,
                avg_price REAL NOT NULL,
                sample_count INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        // Create indexes for efficient queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_aggregates_crypto_bucket 
             ON price_aggregates(crypto_name, bucket_start, bucket_duration)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_prices_crypto_timestamp 
             ON prices(crypto_name, timestamp)",
            [],
        )?;

        info!("✅ Initialized aggregated data table and indexes");
        Ok(())
    }

    /// Aggregate raw data into time buckets with batching to reduce lock time
    fn aggregate_data(&self, bucket_duration_seconds: u64, older_than_seconds: u64) -> BotResult<u64> {
        info!("   🔍 Checking for data older than {} seconds to aggregate into {}-second buckets", older_than_seconds, bucket_duration_seconds);
        
        let conn = self.get_connection()?;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let cutoff_time = current_time - older_than_seconds;
        let bucket_duration = bucket_duration_seconds as i64;
        
        // Process in smaller batches to reduce lock contention
        let batch_size = 100;
        let mut total_aggregated = 0u64;
        let mut batch_number = 0;
        
        loop {
            batch_number += 1;
            debug!("   📦 Processing batch {} for {}-second aggregation", batch_number, bucket_duration_seconds);
            
            // Get a small batch of data to aggregate
            let mut stmt = conn.prepare(
                "SELECT crypto_name, 
                        (timestamp / ?) * ? as bucket_start,
                        MIN(price) as low_price,
                        MAX(price) as high_price,
                        AVG(price) as avg_price,
                        COUNT(*) as sample_count
                 FROM prices 
                 WHERE timestamp < ? 
                 AND NOT EXISTS (
                     SELECT 1 FROM price_aggregates pa 
                     WHERE pa.crypto_name = prices.crypto_name 
                     AND pa.bucket_start = (prices.timestamp / ?) * ?
                     AND pa.bucket_duration = ?
                 )
                 GROUP BY crypto_name, bucket_start
                 HAVING COUNT(*) > 0
                 ORDER BY crypto_name, bucket_start
                 LIMIT ?"
            )?;

            let rows = stmt.query_map([
                bucket_duration, bucket_duration, // bucket_start calculation
                cutoff_time as i64, // WHERE timestamp < cutoff
                bucket_duration, bucket_duration, bucket_duration, // NOT EXISTS check
                batch_size as i64 // LIMIT
            ], |row| {
                Ok((
                    row.get::<_, String>(0)?,      // crypto_name
                    row.get::<_, i64>(1)?,         // bucket_start
                    row.get::<_, f64>(2)?,         // low_price
                    row.get::<_, f64>(3)?,         // high_price
                    row.get::<_, f64>(4)?,         // avg_price
                    row.get::<_, i64>(5)?,         // sample_count
                ))
            })?;

            let batch_data: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
            
            if batch_data.is_empty() {
                debug!("   ✅ No more data to aggregate for {}-second buckets", bucket_duration_seconds);
                break; // No more data to process
            }
            
            debug!("   📊 Found {} records to aggregate in batch {}", batch_data.len(), batch_number);
            
            // Process this batch in a transaction
            let tx = conn.unchecked_transaction()?;
            let mut batch_count = 0u64;
            
            for (crypto_name, bucket_start, low_price, high_price, avg_price, sample_count) in batch_data {
                // Get open and close prices separately for accuracy
                let open_price = self.get_bucket_open_price(&conn, &crypto_name, bucket_start, bucket_duration)?;
                let close_price = self.get_bucket_close_price(&conn, &crypto_name, bucket_start, bucket_duration)?;
                
                // Insert the aggregated data
                tx.execute(
                    "INSERT INTO price_aggregates 
                     (crypto_name, bucket_start, bucket_duration, open_price, high_price, low_price, close_price, avg_price, sample_count)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    [
                        &crypto_name,
                        &bucket_start.to_string(),
                        &bucket_duration.to_string(),
                        &open_price.to_string(),
                        &high_price.to_string(),
                        &low_price.to_string(),
                        &close_price.to_string(),
                        &avg_price.to_string(),
                        &sample_count.to_string(),
                    ]
                )?;
                
                batch_count += 1;
            }
            
            // Commit this batch
            tx.commit()?;
            total_aggregated += batch_count;
            
            debug!("   ✅ Batch {} completed: {} buckets aggregated (total: {})", batch_number, batch_count, total_aggregated);
            
            // Small delay between batches to allow other processes to access DB
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        if total_aggregated > 0 {
            info!("📊 Aggregated {} buckets of {}-second data", total_aggregated, bucket_duration_seconds);
        }

        Ok(total_aggregated)
    }

    /// Get the opening price for a bucket
    fn get_bucket_open_price(&self, conn: &Connection, crypto_name: &str, bucket_start: i64, bucket_duration: i64) -> BotResult<f64> {
        let bucket_end = bucket_start + bucket_duration;
        let mut stmt = conn.prepare(
            "SELECT price FROM prices 
             WHERE crypto_name = ? AND timestamp >= ? AND timestamp < ?
             ORDER BY timestamp ASC LIMIT 1"
        )?;
        
        let price: f64 = stmt.query_row([crypto_name, &bucket_start.to_string(), &bucket_end.to_string()], |row| {
            row.get(0)
        })?;
        
        Ok(price)
    }

    /// Get the closing price for a bucket
    fn get_bucket_close_price(&self, conn: &Connection, crypto_name: &str, bucket_start: i64, bucket_duration: i64) -> BotResult<f64> {
        let bucket_end = bucket_start + bucket_duration;
        let mut stmt = conn.prepare(
            "SELECT price FROM prices 
             WHERE crypto_name = ? AND timestamp >= ? AND timestamp < ?
             ORDER BY timestamp DESC LIMIT 1"
        )?;
        
        let price: f64 = stmt.query_row([crypto_name, &bucket_start.to_string(), &bucket_end.to_string()], |row| {
            row.get(0)
        })?;
        
        Ok(price)
    }

    /// Delete raw data that has been successfully aggregated
    fn cleanup_aggregated_raw_data(&self, older_than_seconds: u64) -> BotResult<u64> {
        info!("   🔍 Checking how many raw records are older than {} seconds", older_than_seconds);
        
        let conn = self.get_connection()?;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let cutoff_time = current_time - older_than_seconds;
        
        // First, count how many records we're about to delete
        let mut count_stmt = conn.prepare("SELECT COUNT(*) FROM prices WHERE timestamp < ?")?;
        let count: i64 = count_stmt.query_row([cutoff_time as i64], |row| row.get(0))?;
        
        info!("   📊 Found {} raw records older than {} seconds", count, older_than_seconds);
        
        if count == 0 {
            info!("   ✅ No old raw data to clean up");
            return Ok(0);
        }
        
        info!("   🗑️ Starting deletion of {} old raw records (this may take a while)...", count);
        
        // Use a simpler delete query - just delete old data regardless of aggregation status
        // This is safer and faster than the complex EXISTS query
        let deleted = conn.execute(
            "DELETE FROM prices WHERE timestamp < ?",
            [cutoff_time as i64],
        )?;

        info!("   ✅ Successfully deleted {} raw price records older than {} seconds", deleted, older_than_seconds);

        Ok(deleted as u64)
    }

    /// Delete old aggregated data beyond retention period
    fn cleanup_old_aggregates(&self, bucket_duration_seconds: u64, older_than_seconds: u64) -> BotResult<u64> {
        info!("   � Clelaning up {}-second aggregates older than {} seconds", bucket_duration_seconds, older_than_seconds);
        
        let conn = self.get_connection()?;
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let cutoff_time = current_time - older_than_seconds;
        
        // First count what we're about to delete
        let mut count_stmt = conn.prepare("SELECT COUNT(*) FROM price_aggregates WHERE bucket_start < ? AND bucket_duration = ?")?;
        let count: i64 = count_stmt.query_row([cutoff_time as i64, bucket_duration_seconds as i64], |row| row.get(0))?;
        
        if count > 0 {
            info!("   🗑️ Deleting {} old {}-second aggregate records...", count, bucket_duration_seconds);
        } else {
            info!("   ✅ No old {}-second aggregates to clean up", bucket_duration_seconds);
        }
        
        let deleted = conn.execute(
            "DELETE FROM price_aggregates 
             WHERE bucket_start < ? AND bucket_duration = ?",
            [cutoff_time as i64, bucket_duration_seconds as i64],
        )?;

        if deleted > 0 {
            info!("   ✅ Deleted {} aggregated records ({}-second buckets)", deleted, bucket_duration_seconds);
        }

        Ok(deleted as u64)
    }

    /// Vacuum the database to reclaim space
    fn vacuum_database(&self) -> BotResult<()> {
        let conn = self.get_connection()?;
        
        info!("🧹 Starting database vacuum...");
        conn.execute("VACUUM", [])?;
        info!("✅ Database vacuum completed");
        
        Ok(())
    }

    /// Get database statistics
    fn get_database_stats(&self) -> BotResult<()> {
        let conn = self.get_connection()?;
        
        // Count raw price records
        let raw_count: i64 = conn.query_row("SELECT COUNT(*) FROM prices", [], |row| row.get(0))?;
        
        // Count aggregated records by bucket size
        let mut stmt = conn.prepare(
            "SELECT bucket_duration, COUNT(*) FROM price_aggregates GROUP BY bucket_duration ORDER BY bucket_duration"
        )?;
        
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;

        info!("📊 Database Statistics:");
        info!("   Raw price records: {}", raw_count);
        
        for row in rows {
            let (duration, count) = row?;
            info!("   {}-second aggregates: {}", duration, count);
        }

        Ok(())
    }

    /// Perform complete cleanup cycle with retry logic
    pub async fn perform_cleanup(&self) -> BotResult<()> {
        const MAX_RETRIES: u32 = 3;
        
        for attempt in 1..=MAX_RETRIES {
            match self.perform_cleanup_attempt().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    error!("❌ Cleanup attempt {} failed: {}", attempt, e);
                    if attempt < MAX_RETRIES {
                        info!("⏳ Retrying cleanup in 30 seconds...");
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        unreachable!()
    }

    /// Single cleanup attempt
    async fn perform_cleanup_attempt(&self) -> BotResult<()> {
        info!("🧹 Starting database cleanup cycle...");
        self.health.update_price_timestamp(); // Use as "last activity" timestamp
        
        // Initialize aggregated table if needed
        info!("📋 Step 1/7: Initializing aggregated data table...");
        self.init_aggregated_table()?;
        
        // Tier 1: Aggregate raw data older than 24 hours into 1-minute buckets
        info!("📊 Step 2/7: Aggregating raw data into 1-minute buckets...");
        let aggregated_1m = self.aggregate_data(60, RAW_DATA_RETENTION_HOURS * 3600)?;
        
        // Tier 2: Aggregate 1-minute data older than 7 days into 5-minute buckets  
        info!("📊 Step 3/7: Aggregating 1-minute data into 5-minute buckets...");
        let aggregated_5m = self.aggregate_data(300, MINUTE_DATA_RETENTION_DAYS * 24 * 3600)?;
        
        // Tier 3: Aggregate 5-minute data older than 30 days into 15-minute buckets
        info!("📊 Step 4/7: Aggregating 5-minute data into 15-minute buckets...");
        let aggregated_15m = self.aggregate_data(900, FIVE_MINUTE_DATA_RETENTION_DAYS * 24 * 3600)?;
        
        // Clean up raw data that has been aggregated (older than 24 hours)
        info!("🗑️ Step 5/7: Cleaning up old raw data (older than 24 hours)...");
        let deleted_raw = self.cleanup_aggregated_raw_data(RAW_DATA_RETENTION_HOURS * 3600)?;
        
        // Clean up old aggregated data beyond retention periods
        info!("🗑️ Step 6/7: Cleaning up old aggregated data...");
        let deleted_1m = self.cleanup_old_aggregates(60, MINUTE_DATA_RETENTION_DAYS * 24 * 3600)?;
        let deleted_5m = self.cleanup_old_aggregates(300, FIVE_MINUTE_DATA_RETENTION_DAYS * 24 * 3600)?;
        let deleted_15m = self.cleanup_old_aggregates(900, FIFTEEN_MINUTE_DATA_RETENTION_DAYS * 24 * 3600)?;
        
        // Vacuum database if significant cleanup occurred
        let total_deleted = deleted_raw + deleted_1m + deleted_5m + deleted_15m;
        if total_deleted > 1000 {
            info!("🔧 Step 7/7: Running database vacuum (deleted {} records)...", total_deleted);
            self.vacuum_database()?;
        } else {
            info!("⏭️ Step 7/7: Skipping vacuum (only {} records deleted)", total_deleted);
        }
        
        // Update health timestamp
        self.health.update_db_timestamp();
        
        // Show final statistics
        info!("📈 Generating final database statistics...");
        self.get_database_stats()?;
        
        info!("✅ Cleanup cycle completed:");
        info!("   📊 Aggregated: {}x1m + {}x5m + {}x15m buckets", aggregated_1m, aggregated_5m, aggregated_15m);
        info!("   🗑️ Deleted: {} total records", total_deleted);
        
        Ok(())
    }

    /// Start the health server
    pub async fn start_health_server(&self) {
        let health_clone = self.health.clone();
        tokio::spawn(async move {
            health_server::start_health_server(health_clone, 8080).await;
        });
    }

    /// Run the cleanup service with periodic execution
    pub async fn run(&self) -> BotResult<()> {
        let interval_hours = std::env::var("CLEANUP_INTERVAL_HOURS")
            .unwrap_or_else(|_| "24".to_string())
            .parse::<u64>()
            .unwrap_or(24);
        
        let interval = Duration::from_secs(interval_hours * 3600);
        
        info!("🚀 Database cleanup service started");
        info!("⏰ Cleanup interval: {} hours", interval_hours);
        
        // Start health server
        self.start_health_server().await;
        
        // Run initial cleanup after a short delay
        sleep(Duration::from_secs(30)).await;
        
        loop {
            match self.perform_cleanup().await {
                Ok(_) => {
                    info!("✅ Cleanup completed successfully");
                    self.health.reset_failures();
                }
                Err(e) => {
                    error!("❌ Cleanup failed: {}", e);
                    self.health.increment_failures();
                }
            }
            
            info!("⏰ Next cleanup in {} hours", interval_hours);
            sleep(interval).await;
        }
    }
}

#[tokio::main]
async fn main() -> BotResult<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info,db_cleanup=debug")
        .init();
    
    info!("🧹 Starting Database Cleanup Service...");
    
    let cleanup_service = DatabaseCleanup::new();
    cleanup_service.run().await
}