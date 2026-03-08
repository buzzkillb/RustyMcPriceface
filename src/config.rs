use crate::errors::{BotError, BotResult};
use std::time::Duration;

/// Configuration for the Discord bot
#[derive(Debug, Clone)]
pub struct BotConfig {
    /// Discord bot token
    pub discord_token: String,
    /// Cryptocurrency name to track
    pub crypto_name: String,
    /// Update interval in seconds
    pub update_interval: Duration,
    /// Pyth Network feed ID (optional)
    pub pyth_feed_id: Option<String>,
}

impl BotConfig {
    /// Load global configuration from environment variables
    pub fn from_env() -> BotResult<Self> {
        // These are global defaults, typically unused in multi-bot mode except for defaults
        let discord_token = std::env::var("DISCORD_TOKEN").unwrap_or_default();
        let crypto_name = std::env::var("CRYPTO_NAME").unwrap_or_else(|_| "SOL".to_string());

        let update_interval_secs = std::env::var("UPDATE_INTERVAL_SECONDS")
            .unwrap_or_else(|_| "300".to_string())
            .parse::<u64>()
            .map_err(|_| BotError::Parse("Invalid UPDATE_INTERVAL_SECONDS".into()))?;

        let pyth_feed_id = std::env::var("PYTH_FEED_ID").ok();

        Ok(Self {
            discord_token,
            crypto_name,
            update_interval: Duration::from_secs(update_interval_secs),
            pyth_feed_id,
        })
    }

    /// Load all bot instances defined in environment variables (DISCORD_TOKEN_BTC, etc.)
    pub fn load_bot_instances() -> Vec<(String, String)> {
        let mut instances = Vec::new();

        // Scan environment variables
        for (key, value) in std::env::vars() {
            if key.starts_with("DISCORD_TOKEN_") {
                let ticker = key.trim_start_matches("DISCORD_TOKEN_").to_string();
                if !ticker.is_empty() && !value.is_empty() {
                    instances.push((ticker, value));
                }
            }
        }

        // Sort for consistent startup order
        instances.sort_by(|a, b| a.0.cmp(&b.0));

        // If no specific tokens found, fallback to single instance config if present
        if instances.is_empty() {
            if let Ok(token) = std::env::var("DISCORD_TOKEN") {
                let name = std::env::var("CRYPTO_NAME").unwrap_or_else(|_| "SOL".to_string());
                if !token.is_empty() {
                    instances.push((name, token));
                }
            }
        }

        instances
    }
}

/// Constants for the application
pub const PRICE_HISTORY_DAYS: u64 = 365; // Keep 1 year of history

/// Data retention tiers for aggregation
pub const RAW_DATA_RETENTION_HOURS: u64 = 24; // Keep raw 15-second data for 24 hours
pub const MINUTE_DATA_RETENTION_DAYS: u64 = 7; // Keep 1-minute data for 7 days
pub const FIVE_MINUTE_DATA_RETENTION_DAYS: u64 = 30; // Keep 5-minute data for 30 days
pub const FIFTEEN_MINUTE_DATA_RETENTION_DAYS: u64 = 365; // Keep 15-minute data for 1 year
