use crate::config::PRICE_HISTORY_DAYS;
use crate::errors::{BotError, BotResult};
use crate::utils::{
    calculate_percentage_change, get_change_arrow, get_current_timestamp, validate_crypto_name,
    validate_price,
};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, error, info};

const CLEANUP_INTERVAL_SECONDS: u64 = 86400; // 24 hours

/// Database abstraction layer for price data
#[derive(Debug)]
pub struct PriceDatabase {
    pool: Pool<SqliteConnectionManager>,
}

impl PriceDatabase {
    pub fn new(db_path: &str) -> BotResult<Self> {
        let manager = SqliteConnectionManager::file(db_path).with_init(|c| {
            c.execute_batch(
                "PRAGMA journal_mode = WAL;  -- Enable WAL mode
                     PRAGMA busy_timeout = 30000;  -- Set busy timeout to 30s
                     PRAGMA synchronous = NORMAL; -- Faster sync
                     CREATE TABLE IF NOT EXISTS prices (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         crypto_name TEXT NOT NULL,
                         price REAL NOT NULL,
                         timestamp INTEGER NOT NULL,
                         created_at TEXT DEFAULT CURRENT_TIMESTAMP
                     );
                     CREATE INDEX IF NOT EXISTS idx_prices_crypto_timestamp ON prices(crypto_name, timestamp);",
            )
        });

        let pool = Pool::builder()
            .max_size(10) // Allow up to 10 concurrent connections
            .build(manager)
            .map_err(|e| {
                BotError::Database(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            })?;

        Ok(Self { pool })
    }

    /// Get a database connection from the pool
    pub fn get_connection(&self) -> BotResult<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool
            .get()
            .map_err(|e| BotError::Database(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))
    }

    /// Save a price record to the database
    pub fn save_price(&self, crypto_name: &str, price: f64) -> BotResult<()> {
        // Skip invalid prices (0 or negative)
        if price <= 0.0 {
            debug!(
                "Skipping save for {} - invalid price: {}",
                crypto_name, price
            );
            return Ok(());
        }

        let conn = self.get_connection()?;
        let current_time = get_current_timestamp()?;

        let mut stmt = conn.prepare_cached(
            "INSERT INTO prices (crypto_name, price, timestamp) VALUES (?, ?, ?)",
        )?;

        stmt.execute([crypto_name, &price.to_string(), &current_time.to_string()])?;
        debug!("Saved {} price to database: ${}", crypto_name, price);
        Ok(())
    }

    /// Get the latest price for a cryptocurrency from the database
    pub fn get_latest_price(&self, crypto_name: &str) -> BotResult<f64> {
        let conn = self.get_connection()?;

        let mut stmt = conn.prepare_cached(
            "SELECT price FROM prices WHERE crypto_name = ? ORDER BY timestamp DESC LIMIT 1",
        )?;

        let price: f64 = stmt
            .query_row([crypto_name], |row| row.get(0))
            .map_err(|e| BotError::Database(e))?;

        Ok(price)
    }

