use std::time::Duration;
use crate::errors::{BotError, BotResult};

/// Configuration for the Discord bot
#[derive(Debug, Clone)]
pub struct BotConfig {
    /// Discord bot token
    pub discord_token: String,
    /// Cryptocurrency name to track
    pub crypto_name: String,
    /// Update interval in seconds
    pub update_interval: Duration,
    /// Price tracking duration in seconds
    pub tracking_duration: Duration,
    /// Pyth Network feed ID (optional)
    pub pyth_feed_id: Option<String>,
}

impl BotConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> BotResult<Self> {
        let discord_token = std::env::var("DISCORD_TOKEN")
            .map_err(|_| BotError::EnvVar("DISCORD_TOKEN not set".into()))?;

        let crypto_name = std::env::var("CRYPTO_NAME")
            .unwrap_or_else(|_| "SOL".to_string());

        let update_interval_secs = std::env::var("UPDATE_INTERVAL_SECONDS")
            .unwrap_or_else(|_| "300".to_string())
            .parse::<u64>()
            .map_err(|_| BotError::Parse("Invalid UPDATE_INTERVAL_SECONDS".into()))?;

        let pyth_feed_id = std::env::var("PYTH_FEED_ID").ok();

        Ok(Self {
            discord_token,
            crypto_name,
            update_interval: Duration::from_secs(update_interval_secs),
            tracking_duration: Duration::from_secs(3600), // 1 hour
            pyth_feed_id,
        })
    }

    /// Validate the configuration
    pub fn validate(&self) -> BotResult<()> {
        if self.discord_token.is_empty() {
            return Err(BotError::InvalidInput("Discord token cannot be empty".into()));
        }

        if self.crypto_name.is_empty() {
            return Err(BotError::InvalidInput("Crypto name cannot be empty".into()));
        }

        if self.update_interval.as_secs() == 0 {
            return Err(BotError::InvalidInput("Update interval must be greater than 0".into()));
        }

        Ok(())
    }
}

/// Constants for the application
pub const CLEANUP_INTERVAL_SECONDS: u64 = 86400; // 24 hours
pub const PRICE_HISTORY_DAYS: u64 = 365; // Keep 1 year of history

/// Data retention tiers for aggregation
pub const RAW_DATA_RETENTION_HOURS: u64 = 24; // Keep raw 15-second data for 24 hours
pub const MINUTE_DATA_RETENTION_DAYS: u64 = 7; // Keep 1-minute data for 7 days  
pub const FIVE_MINUTE_DATA_RETENTION_DAYS: u64 = 30; // Keep 5-minute data for 30 days
pub const FIFTEEN_MINUTE_DATA_RETENTION_DAYS: u64 = 365; // Keep 15-minute data for 1 year 