mod errors;
mod config;
mod utils;
mod health;
mod health_server;
mod database;
mod discord_api;
mod bot;
mod price_service;
mod db_cleanup;
mod charting;
mod shanghai_price_service;
#[cfg(test)]
mod database_tests;

use errors::BotResult;
use config::BotConfig;
use database::PriceDatabase;
use db_cleanup::DatabaseCleanup;
use bot::start_bot;
use health::{HealthState, HealthAggregator};
use health_server::start_health_server;

use dotenv::dotenv;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, error, warn};

const RECONNECT_DELAY_SECONDS: u64 = 30;

#[tokio::main]
async fn main() -> BotResult<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info,discord_bot=debug,discord_bot::database=info")
        .init();
    
    info!("🚀 Starting RustyMcPriceface Unified Container...");
    dotenv().ok();
    
    // Initialize shared database
    info!("📦 Initializing shared database...");
    let db = match PriceDatabase::new(config::DATABASE_PATH) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            error!("Failed to initialize database: {}", e);
            return Err(e);
        }
    };

    // Start Database Cleanup Service
    info!("🧹 Starting Database Cleanup Service...");
    {
        tokio::spawn(async move {
            let cleanup = DatabaseCleanup::new();
            if let Err(e) = cleanup.run().await {
                error!("Cleanup service crashed: {}", e);
            }
        });
    }

    // Start Price Service
    info!("💹 Starting Price Fetching Service...");
    {
        let db_clone = db.clone();
        tokio::spawn(async move {
            if let Err(e) = price_service::run(db_clone).await {
                error!("Price service crashed: {}", e);
            }
        });
    }

    // Start Shanghai Silver Price Service
    info!("🏭 Starting Shanghai Silver Price Service...");
    {
        let db_clone = db.clone();
        tokio::spawn(async move {
            if let Err(e) = shanghai_price_service::run(db_clone).await {
                error!("Shanghai price service crashed: {}", e);
            }
        });
    }

    // Load all bot instances
    let instances = BotConfig::load_bot_instances();
    if instances.is_empty() {
        warn!("⚠️ No bot configurations found! Set DISCORD_TOKEN or DISCORD_TOKEN_[TICKER]");
    } else {
        info!("🤖 Found {} bot configurations", instances.len());
    }

    // Global configuration for update interval
    let global_config = BotConfig::from_env()?;

    // Create health aggregator for all bots
    let health_aggregator = Arc::new(HealthAggregator::new());

    // Spawn a task for each bot
    let mut handles = vec![];

    for (ticker, token) in instances {
        let db_clone = db.clone();
        let health_agg_clone = health_aggregator.clone();
        let mut bot_config = global_config.clone();
        bot_config.crypto_name = ticker.clone();
        bot_config.discord_token = token.clone();

        // Create health state for this bot and register with aggregator
        let health = Arc::new(HealthState::new(ticker.clone()));
        let health_clone = health.clone();
        
        // Add to aggregator
        health_agg_clone.add_bot(health);

        info!("🚀 Spawning bot for {}...", ticker);

        let handle = tokio::spawn(async move {
            loop {
                // Determine appropriate emoji for logs
                let emoji = utils::get_crypto_emoji(&ticker);
                info!("{} Starting {} bot...", emoji, ticker);

                match start_bot(bot_config.clone(), db_clone.clone(), health_clone.clone(), health_agg_clone.clone()).await {
                    Ok(_) => {
                        error!("{} {} bot exited unexpectedly", emoji, ticker);
                    },
                    Err(e) => {
                        error!("{} {} bot crashed: {}", emoji, ticker, e);
                    }
                }

                error!("{} Restarting {} bot in {} seconds...", emoji, ticker, RECONNECT_DELAY_SECONDS);
                sleep(Duration::from_secs(RECONNECT_DELAY_SECONDS)).await;
            }
        });
        handles.push(handle);
    }

    // Start health check server
    info!("🏥 Starting health check server...");
    let health_for_server = health_aggregator.clone();
    tokio::spawn(async move {
        start_health_server(health_for_server, 8080).await;
    });

    // Give health server time to start
    sleep(Duration::from_secs(1)).await;

    // Keep the main process alive
    if !handles.is_empty() {
        info!("✅ All bots spawned. Main process entering monitor loop.");
        // Wait for all handles (they shouldn't return unless panicked/cancelled)
        for handle in handles {
            let _ = handle.await;
        }
    } else {
        warn!("⚠️ No bots to run. Exiting.");
    }
    
    Ok(())
}