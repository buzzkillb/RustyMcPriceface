use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::json;

/// Health check state shared across the application
#[derive(Debug, Clone)]
pub struct HealthState {
    pub last_price_update: Arc<AtomicU64>,
    pub last_db_write: Arc<AtomicU64>,
    pub last_discord_update: Arc<AtomicU64>,
    pub consecutive_failures: Arc<AtomicU64>,
    pub gateway_failures: Arc<AtomicU64>,
    pub bot_name: String,
}

impl HealthState {
    pub fn new(bot_name: String) -> Self {
        Self {
            last_price_update: Arc::new(AtomicU64::new(0)),
            last_db_write: Arc::new(AtomicU64::new(0)),
            last_discord_update: Arc::new(AtomicU64::new(0)),
            consecutive_failures: Arc::new(AtomicU64::new(0)),
            gateway_failures: Arc::new(AtomicU64::new(0)),
            bot_name,
        }
    }

    pub fn update_price_timestamp(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_price_update.store(now, Ordering::Relaxed);
    }

    pub fn update_db_timestamp(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_db_write.store(now, Ordering::Relaxed);
    }

    pub fn update_discord_timestamp(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_discord_update.store(now, Ordering::Relaxed);
    }

    pub fn increment_failures(&self) {
        self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reset_failures(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }

    pub fn increment_gateway_failures(&self) {
        self.gateway_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reset_gateway_failures(&self) {
        self.gateway_failures.store(0, Ordering::Relaxed);
    }

    pub fn is_healthy(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let last_price = self.last_price_update.load(Ordering::Relaxed);
        let last_db = self.last_db_write.load(Ordering::Relaxed);
        let last_discord = self.last_discord_update.load(Ordering::Relaxed);
        let failures = self.consecutive_failures.load(Ordering::Relaxed);
        let gateway_failures = self.gateway_failures.load(Ordering::Relaxed);

        // Consider unhealthy if:
        // - No price update in last 5 minutes
        // - No database write in last 5 minutes
        // - No Discord update in last 3 minutes (more aggressive for gateway issues)
        // - More than 3 consecutive failures
        // - More than 5 gateway failures (indicates broken Discord connection)
        let price_stale = now.saturating_sub(last_price) > 300; // 5 minutes
        let db_stale = now.saturating_sub(last_db) > 300; // 5 minutes
        let discord_stale = now.saturating_sub(last_discord) > 180; // 3 minutes (more aggressive)
        let too_many_failures = failures > 3;
        let gateway_broken = gateway_failures > 5;

        !price_stale && !db_stale && !discord_stale && !too_many_failures && !gateway_broken
    }

    pub fn to_json(&self) -> serde_json::Value {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let last_price = self.last_price_update.load(Ordering::Relaxed);
        let last_db = self.last_db_write.load(Ordering::Relaxed);
        let last_discord = self.last_discord_update.load(Ordering::Relaxed);
        let failures = self.consecutive_failures.load(Ordering::Relaxed);
        let gateway_failures = self.gateway_failures.load(Ordering::Relaxed);

        json!({
            "bot_name": self.bot_name,
            "healthy": self.is_healthy(),
            "timestamp": now,
            "last_price_update": last_price,
            "last_db_write": last_db,
            "last_discord_update": last_discord,
            "consecutive_failures": failures,
            "gateway_failures": gateway_failures,
            "seconds_since_price_update": now.saturating_sub(last_price),
            "seconds_since_db_write": now.saturating_sub(last_db),
            "seconds_since_discord_update": now.saturating_sub(last_discord)
        })
    }
}