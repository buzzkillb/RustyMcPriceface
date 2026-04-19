use crate::config::PRICE_HISTORY_DAYS;
use crate::errors::{BotError, BotResult};
use crate::utils::{
    calculate_percentage_change, get_change_arrow, get_current_timestamp, validate_crypto_name,
    validate_price,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, error, info};

const CLEANUP_INTERVAL_SECONDS: u64 = 86400;

#[derive(Debug, Clone)]
pub struct PriceDatabase {
    pool: PgPool,
}

impl PriceDatabase {
    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    pub async fn new(database_url: &str) -> BotResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .min_connections(4)
            .connect(database_url)
            .await
            .map_err(|e| BotError::Database(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS prices (
                id BIGSERIAL PRIMARY KEY,
                crypto_name TEXT NOT NULL,
                price REAL NOT NULL,
                timestamp BIGINT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_prices_crypto_timestamp_unique ON prices(crypto_name, timestamp);
            CREATE INDEX IF NOT EXISTS idx_prices_crypto_timestamp ON prices(crypto_name, timestamp);
            CREATE TABLE IF NOT EXISTS price_aggregates (
                id BIGSERIAL PRIMARY KEY,
                crypto_name TEXT NOT NULL,
                bucket_start BIGINT NOT NULL,
                bucket_duration INTEGER NOT NULL,
                open_price REAL NOT NULL,
                high_price REAL NOT NULL,
                low_price REAL NOT NULL,
                close_price REAL NOT NULL,
                avg_price REAL NOT NULL,
                sample_count INTEGER NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_aggregates_crypto_bucket 
                ON price_aggregates(crypto_name, bucket_start, bucket_duration);
            "#,
        )
        .execute(&pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        Ok(Self { pool })
    }

    pub async fn save_price(&self, crypto_name: &str, price: f64) -> BotResult<()> {
        if price <= 0.0 {
            debug!(
                "Skipping save for {} - invalid price: {}",
                crypto_name, price
            );
            return Ok(());
        }

        let current_time = get_current_timestamp()?;

        sqlx::query(
            "INSERT INTO prices (crypto_name, price, timestamp) VALUES ($1, $2, $3) ON CONFLICT (crypto_name, timestamp) DO UPDATE SET price = EXCLUDED.price",
        )
        .bind(crypto_name)
        .bind(price)
        .bind(current_time as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        debug!("Saved {} price to database: ${}", crypto_name, price);
        Ok(())
    }

    pub async fn get_latest_price(&self, crypto_name: &str) -> BotResult<f64> {
        let row: (f64,) = sqlx::query_as(
            "SELECT price FROM prices WHERE crypto_name = $1 ORDER BY timestamp DESC LIMIT 1",
        )
        .bind(crypto_name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        Ok(row.0)
    }

    pub async fn get_all_latest_prices(&self) -> BotResult<HashMap<String, f64>> {
        let rows: Vec<(String, f64)> = sqlx::query_as(
            r#"
            SELECT p.crypto_name, p.price 
            FROM prices p
            INNER JOIN (
                SELECT crypto_name, MAX(timestamp) as max_ts
                FROM prices GROUP BY crypto_name
            ) latest ON p.crypto_name = latest.crypto_name AND p.timestamp = latest.max_ts
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        let mut prices = HashMap::new();
        for (name, price) in rows {
            prices.insert(name, price);
        }

        debug!(
            "Fetched all latest prices: {:?}",
            prices.keys().collect::<Vec<_>>()
        );
        Ok(prices)
    }

    pub async fn get_price_changes(&self, crypto: &str, current_price: f64) -> BotResult<String> {
        info!(
            "🔍 Getting price changes for {} at ${}",
            crypto, current_price
        );
        validate_crypto_name(crypto)?;
        validate_price(current_price)?;

        let current_time = get_current_timestamp()?;

        let mut changes = Vec::new();

        let periods = vec![
            (3600, "1h"),
            (43200, "12h"),
            (86400, "24h"),
            (604800, "7d"),
            (2592000, "30d"),
        ];

        for (seconds, label) in periods {
            let time_ago = if current_time >= seconds {
                current_time - seconds
            } else {
                debug!(
                    "Clock appears to have gone backwards, skipping {} price lookup",
                    label
                );
                continue;
            };

            let old_price = if seconds <= 24 * 3600 {
                self.get_price_from_raw_data(crypto, time_ago as i64)
                    .await?
            } else if seconds <= 7 * 24 * 3600 {
                self.get_price_from_aggregates(crypto, time_ago as i64, 60)
                    .await?
            } else if seconds < 30 * 24 * 3600 {
                self.get_price_from_aggregates(crypto, time_ago as i64, 300)
                    .await?
            } else {
                self.get_price_from_aggregates(crypto, time_ago as i64, 900)
                    .await?
            };

            if let Some(price) = old_price {
                debug!(
                    "Found {} {} price: ${} (current: ${})",
                    label, crypto, price, current_price
                );
                let change_percent = calculate_percentage_change(current_price, price)?;
                let arrow = get_change_arrow(change_percent);
                let sign = if change_percent >= 0.0 { "+" } else { "" };
                changes.push(format!(
                    "{} {}{:.2}% ({})",
                    arrow, sign, change_percent, label
                ));
            } else {
                debug!(
                    "No {} {} price data found for time_ago: {}",
                    label, crypto, time_ago
                );
            }
        }

        info!(
            "Found {} price changes for {}: {:?}",
            changes.len(),
            crypto,
            changes
        );

        if changes.is_empty() {
            Ok("🔄 Building history".to_string())
        } else {
            Ok(format!(" {}", changes.join(" | ")))
        }
    }

    async fn get_price_from_raw_data(&self, crypto: &str, time_ago: i64) -> BotResult<Option<f64>> {
        let row: Option<(f64,)> = sqlx::query_as(
            "SELECT price FROM prices WHERE crypto_name = $1 AND timestamp >= $2 ORDER BY timestamp ASC LIMIT 1",
        )
        .bind(crypto)
        .bind(time_ago)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        Ok(row.map(|r| r.0))
    }

    async fn get_price_from_aggregates(
        &self,
        crypto: &str,
        time_ago: i64,
        bucket_duration: i64,
    ) -> BotResult<Option<f64>> {
        let row: Option<(f64,)> = sqlx::query_as(
            "SELECT open_price FROM price_aggregates 
             WHERE crypto_name = $1 AND bucket_duration = $2
             AND bucket_start <= $3
             ORDER BY bucket_start DESC LIMIT 1",
        )
        .bind(crypto)
        .bind(bucket_duration)
        .bind(time_ago)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        Ok(row.map(|r| r.0))
    }

    pub async fn get_price_indicator(
        &self,
        crypto_name: &str,
        current_price: f64,
    ) -> (String, f64) {
        let current_time = match get_current_timestamp() {
            Ok(time) => time as i64,
            Err(_) => return ("🔄".to_string(), 0.0),
        };

        let one_hour_ago = current_time - 3600;

        let row: Option<(f64,)> = sqlx::query_as(
            "SELECT price FROM prices WHERE crypto_name = $1 AND timestamp >= $2 ORDER BY timestamp ASC LIMIT 1",
        )
        .bind(crypto_name)
        .bind(one_hour_ago)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            debug!("Failed to get price indicator: {}", e);
            BotError::Database(e.to_string())
        }).ok().flatten();

        if let Some((oldest_price,)) = row {
            match calculate_percentage_change(current_price, oldest_price) {
                Ok(change_percent) => {
                    let arrow = get_change_arrow(change_percent);
                    return (arrow.to_string(), change_percent);
                }
                Err(_) => return ("🔄".to_string(), 0.0),
            }
        }

        ("🔄".to_string(), 0.0)
    }

    pub async fn get_price_history(
        &self,
        crypto_name: &str,
        days: u64,
    ) -> BotResult<Vec<(i64, f64)>> {
        let current_time = get_current_timestamp()? as i64;
        let start_time = current_time - (days as i64 * 86400);

        let mut history = Vec::new();
        let mut last_aggregated_time = start_time;

        let bucket_durations = vec![300, 60, 900, 3600];

        for duration in bucket_durations {
            let rows: Vec<(i64, f64)> = sqlx::query_as(
                "SELECT bucket_start, open_price FROM price_aggregates 
                 WHERE crypto_name = $1 AND bucket_duration = $2 AND bucket_start >= $3 
                 ORDER BY bucket_start ASC",
            )
            .bind(crypto_name)
            .bind(duration)
            .bind(start_time)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| BotError::Database(e.to_string()))?;

            if !rows.is_empty() {
                if let Some((ts, _)) = rows.last() {
                    last_aggregated_time = *ts;
                }
                history = rows;
                debug!(
                    "Found {} aggregated points for {} using {}-second buckets",
                    history.len(),
                    crypto_name,
                    duration
                );
                break;
            }
        }

        debug!(
            "Fetching raw prices for {} newer than {}",
            crypto_name, last_aggregated_time
        );

        let rows: Vec<(i64, f64)> = sqlx::query_as(
            "SELECT timestamp, price FROM prices 
             WHERE crypto_name = $1 AND timestamp > $2 
             ORDER BY timestamp ASC",
        )
        .bind(crypto_name)
        .bind(last_aggregated_time)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| BotError::Database(e.to_string()))?;

        if rows.len() > 1000 {
            let step = rows.len() / 500;
            let downsampled = rows
                .into_iter()
                .enumerate()
                .filter(|(i, _)| i % step == 0)
                .map(|(_, val)| val);
            history.extend(downsampled);
        } else {
            history.extend(rows);
        }

        history.sort_by_key(|k| k.0);

        Ok(history)
    }

    pub async fn cleanup_old_prices(&self) -> BotResult<()> {
        let cutoff_time = get_current_timestamp()? - (PRICE_HISTORY_DAYS * 24 * 3600);

        let result = sqlx::query("DELETE FROM prices WHERE timestamp < $1")
            .bind(cutoff_time as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| BotError::Database(e.to_string()))?;

        if result.rows_affected() > 0 {
            info!(
                "Cleaned up {} old price records from database",
                result.rows_affected()
            );
        }

        Ok(())
    }

    pub async fn maybe_cleanup(&self) {
        static LAST_CLEANUP: AtomicU64 = AtomicU64::new(0);

        if let Ok(current_time) = get_current_timestamp() {
            let last_cleanup = LAST_CLEANUP.load(Ordering::Relaxed);
            if current_time - last_cleanup > CLEANUP_INTERVAL_SECONDS {
                if let Err(e) = self.cleanup_old_prices().await {
                    error!("Failed to cleanup old prices: {}", e);
                }
                LAST_CLEANUP.store(current_time, Ordering::Relaxed);
            }
        }
    }
}
