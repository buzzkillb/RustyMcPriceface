mod bot;
mod charting;
mod config;
mod database;
#[cfg(test)]
mod database_tests;
mod db_cleanup;
mod discord_api;
mod errors;
mod health;
mod health_server;
mod price_service;
mod price_state;
mod shanghai_price_service;
mod utils;

use bot::start_bot;
use config::BotConfig;
use database::PriceDatabase;
use db_cleanup::DatabaseCleanup;
use errors::BotResult;
use health::{HealthAggregator, HealthState};
use health_server::start_health_server_with_retry;
use price_state::SharedPrices;

use dotenv::dotenv;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

const RECONNECT_DELAY_SECONDS: u64 = 30;
const SERVICE_RESTART_DELAY_SECONDS: u64 = 5;

enum ServiceHandle {
    Bot(tokio::task::JoinHandle<()>, String),
    Service(tokio::task::JoinHandle<()>, String),
    HealthServer(tokio::task::JoinHandle<()>),
}

fn format_service_name(handle: &ServiceHandle) -> &str {
    match handle {
        ServiceHandle::Bot(_, name) => name.as_str(),
        ServiceHandle::Service(_, name) => name.as_str(),
        ServiceHandle::HealthServer(_) => "health_server",
    }
}

#[tokio::main]
async fn main() -> BotResult<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info,rustymcpriceface=debug")
        .init();

    info!("🚀 Starting RustyMcPriceface Unified Container...");
    dotenv().ok();

    // Initialize shared database
    info!("📦 Initializing shared database...");
    let db = match PriceDatabase::new(&config::DATABASE_URL).await {
        Ok(db) => Arc::new(db),
        Err(e) => {
            error!("Failed to initialize database: {}", e);
            return Err(e);
        }
    };

    // Create shared price state for all services and bots
    let shared_prices = Arc::new(SharedPrices::new());

    // Create health aggregator for all bots
    let health_aggregator = Arc::new(HealthAggregator::new());

    // Storage for all service handles
    let mut service_handles: Vec<ServiceHandle> = Vec::new();

    // Spawn Database Cleanup Service with supervision
    info!("🧹 Starting Database Cleanup Service...");
    let db_cleanup_db = db.clone();
    let cleanup_handle = tokio::spawn(async move {
        let cleanup = DatabaseCleanup::new(&db_cleanup_db);
        loop {
            info!("🧹 Cleanup service starting...");
            match cleanup.run().await {
                Ok(_) => {
                    error!(
                        "🧹 Cleanup service exited unexpectedly - restarting in {}s",
                        SERVICE_RESTART_DELAY_SECONDS
                    );
                }
                Err(e) => {
                    error!(
                        "🧹 Cleanup service crashed: {} - restarting in {}s",
                        e, SERVICE_RESTART_DELAY_SECONDS
                    );
                }
            }
            sleep(Duration::from_secs(SERVICE_RESTART_DELAY_SECONDS)).await;
        }
    });
    service_handles.push(ServiceHandle::Service(
        cleanup_handle,
        "cleanup".to_string(),
    ));

    // Spawn Price Service with supervision
    info!("💹 Starting Price Fetching Service...");
    let price_service_db = db.clone();
    let price_service_prices = shared_prices.clone();
    let price_handle = tokio::spawn(async move {
        loop {
            info!("💹 Price service starting...");
            match price_service::run(price_service_db.clone(), price_service_prices.clone()).await {
                Ok(_) => {
                    error!(
                        "💹 Price service exited unexpectedly - restarting in {}s",
                        SERVICE_RESTART_DELAY_SECONDS
                    );
                }
                Err(e) => {
                    error!(
                        "💹 Price service crashed: {} - restarting in {}s",
                        e, SERVICE_RESTART_DELAY_SECONDS
                    );
                }
            }
            sleep(Duration::from_secs(SERVICE_RESTART_DELAY_SECONDS)).await;
        }
    });
    service_handles.push(ServiceHandle::Service(
        price_handle,
        "price_service".to_string(),
    ));

    // Spawn Shanghai Silver Price Service with supervision
    info!("🏭 Starting Shanghai Silver Price Service...");
    let shanghai_db = db.clone();
    let shanghai_prices = shared_prices.clone();
    let shanghai_handle = tokio::spawn(async move {
        loop {
            info!("🏭 Shanghai price service starting...");
            match shanghai_price_service::run(shanghai_db.clone(), shanghai_prices.clone()).await {
                Ok(_) => {
                    error!(
                        "🏭 Shanghai price service exited unexpectedly - restarting in {}s",
                        SERVICE_RESTART_DELAY_SECONDS
                    );
                }
                Err(e) => {
                    error!(
                        "🏭 Shanghai price service crashed: {} - restarting in {}s",
                        e, SERVICE_RESTART_DELAY_SECONDS
                    );
                }
            }
            sleep(Duration::from_secs(SERVICE_RESTART_DELAY_SECONDS)).await;
        }
    });
    service_handles.push(ServiceHandle::Service(
        shanghai_handle,
        "shanghai_price_service".to_string(),
    ));

    // Load all bot instances
    let instances = BotConfig::load_bot_instances();
    if instances.is_empty() {
        warn!("⚠️ No bot configurations found! Set DISCORD_TOKEN or DISCORD_TOKEN_[TICKER]");
    } else {
        info!("🤖 Found {} bot configurations", instances.len());
    }

    // Global configuration for update interval
    let global_config = BotConfig::from_env()?;

    // Spawn a task for each bot
    for (ticker, token) in instances {
        let db_clone = db.clone();
        let health_agg_clone = health_aggregator.clone();
        let bot_prices = shared_prices.clone();
        let mut bot_config = global_config.clone();
        bot_config.crypto_name = ticker.clone();
        bot_config.discord_token = token.clone();

        // Create health state for this bot and register with aggregator
        let health = Arc::new(HealthState::new(ticker.clone()));
        let health_clone = health.clone();

        // Add to aggregator
        health_agg_clone.add_bot(health).await;

        info!("🚀 Spawning bot for {}...", ticker);

        let ticker_for_handle = ticker.clone();
        let bot_handle = tokio::spawn(async move {
            loop {
                let emoji = utils::get_crypto_emoji(&ticker_for_handle);
                info!("{} Starting {} bot...", emoji, ticker_for_handle);

                match start_bot(
                    bot_config.clone(),
                    db_clone.clone(),
                    health_clone.clone(),
                    health_agg_clone.clone(),
                    bot_prices.clone(),
                )
                .await
                {
                    Ok(_) => {
                        error!("{} {} bot exited unexpectedly", emoji, ticker_for_handle);
                    }
                    Err(e) => {
                        error!("{} {} bot crashed: {}", emoji, ticker_for_handle, e);
                    }
                }

                error!(
                    "{} Restarting {} bot in {} seconds...",
                    emoji, ticker_for_handle, RECONNECT_DELAY_SECONDS
                );
                sleep(Duration::from_secs(RECONNECT_DELAY_SECONDS)).await;
            }
        });
        service_handles.push(ServiceHandle::Bot(bot_handle, ticker));
    }

    // Start health check server (non-fatal - retries but doesn't crash container)
    info!("🏥 Starting health check server...");
    let health_for_server = health_aggregator.clone();
    let health_handle = tokio::spawn(async move {
        match start_health_server_with_retry(health_for_server, 8080, 10).await {
            Ok(_) => {
                error!("🏥 Health server exited unexpectedly");
            }
            Err(e) => {
                error!("🏥 Health server failed after retries: {}", e);
            }
        }
        // Don't restart health server - if it can't bind, something is wrong
        // The bots should continue running regardless
        loop {
            sleep(Duration::from_secs(60)).await;
        }
    });
    service_handles.push(ServiceHandle::HealthServer(health_handle));

    // Give health server time to start
    sleep(Duration::from_secs(1)).await;

    // Monitor all service handles
    if !service_handles.is_empty() {
        info!(
            "✅ All services spawned. Monitoring {} services...",
            service_handles.len()
        );

        loop {
            // Check all handles
            let mut all_dead = true;
            let mut dead_services = Vec::new();

            for handle in &service_handles {
                let is_dead = match handle {
                    ServiceHandle::Bot(h, name) => h.is_finished(),
                    ServiceHandle::Service(h, name) => h.is_finished(),
                    ServiceHandle::HealthServer(h) => h.is_finished(),
                };

                if !is_dead {
                    all_dead = false;
                } else {
                    dead_services.push(handle);
                }
            }

            // If any service died, log fatal error (they should restart themselves)
            if !dead_services.is_empty() {
                for handle in &dead_services {
                    let name = format_service_name(handle);
                    error!(
                        "💀 CRITICAL: {} died unexpectedly - it should auto-restart",
                        name
                    );
                }
            }

            // Sleep before next check
            sleep(Duration::from_secs(5)).await;
        }
    } else {
        warn!("⚠️ No services to run. Exiting.");
    }

    Ok(())
}
