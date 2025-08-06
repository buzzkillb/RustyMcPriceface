use crate::errors::{BotError, BotResult};
use crate::config::PRICE_HISTORY_DAYS;
use crate::utils::{get_current_timestamp, calculate_percentage_change, get_change_arrow, validate_crypto_name, validate_price};
use rusqlite::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, error, debug};

const MAX_RETRIES: u32 = 3;
const CLEANUP_INTERVAL_SECONDS: u64 = 86400; // 24 hours

/// Database abstraction layer for price data
#[derive(Debug)]
pub struct PriceDatabase {
    db_path: String,
}

impl PriceDatabase {
    pub fn new(db_path: &str) -> Self {
        Self {
            db_path: db_path.to_string(),
        }
    }

    /// Get a database connection with retry logic and WAL mode for better concurrency
    pub fn get_connection(&self) -> BotResult<Connection> {
        for attempt in 1..=MAX_RETRIES {
            match Connection::open(&self.db_path) {
                Ok(conn) => {
                    // Enable WAL mode for better concurrent access
                    if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
                        error!("Failed to enable WAL mode: {}", e);
                    }
                    // Set busy timeout to handle locks better (30 seconds)
                    if let Err(e) = conn.pragma_update(None, "busy_timeout", 30000) {
                        error!("Failed to set busy timeout: {}", e);
                    }
                    return Ok(conn);
                },
                Err(e) => {
                    error!("Database connection attempt {} failed: {}", attempt, e);
                    if attempt < MAX_RETRIES {
                        std::thread::sleep(std::time::Duration::from_millis(1000 * attempt as u64));
                    } else {
                        return Err(BotError::Database(e));
                    }
                }
            }
        }
        unreachable!()
    }

    /// Save a price record to the database
    pub fn save_price(&self, crypto_name: &str, price: f64) -> BotResult<()> {
        let conn = self.get_connection()?;
        let current_time = get_current_timestamp()?;
        
        let mut stmt = conn.prepare(
            "INSERT INTO prices (crypto_name, price, timestamp) VALUES (?, ?, ?)"
        )?;
        
        stmt.execute([crypto_name, &price.to_string(), &current_time.to_string()])?;
        debug!("Saved {} price to database: ${}", crypto_name, price);
        Ok(())
    }

    /// Get price changes for different time periods (works with both raw and aggregated data)
    pub fn get_price_changes(&self, crypto: &str, current_price: f64) -> BotResult<String> {
        info!("🔍 Getting price changes for {} at ${}", crypto, current_price);
        validate_crypto_name(crypto)?;
        validate_price(current_price)?;
        
        let conn = self.get_connection()?;
        let current_time = get_current_timestamp()?;
        
        let mut changes = Vec::new();
        
        // Define time periods and their labels
        let periods = vec![
            (3600, "1h"),
            (43200, "12h"), 
            (86400, "24h"),
            (604800, "7d"),
            (2592000, "30d"), // 30 days in seconds
        ];
        
        for (seconds, label) in periods {
            let time_ago = current_time - seconds;
            
            // Try to get price from appropriate data source based on age
            let old_price = if seconds <= 24 * 3600 {
                // For recent data (< 24h), use raw prices table
                debug!("Looking for {} {} data in raw table, time_ago: {}", label, crypto, time_ago);
                self.get_price_from_raw_data(&conn, crypto, time_ago)?
            } else if seconds <= 7 * 24 * 3600 {
                // For 1-7 days old, use 1-minute aggregates
                debug!("Looking for {} {} data in 60s aggregates, time_ago: {}", label, crypto, time_ago);
                self.get_price_from_aggregates(&conn, crypto, time_ago, 60)?
            } else if seconds <= 30 * 24 * 3600 {
                // For 7-30 days old, use 5-minute aggregates
                debug!("Looking for {} {} data in 300s aggregates, time_ago: {}", label, crypto, time_ago);
                self.get_price_from_aggregates(&conn, crypto, time_ago, 300)?
            } else {
                // For older data, use 15-minute aggregates
                debug!("Looking for {} {} data in 900s aggregates, time_ago: {}", label, crypto, time_ago);
                self.get_price_from_aggregates(&conn, crypto, time_ago, 900)?
            };
            
            // Only add the change if we have data for that time period
            if let Some(price) = old_price {
                debug!("Found {} {} price: ${} (current: ${})", label, crypto, price, current_price);
                let change_percent = calculate_percentage_change(current_price, price)?;
                let arrow = get_change_arrow(change_percent);
                let sign = if change_percent >= 0.0 { "+" } else { "" };
                changes.push(format!("{} {}{:.2}% ({})", arrow, sign, change_percent, label));
            } else {
                debug!("No {} {} price data found for time_ago: {}", label, crypto, time_ago);
            }
        }
        
        info!("Found {} price changes for {}: {:?}", changes.len(), crypto, changes);
        
        if changes.is_empty() {
            Ok("🔄 Building history".to_string())
        } else {
            Ok(format!(" {}", changes.join(" | ")))
        }
    }

    /// Get price from raw data table
    fn get_price_from_raw_data(&self, conn: &Connection, crypto: &str, time_ago: u64) -> BotResult<Option<f64>> {
        let mut stmt = conn.prepare(
            "SELECT price FROM prices WHERE crypto_name = ? AND timestamp >= ? ORDER BY timestamp ASC LIMIT 1"
        )?;
        
        let rows = stmt.query_map([crypto, &time_ago.to_string()], |row| {
            Ok(row.get(0)?)
        })?;
        
        let mut prices = rows.collect::<Result<Vec<f64>, _>>()?;
        Ok(prices.pop())
    }

    /// Get price from aggregated data table
    fn get_price_from_aggregates(&self, conn: &Connection, crypto: &str, time_ago: u64, bucket_duration: u64) -> BotResult<Option<f64>> {
        // Find the bucket that contains or is closest to the target time
        // We want the bucket where bucket_start <= time_ago < bucket_start + bucket_duration
        // Or the closest bucket if no exact match
        let mut stmt = conn.prepare(
            "SELECT open_price FROM price_aggregates 
             WHERE crypto_name = ? AND bucket_duration = ?
             AND bucket_start <= ?
             ORDER BY bucket_start DESC LIMIT 1"
        )?;
        
        let rows = stmt.query_map([crypto, &bucket_duration.to_string(), &time_ago.to_string()], |row| {
            Ok(row.get(0)?)
        })?;
        
        let mut prices = rows.collect::<Result<Vec<f64>, _>>()?;
        Ok(prices.pop())
    }

    /// Get price indicator from database for status display
    pub fn get_price_indicator(&self, crypto_name: &str, current_price: f64) -> (String, f64) {
        let current_time = match get_current_timestamp() {
            Ok(time) => time,
            Err(_) => return ("🔄".to_string(), 0.0),
        };
        
        let conn = match self.get_connection() {
            Ok(conn) => conn,
            Err(_) => return ("🔄".to_string(), 0.0),
        };
        
        let mut stmt = match conn.prepare(
            "SELECT price FROM prices WHERE crypto_name = ? AND timestamp >= ? ORDER BY timestamp ASC LIMIT 1"
        ) {
            Ok(stmt) => stmt,
            Err(_) => return ("🔄".to_string(), 0.0),
        };
        
        let one_hour_ago = current_time - 3600; // 1 hour
        let rows = match stmt.query_map([crypto_name, &one_hour_ago.to_string()], |row| {
            Ok(row.get(0)?)
        }) {
            Ok(rows) => rows,
            Err(_) => return ("🔄".to_string(), 0.0),
        };
        
        let mut prices = match rows.collect::<Result<Vec<f64>, _>>() {
            Ok(prices) => prices,
            Err(_) => return ("🔄".to_string(), 0.0),
        };
        
        if let Some(oldest_price) = prices.pop() {
            match calculate_percentage_change(current_price, oldest_price) {
                Ok(change_percent) => {
                    let arrow = get_change_arrow(change_percent);
                    return (arrow.to_string(), change_percent);
                }
                Err(_) => return ("🔄".to_string(), 0.0),
            }
        }
        
        // No history yet
        ("🔄".to_string(), 0.0)
    }

    /// Clean up old price records from the database
    pub fn cleanup_old_prices(&self) -> BotResult<()> {
        let conn = self.get_connection()?;
        
        // Keep only the last 60 days of data
        let cutoff_time = get_current_timestamp()? - (PRICE_HISTORY_DAYS * 24 * 3600);
        
        let deleted = conn.execute(
            "DELETE FROM prices WHERE timestamp < ?",
            [&cutoff_time.to_string()]
        )?;
        
        if deleted > 0 {
            info!("Cleaned up {} old price records from database", deleted);
        }
        
        Ok(())
    }

    /// Perform periodic cleanup if needed
    pub fn maybe_cleanup(&self) {
        static LAST_CLEANUP: AtomicU64 = AtomicU64::new(0);
        
        if let Ok(current_time) = get_current_timestamp() {
            let last_cleanup = LAST_CLEANUP.load(Ordering::Relaxed);
            if current_time - last_cleanup > CLEANUP_INTERVAL_SECONDS {
                match self.cleanup_old_prices() {
                    Ok(_) => debug!("Database cleanup completed"),
                    Err(e) => error!("Failed to cleanup old prices: {}", e),
                }
                LAST_CLEANUP.store(current_time, Ordering::Relaxed);
            }
        }
    }
}