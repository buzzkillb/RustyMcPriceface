mod errors;
mod config;
mod utils;
mod health;
mod health_server;
mod database;
mod discord_api;
mod bot;
mod price_service;

use errors::BotResult;
use config::BotConfig;
use bot::start_bot_with_reconnection;

use dotenv::dotenv;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, error};

const RECONNECT_DELAY_SECONDS: u64 = 30;

#[tokio::main]
async fn main() -> BotResult<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info,discord_bot=debug")
        .init();
    
    info!("Starting bot with slash command support...");
    dotenv().ok();
    
    // Load and validate configuration
    let config = BotConfig::from_env()?;
    config.validate()?;
    
    info!("Starting Multi-Crypto Price Discord Bot...");
    info!("Update interval: {:?}", config.update_interval);
    info!("Price tracking: {:?}", config.tracking_duration);
    info!("Crypto: {}", config.crypto_name);
    info!("Reading from: shared/prices.json");
    
    // Create a new instance of the bot
    info!("Creating bot with event handler...");
    
    // Start the bot with reconnection logic
    loop {
        match start_bot_with_reconnection(&config).await {
            Ok(_) => {
                info!("Bot exited normally");
                break;
            }
            Err(e) => {
                error!("Bot crashed: {}", e);
                error!("Attempting to reconnect in {} seconds...", RECONNECT_DELAY_SECONDS);
                sleep(Duration::from_secs(RECONNECT_DELAY_SECONDS)).await;
            }
        }
    }
    
    Ok(())
}