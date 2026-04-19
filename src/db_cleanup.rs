use crate::config::{
    FIFTEEN_MINUTE_DATA_RETENTION_DAYS, FIVE_MINUTE_DATA_RETENTION_DAYS,
    MINUTE_DATA_RETENTION_DAYS, RAW_DATA_RETENTION_HOURS,
};
use crate::database::PriceDatabase;
use crate::errors::{BotError, BotResult};
use crate::health::HealthState;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

pub struct DatabaseCleanup {
    health: Arc<HealthState>,
    pool: PgPool,
}

impl DatabaseCleanup {
    pub fn new(database: &Arc<PriceDatabase>) -> Self {
        let health = Arc::new(HealthState::new("DB-CLEANUP".to_string()));
        Self {
            health,
            pool: database.pool(),
        }
    }

    async fn aggregate_data(
        &self,
        bucket_duration_seconds: i64,
        older_than_seconds: i64,
    ) -> BotResult<u64> {
        info!(
            "   🔍 Checking for data older than {} seconds to aggregate into {}-second buckets",
            older_than_seconds, bucket_duration_seconds
        );

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| BotError::SystemTime(format!("System time error: {}", e)))?
            .as_secs() as i64;

        let cutoff_time = current_time - older_than_seconds;

        let batch_size: i64 = 100;
        let mut total_aggregated = 0u64;
        let mut batch_number = 0;

        loop {
            batch_number += 1;
            debug!(
                "   📦 Processing batch {} for {}-second aggregation",
                batch_number, bucket_duration_seconds
            );

            let rows: Vec<(String, i64, f64, f64, f64, i64)> = sqlx::query_as(
                r#"
                SELECT crypto_name, 
                       (timestamp / $1) * $1 as bucket_start,
                       MIN(price) as low_price,
                       MAX(price) as high_price,
                       AVG(price) as avg_price,
                       COUNT(*) as sample_count
                FROM prices 
                WHERE timestamp < $2 
                AND NOT EXISTS (
                    SELECT 1 FROM price_aggregates pa 
                    WHERE pa.crypto_name = prices.crypto_name 
                    AND pa.bucket_start = (prices.timestamp / $1) * $1
                    AND pa.bucket_duration = $1
                )
                GROUP BY crypto_name, bucket_start
                HAVING COUNT(*) > 0
                ORDER BY crypto_name, bucket_start
                LIMIT $3
                "#,
            )
            .bind(bucket_duration_seconds)
            .bind(cutoff_time)
            .bind(batch_size)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| BotError::Database(e.to_string()))?;

            if rows.is_empty() {
                debug!(
                    "   ✅ No more data to aggregate for {}-second buckets",
                    bucket_duration_seconds
                );
                break;
            }

            debug!(
                "   📊 Found {} records to aggregate in batch {}",
                rows.len(),
                batch_number
            );

            let mut batch_count = 0u64;

            for (crypto_name, bucket_start, low_price, high_price, avg_price, sample_count) in rows
            {
                let open_price = self
                    .get_bucket_open_price(
                        crypto_name.clone(),
                        bucket_start,
                        bucket_duration_seconds,
                    )
                    .await?;
                let close_price = self
                    .get_bucket_close_price(
                        crypto_name.clone(),
                        bucket_start,
                        bucket_duration_seconds,
                    )
                    .await?;

                sqlx::query(
                    r#"INSERT INTO price_aggregates 
                       (crypto_name, bucket_start, bucket_duration, open_price, high_price, low_price, close_price, avg_price, sample_count)
                       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
                )
                .bind(&crypto_name)
                .bind(bucket_start)
                .bind(bucket_duration_seconds)
                .bind(open_price)
                .bind(high_price)
                .bind(low_price)
                .bind(close_price)
                .bind(avg_price)
                .bind(sample_count)
                .execute(&self.pool)
                .await
                .map_err(|e| BotError::Database(e.to_string()))?;

                batch_count += 1;
            }

            total_aggregated += batch_count;

            debug!(
                "   ✅ Batch {} completed: {} buckets aggregated (total: {})",
                batch_number, batch_count, total_aggregated
            );

