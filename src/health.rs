use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Health check state shared across the application
#[derive(Debug, Clone)]
pub struct HealthState {
    pub last_price_update: Arc<AtomicU64>,
    pub last_db_write: Arc<AtomicU64>,
    pub last_discord_update: Arc<AtomicU64>,
    pub last_discord_test: Arc<AtomicU64>,
    pub consecutive_failures: Arc<AtomicU64>,
    pub gateway_failures: Arc<AtomicU64>,
    pub discord_test_failures: Arc<AtomicU64>,
    pub start_time: Arc<AtomicU64>,
    pub bot_name: String,
}

impl HealthState {
    pub fn new(bot_name: String) -> Self {
        let start = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            last_price_update: Arc::new(AtomicU64::new(0)),
            last_db_write: Arc::new(AtomicU64::new(0)),
            last_discord_update: Arc::new(AtomicU64::new(0)),
            last_discord_test: Arc::new(AtomicU64::new(0)),
            consecutive_failures: Arc::new(AtomicU64::new(0)),
            gateway_failures: Arc::new(AtomicU64::new(0)),
            discord_test_failures: Arc::new(AtomicU64::new(0)),
            start_time: Arc::new(AtomicU64::new(start)),
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

    pub fn update_discord_test_timestamp(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_discord_test.store(now, Ordering::Relaxed);
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

    pub fn increment_discord_test_failures(&self) {
        self.discord_test_failures.fetch_add(1, Ordering::Relaxed);
    }

    pub fn reset_discord_test_failures(&self) {
        self.discord_test_failures.store(0, Ordering::Relaxed);
    }

    pub fn get_uptime_seconds(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let start = self.start_time.load(Ordering::Relaxed);
        now.saturating_sub(start)
    }

    pub fn is_healthy(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let start_time = self.start_time.load(Ordering::Relaxed);
        let last_price = self.last_price_update.load(Ordering::Relaxed);
        let last_db = self.last_db_write.load(Ordering::Relaxed);
        let last_discord = self.last_discord_update.load(Ordering::Relaxed);
        let last_discord_test = self.last_discord_test.load(Ordering::Relaxed);
        let failures = self.consecutive_failures.load(Ordering::Relaxed);
        let gateway_failures = self.gateway_failures.load(Ordering::Relaxed);
        let discord_test_failures = self.discord_test_failures.load(Ordering::Relaxed);

        // For newly started bots, use start_time as baseline (value of 0 means never updated)
        let _effective_start = if start_time > 0 { start_time } else { now };

        // Consider unhealthy if:
        // - No price update in last 5 minutes
        // - No database write in last 5 minutes
        // - No Discord update in last 3 minutes (more aggressive for gateway issues)
        // - No successful Discord connectivity test in last 10 minutes
        // - More than 3 consecutive failures
        // - More than 5 gateway failures (indicates broken Discord connection)
        // - More than 3 Discord test failures (indicates connection issues)
        // Treat 0 (never updated) as using start_time for staleness check
        let price_stale = last_price > 0 && now.saturating_sub(last_price) > 300;
        let db_stale = last_db > 0 && now.saturating_sub(last_db) > 300;
        let discord_stale = last_discord > 0 && now.saturating_sub(last_discord) > 180;
        let discord_test_stale =
            last_discord_test > 0 && now.saturating_sub(last_discord_test) > 600;
        let too_many_failures = failures > 3;
        let gateway_broken = gateway_failures > 5;
        let discord_test_broken = discord_test_failures > 3;

        !price_stale
            && !db_stale
            && !discord_stale
            && !discord_test_stale
            && !too_many_failures
            && !gateway_broken
            && !discord_test_broken
    }

    pub fn to_json(&self) -> serde_json::Value {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let last_price = self.last_price_update.load(Ordering::Relaxed);
        let last_db = self.last_db_write.load(Ordering::Relaxed);
        let last_discord = self.last_discord_update.load(Ordering::Relaxed);
        let last_discord_test = self.last_discord_test.load(Ordering::Relaxed);
        let failures = self.consecutive_failures.load(Ordering::Relaxed);
        let gateway_failures = self.gateway_failures.load(Ordering::Relaxed);
        let discord_test_failures = self.discord_test_failures.load(Ordering::Relaxed);
        let uptime = self.get_uptime_seconds();

        json!({
            "bot_name": self.bot_name,
            "healthy": self.is_healthy(),
            "uptime_seconds": uptime,
            "timestamp": now,
            "last_price_update": last_price,
            "last_db_write": last_db,
            "last_discord_update": last_discord,
            "last_discord_test": last_discord_test,
            "consecutive_failures": failures,
            "gateway_failures": gateway_failures,
            "discord_test_failures": discord_test_failures,
            "seconds_since_price_update": now.saturating_sub(last_price),
            "seconds_since_db_write": now.saturating_sub(last_db),
            "seconds_since_discord_update": now.saturating_sub(last_discord),
            "seconds_since_discord_test": now.saturating_sub(last_discord_test)
        })
    }
}

/// Aggregates health status from all bots in the container
/// Returns healthy if at least one bot is functioning
#[derive(Debug, Clone)]
pub struct HealthAggregator {
    bots: Arc<std::sync::Mutex<Vec<Arc<HealthState>>>>,
}

impl HealthAggregator {
    pub fn new() -> Self {
        Self {
            bots: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn add_bot(&self, health: Arc<HealthState>) {
        if let Ok(mut bots) = self.bots.lock() {
            bots.push(health);
        }
    }

    pub fn is_healthy(&self) -> bool {
        if let Ok(bots) = self.bots.lock() {
            if bots.is_empty() {
                return true;
            }
            return bots.iter().any(|b| b.is_healthy());
        }
        false
    }

    pub fn to_json(&self) -> serde_json::Value {
        let bots = match self.bots.lock() {
            Ok(bots) => bots,
            Err(_) => return json!({"error": "lock poisoned"}),
        };
        let bots_json: Vec<serde_json::Value> = bots.iter().map(|b| b.to_json()).collect();

        let any_healthy = bots.iter().any(|b| b.is_healthy());

        json!({
            "healthy": any_healthy,
            "total_bots": bots.len(),
            "healthy_bots": bots.iter().filter(|b| b.is_healthy()).count(),
            "bots": bots_json
        })
    }
}

impl Default for HealthAggregator {
    fn default() -> Self {
        Self::new()
    }
}