    /// Get all latest prices from the database (one per crypto)
    pub fn get_all_latest_prices(&self) -> BotResult<HashMap<String, f64>> {
        let conn = self.get_connection()?;

        let mut stmt = conn.prepare_cached(
            "SELECT p.crypto_name, p.price 
             FROM prices p
             INNER JOIN (
                 SELECT crypto_name, MAX(timestamp) as max_ts
                 FROM prices GROUP BY crypto_name
             ) latest ON p.crypto_name = latest.crypto_name AND p.timestamp = latest.max_ts",
        )?;

        let mut prices = HashMap::new();
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let price: f64 = row.get(1)?;
            Ok((name, price))
        })?;

        for row in rows {
            let (name, price) = row.map_err(|e| BotError::Database(e))?;
            prices.insert(name, price);
        }

        debug!(
            "Fetched all latest prices: {:?}",
            prices.keys().collect::<Vec<_>>()
        );
        Ok(prices)
    }

    /// Get price changes for different time periods (works with both raw and aggregated data)
    pub fn get_price_changes(&self, crypto: &str, current_price: f64) -> BotResult<String> {
        info!(
            "🔍 Getting price changes for {} at ${}",
            crypto, current_price
        );
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
                debug!(
                    "Looking for {} {} data in raw table, time_ago: {}",
                    label, crypto, time_ago
                );
                self.get_price_from_raw_data(&conn, crypto, time_ago)?
            } else if seconds <= 7 * 24 * 3600 {
                // For 1-7 days old, use 1-minute aggregates
                debug!(
                    "Looking for {} {} data in 60s aggregates, time_ago: {}",
                    label, crypto, time_ago
                );
                self.get_price_from_aggregates(&conn, crypto, time_ago, 60)?
            } else if seconds < 30 * 24 * 3600 {
                // For 7-30 days old, use 5-minute aggregates
                debug!(
                    "Looking for {} {} data in 300s aggregates, time_ago: {}",
                    label, crypto, time_ago
                );
                self.get_price_from_aggregates(&conn, crypto, time_ago, 300)?
            } else {
                // For older data, use 15-minute aggregates
                debug!(
                    "Looking for {} {} data in 900s aggregates, time_ago: {}",
                    label, crypto, time_ago
                );
                self.get_price_from_aggregates(&conn, crypto, time_ago, 900)?
            };

            // Only add the change if we have data for that time period
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

    /// Get price from raw data table
    fn get_price_from_raw_data(
        &self,
        conn: &Connection,
        crypto: &str,
        time_ago: u64,
    ) -> BotResult<Option<f64>> {
        let mut stmt = conn.prepare_cached(
            "SELECT price FROM prices WHERE crypto_name = ? AND timestamp >= ? ORDER BY timestamp ASC LIMIT 1"
        )?;

        let rows = stmt.query_map([crypto, &time_ago.to_string()], |row| Ok(row.get(0)?))?;

        let mut prices = rows.collect::<Result<Vec<f64>, _>>()?;
        Ok(prices.pop())
    }

    /// Get price from aggregated data table
    fn get_price_from_aggregates(
        &self,
        conn: &Connection,
        crypto: &str,
        time_ago: u64,
        bucket_duration: u64,
    ) -> BotResult<Option<f64>> {
        // Find the bucket that contains or is closest to the target time
        // We want the bucket where bucket_start <= time_ago < bucket_start + bucket_duration
        // Or the closest bucket if no exact match
        let mut stmt = conn.prepare_cached(
            "SELECT open_price FROM price_aggregates 
             WHERE crypto_name = ? AND bucket_duration = ?
             AND bucket_start <= ?
             ORDER BY bucket_start DESC LIMIT 1",
        )?;

        let rows = stmt.query_map(
            [crypto, &bucket_duration.to_string(), &time_ago.to_string()],
            |row| Ok(row.get(0)?),
        )?;

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

        let mut stmt = match conn.prepare_cached(
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

    /// Get price history for charting (up to specified days)
    /// Returns vector of (timestamp, price) tuples
    /// Get price history for charting (up to specified days)
    /// Returns vector of (timestamp, price) tuples
    pub fn get_price_history(&self, crypto_name: &str, days: u64) -> BotResult<Vec<(i64, f64)>> {
        let conn = self.get_connection()?;
        let current_time = get_current_timestamp()?;
        let start_time = current_time - (days * 86400);

        // Strategy: Combine aggregated history + Recent raw data
        // 1. Fetch best available aggregates
        // 2. Fetch raw data that is newer than the newest aggregate
        // 3. Merge and sort

        let mut history = Vec::new();
        let mut last_aggregated_time = start_time as i64;

        // 1. Fetch Aggregates
        // Try to get 5-minute buckets first, then 1-minute (fallback), then 15m, then 1h
        // This ensures we get the best resolution available for the time range
        let bucket_durations = vec![300, 60, 900, 3600];

        for duration in bucket_durations {
            let mut stmt = conn.prepare_cached(
                "SELECT bucket_start, open_price FROM price_aggregates 
                 WHERE crypto_name = ? AND bucket_duration = ? AND bucket_start >= ? 
                 ORDER BY bucket_start ASC",
            )?;

            let rows = stmt.query_map(
                [crypto_name, &duration.to_string(), &start_time.to_string()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)),
            )?;

            let data: Vec<(i64, f64)> = rows.collect::<Result<Vec<_>, _>>()?;

            if !data.is_empty() {
                // If we found data, record the last timestamp so we know where to start raw data
                if let Some((ts, _)) = data.last() {
                    last_aggregated_time = *ts;
                }
                history = data;
                debug!(
                    "Found {} aggregated points for {} using {}-second buckets",
                    history.len(),
                    crypto_name,
                    duration
                );
                break;
            }
        }

        // 2. Fetch Raw Data (Newer than last aggregate)
        // This covers the gap from the last cleanup/aggregation run to NOW
        // Also helps if no aggregates exist at all (last_aggregated_time == start_time)

        debug!(
            "Fetching raw prices for {} newer than {}",
            crypto_name, last_aggregated_time
        );

        let mut stmt = conn.prepare_cached(
            "SELECT timestamp, price FROM prices 
             WHERE crypto_name = ? AND timestamp > ? 
             ORDER BY timestamp ASC",
        )?;

        let rows = stmt.query_map([crypto_name, &last_aggregated_time.to_string()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
        })?;

        let raw_data: Vec<(i64, f64)> = rows.collect::<Result<Vec<_>, _>>()?;

        // Downsample raw data if there's too much (e.g., if we have no aggregates and 30 days of raw data)
        // But typically this will just be the last 24h ~ few hundred points max
        if raw_data.len() > 1000 {
            let step = raw_data.len() / 500;
            let downsampled = raw_data
                .into_iter()
                .enumerate()
                .filter(|(i, _)| i % step == 0)
                .map(|(_, val)| val);
            history.extend(downsampled);
        } else {
            history.extend(raw_data);
        }

        // 3. Final Sort (just in case, though append should be sorted)
        history.sort_by_key(|k| k.0);

        Ok(history)
    }

    /// Clean up old price records from the database
    pub fn cleanup_old_prices(&self) -> BotResult<()> {
        let conn = self.get_connection()?;

        // Keep only the last 60 days of data
        let cutoff_time = get_current_timestamp()? - (PRICE_HISTORY_DAYS * 24 * 3600);

        let deleted = conn.execute(
            "DELETE FROM prices WHERE timestamp < ?",
            [&cutoff_time.to_string()],
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