            sleep(Duration::from_millis(100)).await;
        }

        if total_aggregated > 0 {
            info!(
                "📊 Aggregated {} buckets of {}-second data",
                total_aggregated, bucket_duration_seconds
            );
        }

        Ok(total_aggregated)
    }

    async fn get_bucket_open_price(
        &self,
        crypto_name: String,
        bucket_start: i64,
        bucket_duration: i64,
    ) -> BotResult<f64> {
        let bucket_end = bucket_start + bucket_duration;

        let row: (f64,) = sqlx::query_as(
            "SELECT price FROM prices 
             WHERE crypto_name = $1 AND timestamp >= $2 AND timestamp < $3
             ORDER BY timestamp ASC LIMIT 1",
        )
        .bind(&crypto_name)
        .bind(bucket_start)
        .bind(bucket_end)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        Ok(row.0)
    }

    async fn get_bucket_close_price(
        &self,
        crypto_name: String,
        bucket_start: i64,
        bucket_duration: i64,
    ) -> BotResult<f64> {
        let bucket_end = bucket_start + bucket_duration;

        let row: (f64,) = sqlx::query_as(
            "SELECT price FROM prices 
             WHERE crypto_name = $1 AND timestamp >= $2 AND timestamp < $3
             ORDER BY timestamp DESC LIMIT 1",
        )
        .bind(&crypto_name)
        .bind(bucket_start)
        .bind(bucket_end)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        Ok(row.0)
    }

    async fn cleanup_aggregated_raw_data(&self, older_than_seconds: i64) -> BotResult<u64> {
        info!(
            "   🔍 Checking how many raw records are older than {} seconds",
            older_than_seconds
        );

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| BotError::SystemTime(format!("System time error: {}", e)))?
            .as_secs() as i64;

        let cutoff_time = current_time - older_than_seconds;

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM prices WHERE timestamp < $1")
            .bind(cutoff_time)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| BotError::Database(e.to_string()))?;

        info!(
            "   📊 Found {} raw records older than {} seconds",
            count.0, older_than_seconds
        );

        if count.0 == 0 {
            info!("   ✅ No old raw data to clean up");
            return Ok(0);
        }

        info!("   🗑️ Deleting old raw records in batches...");

        let mut total_deleted = 0i64;
        let batch_size: i64 = 10000;

        loop {
            let result = sqlx::query(
                r#"DELETE FROM prices 
                 WHERE id IN (
                     SELECT p.id FROM prices p
                     WHERE p.timestamp < $1 
                     AND EXISTS (
                         SELECT 1 FROM price_aggregates pa 
                         WHERE pa.crypto_name = p.crypto_name 
                         AND pa.bucket_start <= p.timestamp 
                         AND pa.bucket_start + pa.bucket_duration > p.timestamp
                     )
                     LIMIT $2
                 )"#,
            )
            .bind(cutoff_time)
            .bind(batch_size)
            .execute(&self.pool)
            .await
            .map_err(|e| BotError::Database(e.to_string()))?;

            let deleted = result.rows_affected() as i64;
            if deleted == 0 {
                break;
            }

            total_deleted += deleted;
            info!("   Deleted {} records (total: {})", deleted, total_deleted);
        }

        info!(
            "   {} raw price records   ✅ Successfully deleted older than {} seconds",
            total_deleted, older_than_seconds
        );

        Ok(total_deleted as u64)
    }

    async fn cleanup_old_aggregates(
        &self,
        bucket_duration_seconds: i64,
        older_than_seconds: i64,
    ) -> BotResult<u64> {
        info!(
            "   🧹 Cleaning up {}-second aggregates older than {} seconds",
            bucket_duration_seconds, older_than_seconds
        );

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| BotError::SystemTime(format!("System time error: {}", e)))?
            .as_secs() as i64;

        let cutoff_time = current_time - older_than_seconds;

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM price_aggregates WHERE bucket_start < $1 AND bucket_duration = $2",
        )
        .bind(cutoff_time)
        .bind(bucket_duration_seconds)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        if count.0 > 0 {
            info!(
                "   🗑️ Deleting {} old {}-second aggregate records...",
                count.0, bucket_duration_seconds
            );
        } else {
            info!(
                "   ✅ No old {}-second aggregates to clean up",
                bucket_duration_seconds
            );
        }

        let result = sqlx::query(
            "DELETE FROM price_aggregates WHERE bucket_start < $1 AND bucket_duration = $2",
        )
        .bind(cutoff_time)
        .bind(bucket_duration_seconds)
        .execute(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        if result.rows_affected() > 0 {
            info!(
                "   ✅ Deleted {} aggregated records ({}-second buckets)",
                result.rows_affected(),
                bucket_duration_seconds
            );
        }

        Ok(result.rows_affected())
    }

    async fn get_database_stats(&self) -> BotResult<()> {
        let raw_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM prices")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| BotError::Database(e.to_string()))?;

        let aggregates: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT bucket_duration, COUNT(*) FROM price_aggregates GROUP BY bucket_duration ORDER BY bucket_duration",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        info!("📊 Database Statistics:");
        info!("   Raw price records: {}", raw_count.0);

        for (duration, count) in aggregates {
            info!("   {}-second aggregates: {}", duration, count);
        }

        Ok(())
    }

    async fn aggregate_buckets(
        &self,
        source_duration: i64,
        target_duration: i64,
        older_than_seconds: i64,
    ) -> BotResult<u64> {
        info!(
            "   🔍 Aggregating {}-second buckets older than {} seconds into {}-second buckets",
            source_duration, older_than_seconds, target_duration
        );

        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| BotError::SystemTime(format!("System time error: {}", e)))?
            .as_secs() as i64;

        let cutoff_time = current_time - older_than_seconds;

        let batch_size: i64 = 100;
        let mut total_aggregated = 0u64;
        let mut batch_number = 0;

        loop {
            batch_number += 1;

            let rows: Vec<(String, i64, f64, f64, f64, i64)> = sqlx::query_as(
                r#"
                SELECT crypto_name, 
                       (bucket_start / $1) * $1 as new_bucket_start,
                       MIN(low_price) as low_price,
                       MAX(high_price) as high_price,
                       SUM(avg_price * sample_count) / SUM(sample_count) as avg_price,
                       SUM(sample_count) as sample_count
                FROM price_aggregates 
                WHERE bucket_duration = $2
                AND bucket_start < $3
                AND NOT EXISTS (
                    SELECT 1 FROM price_aggregates pa 
                    WHERE pa.crypto_name = price_aggregates.crypto_name 
                    AND pa.bucket_start = (price_aggregates.bucket_start / $1) * $1
                    AND pa.bucket_duration = $1
                )
                GROUP BY crypto_name, new_bucket_start
                HAVING COUNT(*) > 0
                ORDER BY crypto_name, new_bucket_start
                LIMIT $4
                "#,
            )
            .bind(target_duration)
            .bind(source_duration)
            .bind(cutoff_time)
            .bind(batch_size)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| BotError::Database(e.to_string()))?;

            if rows.is_empty() {
                break;
            }

            debug!(
                "   📊 Found {} bucket groups to aggregate in batch {}",
                rows.len(),
                batch_number
            );

            let mut batch_count = 0u64;

            for (crypto_name, bucket_start, low_price, high_price, avg_price, sample_count) in rows
            {
                let bucket_end = bucket_start + target_duration;

                let open_price: f64 = match sqlx::query_as(
                    "SELECT open_price FROM price_aggregates 
                     WHERE crypto_name = $1 AND bucket_duration = $2 
                     AND bucket_start >= $3 AND bucket_start < $4
                     ORDER BY bucket_start ASC LIMIT 1",
                )
                .bind(&crypto_name)
                .bind(source_duration)
                .bind(bucket_start)
                .bind(bucket_end)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| BotError::Database(e.to_string()))?
                {
                    Some((price,)) => price,
                    None => {
                        warn!("Failed to get open price for {}, using avg", crypto_name);
                        avg_price
                    }
                };

                let close_price: f64 = match sqlx::query_as(
                    "SELECT close_price FROM price_aggregates 
                     WHERE crypto_name = $1 AND bucket_duration = $2 
                     AND bucket_start >= $3 AND bucket_start < $4
                     ORDER BY bucket_start DESC LIMIT 1",
                )
                .bind(&crypto_name)
                .bind(source_duration)
                .bind(bucket_start)
                .bind(bucket_end)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| BotError::Database(e.to_string()))?
                {
                    Some((price,)) => price,
                    None => {
                        warn!("Failed to get close price for {}, using avg", crypto_name);
                        avg_price
                    }
                };

                sqlx::query(
                    r#"INSERT INTO price_aggregates 
                       (crypto_name, bucket_start, bucket_duration, open_price, high_price, low_price, close_price, avg_price, sample_count)
                       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
                )
                .bind(&crypto_name)
                .bind(bucket_start)
                .bind(target_duration)
                .bind(open_price)
                .bind(high_price)
                .bind(low_price)
                .bind(close_price)
                .bind(avg_price)
                .bind(sample_count)
                .execute(&self.pool)
                .await
                .map_err(|e| BotError::Database(e.to_string()))?;

                batch_count += 1;
            }

            total_aggregated += batch_count;
            sleep(Duration::from_millis(50)).await;
        }

        if total_aggregated > 0 {
            info!(
                "📊 Aggregated {} buckets of {}-second data (sourced from {}-second buckets)",
                total_aggregated, target_duration, source_duration
            );
        }

        Ok(total_aggregated)
    }

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

    async fn perform_cleanup_attempt(&self) -> BotResult<()> {
        info!("🧹 Starting database cleanup cycle...");
        self.health.update_price_timestamp();

        info!("📊 Step 2/7: Aggregating raw data into 1-minute buckets...");
        let aggregated_1m = self
            .aggregate_data(60, (RAW_DATA_RETENTION_HOURS * 3600) as i64)
            .await?;

        info!("📊 Step 3/7: Aggregating 1-minute data into 5-minute buckets...");
        let aggregated_5m = self
            .aggregate_buckets(60, 300, (MINUTE_DATA_RETENTION_DAYS * 24 * 3600) as i64)
            .await?;

        info!("📊 Step 4/7: Aggregating 5-minute data into 15-minute buckets...");
        let aggregated_15m = self
            .aggregate_buckets(
                300,
                900,
                (FIVE_MINUTE_DATA_RETENTION_DAYS * 24 * 3600) as i64,
            )
            .await?;

        info!("🗑️ Step 5/7: Cleaning up old raw data (older than 24 hours)...");
        let deleted_raw = self
            .cleanup_aggregated_raw_data((RAW_DATA_RETENTION_HOURS * 3600) as i64)
            .await?;

        info!("🗑️ Step 6/7: Cleaning up old aggregated data...");
        let deleted_1m = self
            .cleanup_old_aggregates(60, (MINUTE_DATA_RETENTION_DAYS * 24 * 3600) as i64)
            .await?;
        let deleted_5m = self
            .cleanup_old_aggregates(300, (FIVE_MINUTE_DATA_RETENTION_DAYS * 24 * 3600) as i64)
            .await?;
        let deleted_15m = self
            .cleanup_old_aggregates(900, (FIFTEEN_MINUTE_DATA_RETENTION_DAYS * 24 * 3600) as i64)
            .await?;

        let total_deleted = deleted_raw + deleted_1m + deleted_5m + deleted_15m;
        if total_deleted > 1000 {
            info!(
                "🔧 Step 7/7: Skipping vacuum for PostgreSQL (deleted {} records)",
                total_deleted
            );
        } else {
            info!(
                "⏭️ Step 7/7: Skipping vacuum (only {} records deleted)",
                total_deleted
            );
        }

        self.health.update_db_timestamp();

        info!("📈 Generating final database statistics...");
        self.get_database_stats().await?;

        info!("✅ Cleanup cycle completed:");
        info!(
            "   📊 Aggregated: {}x1m + {}x5m + {}x15m buckets",
            aggregated_1m, aggregated_5m, aggregated_15m
        );
        info!("   🗑️ Deleted: {} total records", total_deleted);

        Ok(())
    }

    pub async fn run(&self) -> BotResult<()> {
        let interval_hours = std::env::var("CLEANUP_INTERVAL_HOURS")
            .unwrap_or_else(|_| "24".to_string())
            .parse::<u64>()
            .unwrap_or(24);

        let interval = Duration::from_secs(interval_hours * 3600);

        info!("🚀 Database cleanup service started");
        info!("⏰ Cleanup interval: {} hours", interval_hours);

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
