mod errors;
mod config;
mod utils;

use errors::{BotError, BotResult};
use config::{BotConfig, CLEANUP_INTERVAL_SECONDS, PRICE_HISTORY_DAYS};
use utils::{
    validate_crypto_name, get_current_timestamp, format_price, get_crypto_emoji,
    validate_price, calculate_percentage_change, get_change_arrow
};

use serenity::{
    async_trait,
    model::gateway::Ready,
    prelude::*,
    http::Http,
    all::{Command, CommandDataOptionValue, CommandOptionType, CreateCommand, CreateCommandOption},
    model::application::CommandInteraction,
    builder::{CreateInteractionResponse, CreateInteractionResponseMessage},
};
use std::time::Duration;
use tokio::time::sleep;
use std::sync::Arc;
use dotenv::dotenv;
use std::time::{SystemTime, UNIX_EPOCH};
use serenity::all::ActivityData;
use std::fs;
use reqwest;
use serde_json::Value;
use std::collections::HashMap;
use rusqlite::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn, error, debug};

const TRACKING_DURATION_SECONDS: u64 = 3600; // 1 hour

#[derive(serde::Deserialize)]
struct PriceData {
    price: f64,
    #[allow(dead_code)]
    timestamp: u64,
}

#[derive(serde::Deserialize)]
struct PricesFile {
    prices: HashMap<String, PriceData>,
    timestamp: u64,
}

/// Discord bot for tracking cryptocurrency prices
#[derive(Debug)]
pub struct Bot {
    config: BotConfig,
}

impl Bot {
    /// Create a new bot instance with configuration
    pub fn new(config: BotConfig) -> Self {
        Self { config }
    }

    /// Register slash commands with Discord
    async fn register_commands(&self, http: &Http) -> BotResult<()> {
        info!("Registering slash commands...");
        
        let current_crypto = &self.config.crypto_name;
        let price_command = CreateCommand::new("price")
            .description(format!("Get current price for a cryptocurrency (defaults to {})", current_crypto))
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "crypto", format!("Cryptocurrency symbol (defaults to {})", current_crypto))
                    .required(false)
            );

        info!("Creating global command...");
        
        Command::create_global_command(http, price_command).await
            .map_err(|e| BotError::Discord(format!("Failed to register /price command: {}", e)))?;
        
        info!("Successfully registered /price command globally");
        info!("Note: Global commands can take up to 1 hour to appear in Discord");
        
        Ok(())
    }

    /// Handle the /price slash command
    async fn handle_price_command(&self, interaction: &CommandInteraction) -> BotResult<String> {
        // Get crypto name from command option, or default to current bot's crypto
        let crypto_name = if let Some(crypto_option) = interaction.data.options.iter().find(|opt| opt.name == "crypto") {
            match &crypto_option.value {
                CommandDataOptionValue::String(s) => {
                    let name = s.clone();
                    validate_crypto_name(&name)?;
                    name
                },
                _ => return Err(BotError::InvalidInput("Invalid crypto option".into())),
            }
        } else {
            // No crypto specified, use the current bot's crypto
            self.config.crypto_name.clone()
        };

        debug!("Price command called for: {}", crypto_name);

        // Get current price from shared prices file
        let prices = read_prices_from_file().await?;
        debug!("Available cryptos: {:?}", prices.prices.keys().collect::<Vec<_>>());
        
        let price_data = prices.prices.get(&crypto_name)
            .ok_or_else(|| BotError::PriceNotFound(crypto_name.clone()))?;
        
        validate_price(price_data.price)?;
        
        let emoji = get_crypto_emoji(&crypto_name);
        let formatted_price = format_price(price_data.price);
        
        info!("{} price: ${}", crypto_name, price_data.price);
        
        // Calculate price changes over different time periods using database
        let change_info = get_price_changes(&crypto_name, price_data.price)
            .unwrap_or_else(|_| " 🔄 Building history".to_string());
                    
        // Build the main response
        let mut response = format!("{} {}: {} {}", emoji, crypto_name, formatted_price, change_info);
        
        // Add prices in terms of BTC, ETH, and SOL (excluding the crypto's own price)
        let mut conversion_prices = Vec::new();
        
        if crypto_name != "BTC" {
            if let Some(btc_price) = prices.prices.get("BTC") {
                let btc_conversion = price_data.price / btc_price.price;
                conversion_prices.push(format!("{:.8} BTC", btc_conversion));
                debug!("BTC conversion: {:.8} BTC", btc_conversion);
            } else {
                warn!("BTC price not found in shared data");
            }
        }
        
        if crypto_name != "ETH" {
            if let Some(eth_price) = prices.prices.get("ETH") {
                let eth_conversion = price_data.price / eth_price.price;
                conversion_prices.push(format!("{:.6} ETH", eth_conversion));
                debug!("ETH conversion: {:.6} ETH", eth_conversion);
            } else {
                warn!("ETH price not found in shared data");
            }
        }
        
        if crypto_name != "SOL" {
            if let Some(sol_price) = prices.prices.get("SOL") {
                let sol_conversion = price_data.price / sol_price.price;
                conversion_prices.push(format!("{:.4} SOL", sol_conversion));
                debug!("SOL conversion: {:.4} SOL", sol_conversion);
            } else {
                warn!("SOL price not found in shared data");
            }
        }
        
        // Add conversion prices to response if available
        if !conversion_prices.is_empty() {
            response.push_str(&format!("\n💱 Also: {}", conversion_prices.join(" | ")));
            debug!("Final response with conversions: {}", response);
        } else {
            warn!("No conversion prices available");
        }
        
        Ok(response)
    }
}

// These functions are now handled by the BotConfig struct

/// Fetch individual cryptocurrency price from Pyth Network
async fn get_individual_crypto_price(feed_id: &str) -> BotResult<f64> {
    let url = format!("https://hermes.pyth.network/v2/updates/price/latest?ids%5B%5D={}", feed_id);
    
    let client = reqwest::Client::new();
    let response = client.get(&url)
        .header("User-Agent", "Crypto-Price-Bot/1.0")
        .send()
        .await.map_err(|e| BotError::Http(e.to_string()))?;
    
    if !response.status().is_success() {
        return Err(BotError::Http(format!("HTTP request failed: {}", response.status()).into()));
    }
    
    let json: Value = response.json().await.map_err(|e| BotError::Http(e.to_string()))?;
    
    // Parse the price from the parsed array
    let parsed_data = json.get("parsed")
        .and_then(|p| p.as_array())
        .ok_or_else(|| BotError::Parse("No parsed data found".into()))?;
    
    let first_feed = parsed_data.first()
        .ok_or_else(|| BotError::Parse("No feed data found".into()))?;
    
    let price_data = first_feed.get("price")
        .ok_or_else(|| BotError::Parse("No price data found".into()))?;
    
    let price_str = price_data.get("price")
        .and_then(|p| p.as_str())
        .ok_or_else(|| BotError::Parse("No price string found".into()))?;
    
    let price = price_str.parse::<i64>()
        .map_err(|_| BotError::Parse("Invalid price format".into()))?;
    
    let expo = price_data.get("expo").and_then(|e| e.as_i64()).unwrap_or(0);
    let real_price = price as f64 * 10f64.powi(expo as i32);
    
    validate_price(real_price)?;
    Ok(real_price)
}

fn format_about_me(current_price: f64, shared_prices: &PricesFile, config: &BotConfig) -> String {
    let crypto_name = config.crypto_name.clone();
    
    // Get reference prices for cross-rates
    let btc_price = shared_prices.prices.get("BTC").map(|p| p.price).unwrap_or(45000.0);
    let eth_price = shared_prices.prices.get("ETH").map(|p| p.price).unwrap_or(2800.0);
    let sol_price = shared_prices.prices.get("SOL").map(|p| p.price).unwrap_or(95.0);
    
    // Calculate cross-rates
    let btc_rate = current_price / btc_price;
    let eth_rate = current_price / eth_price;
    let sol_rate = current_price / sol_price;
    
    // Format based on price magnitude using the new formatting rules
    let price_format = format_price(current_price);
    
    // Format cross-rates
    let btc_format = if btc_rate >= 0.001 {
        format!("{:.4}", btc_rate)
    } else {
        format!("{:.6}", btc_rate)
    };
    
    let eth_format = if eth_rate >= 0.001 {
        format!("{:.4}", eth_rate)
    } else {
        format!("{:.6}", eth_rate)
    };
    
    let sol_format = if sol_rate >= 0.001 {
        format!("{:.4}", sol_rate)
    } else {
        format!("{:.6}", sol_rate)
    };
    
    format!("{} {}\n💰 BTC: ${} ({})\n🪙 ETH: ${} ({})\n📊 SOL: ${} ({})", 
        get_crypto_emoji(&crypto_name), crypto_name, price_format, btc_format,
        eth_price, eth_format,
        sol_price, sol_format)
}

// These functions are now in utils.rs

// Database functions for slash commands
/// Get a database connection
fn get_db_connection() -> BotResult<Connection> {
    Connection::open("shared/prices.db")
        .map_err(BotError::Database)
}

fn get_latest_prices() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let conn = get_db_connection()?;
    
    let mut stmt = conn.prepare(
        "SELECT p1.crypto_name, p1.price, p1.timestamp 
         FROM prices p1 
         INNER JOIN (
             SELECT crypto_name, MAX(timestamp) as max_timestamp 
             FROM prices GROUP BY crypto_name
         ) p2 ON p1.crypto_name = p2.crypto_name AND p1.timestamp = p2.max_timestamp
         ORDER BY p1.crypto_name"
    )?;
    
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, i64>(2)?))
    })?;
    
    let mut result = String::from("📈 **Latest Prices**\n");
    for row in rows {
        let (crypto, price, timestamp) = row?;
        let date = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|dt| dt.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        
        let emoji = get_crypto_emoji(&crypto);
        result.push_str(&format!("{} **{}**: ${:.6} ({} UTC)\n", emoji, crypto, price, date));
    }
    
    Ok(result)
}

fn get_price_history(crypto: &str, limit: i64) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let conn = get_db_connection()?;
    
    let mut stmt = conn.prepare(
        "SELECT price, timestamp FROM prices 
         WHERE crypto_name = ? ORDER BY timestamp DESC LIMIT ?"
    )?;
    
    let rows = stmt.query_map([crypto, &limit.to_string()], |row| {
        Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    let mut result = format!("📊 **{} Price History** (Last {} records)\n", crypto, limit);
    let emoji = get_crypto_emoji(crypto);
    
    for row in rows {
        let (price, timestamp) = row?;
        let date = chrono::DateTime::from_timestamp(timestamp, 0)
            .map(|dt| dt.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        
        result.push_str(&format!("{} ${:.6} at {} UTC\n", emoji, price, date));
    }
    
    Ok(result)
}

/// Get price changes for different time periods
fn get_price_changes(crypto: &str, current_price: f64) -> BotResult<String> {
    validate_crypto_name(crypto)?;
    validate_price(current_price)?;
    
    let conn = get_db_connection()?;
    let current_time = get_current_timestamp()?;
    
    let mut changes = Vec::new();
    
    // Define time periods and their labels
    let periods = vec![
        (3600, "1h"),
        (43200, "12h"), 
        (86400, "24h"),
        (604800, "7d"),
    ];
    
    for (seconds, label) in periods {
        let time_ago = current_time - seconds;
        
        let mut stmt = conn.prepare(
            "SELECT price FROM prices WHERE crypto_name = ? AND timestamp >= ? ORDER BY timestamp ASC LIMIT 1"
        )?;
        
        let rows = stmt.query_map([crypto, &time_ago.to_string()], |row| {
            Ok(row.get(0)?)
        })?;
        
        let mut prices = rows.collect::<Result<Vec<f64>, _>>()?;
        
        if let Some(old_price) = prices.pop() {
            let change_percent = calculate_percentage_change(current_price, old_price)?;
            let arrow = get_change_arrow(change_percent);
            let sign = if change_percent >= 0.0 { "+" } else { "" };
            changes.push(format!("{} {}{:.2}% ({})", arrow, sign, change_percent, label));
        }
    }
    
    if changes.is_empty() {
        Ok("🔄 Building history".to_string())
    } else {
        Ok(format!(" {}", changes.join(" | ")))
    }
}

fn get_database_stats() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let conn = get_db_connection()?;
    
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM prices", [], |row| row.get(0))?;
    
    let mut stmt = conn.prepare("SELECT crypto_name, COUNT(*) FROM prices GROUP BY crypto_name")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    let mut result = String::from("📊 **Database Statistics**\n");
    result.push_str(&format!("Total records: {}\n\n", total));
    result.push_str("Records per crypto:\n");
    
    for row in rows {
        let (crypto, count) = row?;
        let emoji = get_crypto_emoji(&crypto);
        result.push_str(&format!("{} {}: {} records\n", emoji, crypto, count));
    }
    
    Ok(result)
}

/// Clean up old price records from the database
fn cleanup_old_prices() -> BotResult<()> {
    let conn = get_db_connection()?;
    
    // Keep only the last 7 days of data
    let cutoff_time = get_current_timestamp()? - (PRICE_HISTORY_DAYS * 24 * 3600);
    
    let deleted = conn.execute(
        "DELETE FROM prices WHERE timestamp < ?",
        [&cutoff_time.to_string()]
    )?;
    
    if deleted > 0 {
        info!("Cleaned up {} old price records from database", deleted);
    }
    
    Ok(())
}

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("Bot is ready! Logged in as: {}", ready.user.name);
        info!("Starting command registration...");
        
        // Register slash commands
        if let Err(e) = self.register_commands(&ctx.http).await {
            error!("Command registration failed: {}", e);
            return;
        }
        
        info!("Command registration completed successfully");
        info!("Starting price update loop...");
        
        let http = ctx.http.clone();
        let ctx_arc = Arc::new(ctx);
        let config = self.config.clone();
        
        tokio::spawn(async move {
            price_update_loop(http, ctx_arc, config).await;
        });
        
        info!("Bot initialization complete!");
    }

    async fn interaction_create(&self, ctx: Context, interaction: serenity::model::application::Interaction) {
        debug!("Interaction received: {:?}", interaction.kind());
        
        if let serenity::model::application::Interaction::Command(command_interaction) = interaction {
            debug!("Command interaction: {}", command_interaction.data.name);
            
            let response = match command_interaction.data.name.as_str() {
                "price" => {
                    debug!("Handling /price command");
                    match self.handle_price_command(&command_interaction).await {
                        Ok(message) => {
                            debug!("Price command successful, responding with: {}", message);
                            let data = CreateInteractionResponseMessage::new().content(message);
                            let builder = CreateInteractionResponse::Message(data);
                            command_interaction.create_response(&ctx.http, builder).await
                        },
                        Err(e) => {
                            error!("Price command failed: {}", e);
                            let data = CreateInteractionResponseMessage::new().content(format!("❌ Error: {}", e));
                            let builder = CreateInteractionResponse::Message(data);
                            command_interaction.create_response(&ctx.http, builder).await
                        },
                    }
                }
                _ => {
                    warn!("Unknown command: {}", command_interaction.data.name);
                    let data = CreateInteractionResponseMessage::new().content("❌ Unknown command");
                    let builder = CreateInteractionResponse::Message(data);
                    command_interaction.create_response(&ctx.http, builder).await
                }
            };

            if let Err(e) = response {
                error!("Failed to respond to interaction: {}", e);
            }
        }
    }
}

/// Read prices from the shared JSON file
async fn read_prices_from_file() -> BotResult<PricesFile> {
    let file_path = "shared/prices.json";
    
    // Check if file exists
    if !std::path::Path::new(file_path).exists() {
        return Err(BotError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Prices file not found. Make sure price-service is running."
        )));
    }
    
    let content = fs::read_to_string(file_path)?;
    let prices: PricesFile = serde_json::from_str(&content)?;
    Ok(prices)
}

/// Get current cryptocurrency price
async fn get_crypto_price(config: &BotConfig) -> BotResult<f64> {
    // First try to get from shared prices file
    match read_prices_from_file().await {
        Ok(prices) => {
            if let Some(price_data) = prices.prices.get(&config.crypto_name) {
                validate_price(price_data.price)?;
                return Ok(price_data.price);
            }
        }
        Err(_) => {
            // If shared file doesn't exist or doesn't have our crypto, try direct API call
        }
    }
    
    // Fallback to direct API call if we have a feed ID
    if let Some(feed_id) = &config.pyth_feed_id {
        return get_individual_crypto_price(feed_id).await;
    }
    
    Err(BotError::PriceNotFound(config.crypto_name.clone()))
}

/// Get price indicator from database for status display
fn get_price_indicator_from_db(crypto_name: &str, current_price: f64) -> (String, f64) {
    let current_time = match get_current_timestamp() {
        Ok(time) => time,
        Err(_) => return ("🔄".to_string(), 0.0),
    };
    
    // Get the oldest price from the last hour
    let conn = match get_db_connection() {
        Ok(conn) => conn,
        Err(_) => return ("🔄".to_string(), 0.0),
    };
    
    let mut stmt = match conn.prepare(
        "SELECT price FROM prices WHERE crypto_name = ? AND timestamp >= ? ORDER BY timestamp ASC LIMIT 1"
    ) {
        Ok(stmt) => stmt,
        Err(_) => return ("🔄".to_string(), 0.0),
    };
    
    let one_hour_ago = current_time - TRACKING_DURATION_SECONDS;
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

/// Main price update loop
async fn price_update_loop(http: Arc<Http>, ctx: Arc<Context>, config: BotConfig) {
    let crypto_name = &config.crypto_name;
    
    loop {
        match get_crypto_price(&config).await {
            Ok(current_price) => {
                // Get price change over last hour from database
                let (arrow, change_percent) = get_price_indicator_from_db(crypto_name, current_price);
                
                // Format the nickname (just the price)
                let nickname = format!("{} {}", crypto_name, format_price(current_price));
                
                // Format the custom status with rotation
                let update_count = match get_current_timestamp() {
                    Ok(time) => (time / 12) % 4,
                    Err(_) => 0,
                };
                
                let custom_status = if let Ok(shared_prices) = read_prices_from_file().await {
                    // Calculate ticker price in terms of BTC, ETH, SOL
                    let btc_amount = current_price / shared_prices.prices.get("BTC").map(|p| p.price).unwrap_or(45000.0);
                    let eth_amount = current_price / shared_prices.prices.get("ETH").map(|p| p.price).unwrap_or(2800.0);
                    let sol_amount = current_price / shared_prices.prices.get("SOL").map(|p| p.price).unwrap_or(95.0);
                    
                    match crypto_name.as_str() {
                        "BTC" => {
                            // For BTC bot, show ETH and SOL amounts, skip BTC/BTC
                            match update_count {
                                0 => {
                                    if change_percent == 0.0 && arrow == "🔄" {
                                        format!("{} Building history", arrow)
                                    } else {
                                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                                    }
                                },
                                1 => format!("{:.8} Ξ", eth_amount),
                                2 => format!("{:.8} ◎", sol_amount),
                                3 => format!("{:.8} Ξ", eth_amount), // Repeat ETH since we skip BTC
                                _ => unreachable!(),
                            }
                        },
                        "ETH" => {
                            // For ETH bot, show BTC and SOL amounts, skip ETH/ETH
                            match update_count {
                                0 => {
                                    if change_percent == 0.0 && arrow == "🔄" {
                                        format!("{} Building history", arrow)
                                    } else {
                                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                                    }
                                },
                                1 => format!("{:.8} ₿", btc_amount),
                                2 => format!("{:.8} ◎", sol_amount),
                                3 => format!("{:.8} ₿", btc_amount), // Repeat BTC since we skip ETH
                                _ => unreachable!(),
                            }
                        },
                        "SOL" => {
                            // For SOL bot, show BTC and ETH amounts, skip SOL/SOL
                            match update_count {
                                0 => {
                                    if change_percent == 0.0 && arrow == "🔄" {
                                        format!("{} Building history", arrow)
                                    } else {
                                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                                    }
                                },
                                1 => format!("{:.8} ₿", btc_amount),
                                2 => format!("{:.8} Ξ", eth_amount),
                                3 => format!("{:.8} ₿", btc_amount), // Repeat BTC since we skip SOL
                                _ => unreachable!(),
                            }
                        },
                        _ => {
                            // For other tickers, show all three conversions
                            match update_count {
                                0 => {
                                    if change_percent == 0.0 && arrow == "🔄" {
                                        format!("{} Building history", arrow)
                                    } else {
                                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                                    }
                                },
                                1 => format!("{:.8} ₿", btc_amount),
                                2 => format!("{:.8} Ξ", eth_amount),
                                3 => format!("{:.8} ◎", sol_amount),
                                _ => unreachable!(),
                            }
                        }
                    }
                } else {
                    // Fallback if shared prices not available
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                };
                
                debug!("Updating nickname to: {}", nickname);
                debug!("Updating custom status to: {}", custom_status);
                
                // Update custom status (activity)
                ctx.set_activity(Some(ActivityData::playing(custom_status)));
                
                // Update "About Me" with cross-rates
                if let Ok(shared_prices) = read_prices_from_file().await {
                    let about_me = format_about_me(current_price, &shared_prices, &config);
                    // Note: Discord bot profiles can't be updated via API
                    // This would require OAuth2 user token, not bot token
                    debug!("About Me would be: {}", about_me);
                }
                
                // Save current price to database for history
                if let Ok(conn) = get_db_connection() {
                    if let Ok(current_time) = get_current_timestamp() {
                        if let Ok(mut stmt) = conn.prepare(
                            "INSERT INTO prices (crypto_name, price, timestamp) VALUES (?, ?, ?)"
                        ) {
                            if let Err(e) = stmt.execute([crypto_name, &current_price.to_string(), &current_time.to_string()]) {
                                error!("Failed to save price to database: {}", e);
                            } else {
                                debug!("Saved {} price to database: ${}", crypto_name, current_price);
                            }
                        }
                    }
                }
                
                // Iterate over all guilds and update nickname
                let guilds = ctx.cache.guilds();
                for guild_id in guilds {
                    match http.edit_nickname(guild_id, Some(&nickname), None).await {
                        Ok(_) => debug!("Updated nickname in guild {}", guild_id),
                        Err(e) => warn!("Failed to update nickname in guild {}: {}", guild_id, e),
                    }
                }
            }
            Err(e) => {
                error!("Failed to get {} price: {}", crypto_name, e);
            }
        }
        
                                // Periodic cleanup of old prices (every 24 hours)
        static LAST_CLEANUP: AtomicU64 = AtomicU64::new(0);
        if let Ok(current_time) = get_current_timestamp() {
            let last_cleanup = LAST_CLEANUP.load(Ordering::Relaxed);
            if current_time - last_cleanup > CLEANUP_INTERVAL_SECONDS {
                if let Err(e) = cleanup_old_prices() {
                    error!("Failed to cleanup old prices: {}", e);
                }
                LAST_CLEANUP.store(current_time, Ordering::Relaxed);
            }
        }
        
        // Wait for the next update using the configurable interval
        sleep(config.update_interval).await;
    }
}

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
    let intents = GatewayIntents::GUILD_MESSAGES | GatewayIntents::GUILDS;
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(Bot::new(config))
        .await
        .map_err(|e| BotError::Discord(format!("Error creating client: {:?}", e)))?;
    
    info!("Starting bot client...");
    // Start the bot
    if let Err(why) = client.start().await {
        error!("Client error: {:?}", why);
        return Err(BotError::Discord(format!("Client error: {:?}", why)));
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::*;
    use crate::config::BotConfig;
    use std::time::Duration;

    #[test]
    fn test_format_price() {
        assert_eq!(format_price(1234.5678), "$1235");
        assert_eq!(format_price(99.99), "$99.99");
        assert_eq!(format_price(1.234), "$1.234");
        assert_eq!(format_price(0.1234), "$0.1234");
    }

    #[test]
    fn test_get_crypto_emoji() {
        assert_eq!(get_crypto_emoji("BTC"), "🪙");
        assert_eq!(get_crypto_emoji("ETH"), "🪙");
        assert_eq!(get_crypto_emoji("SOL"), "📊");
        assert_eq!(get_crypto_emoji("WIF"), "🐕");
        assert_eq!(get_crypto_emoji("UNKNOWN"), "🪙");
    }

    #[test]
    fn test_validate_crypto_name() {
        assert!(validate_crypto_name("BTC").is_ok());
        assert!(validate_crypto_name("ETH").is_ok());
        assert!(validate_crypto_name("SOL").is_ok());
        assert!(validate_crypto_name("WIF").is_ok());
        
        assert!(validate_crypto_name("").is_err());
        assert!(validate_crypto_name("VERYLONGNAME").is_err());
        assert!(validate_crypto_name("BTC-USD").is_err());
    }

    #[test]
    fn test_validate_price() {
        assert!(validate_price(100.0).is_ok());
        assert!(validate_price(0.001).is_ok());
        assert!(validate_price(0.0).is_ok());
        
        assert!(validate_price(-1.0).is_err());
        assert!(validate_price(f64::NAN).is_err());
        assert!(validate_price(f64::INFINITY).is_err());
    }

    #[test]
    fn test_calculate_percentage_change() {
        assert_eq!(calculate_percentage_change(110.0, 100.0).unwrap(), 10.0);
        assert_eq!(calculate_percentage_change(90.0, 100.0).unwrap(), -10.0);
        assert_eq!(calculate_percentage_change(100.0, 100.0).unwrap(), 0.0);
        
        assert!(calculate_percentage_change(100.0, 0.0).is_err());
    }

    #[test]
    fn test_get_change_arrow() {
        assert_eq!(get_change_arrow(5.0), "📈");
        assert_eq!(get_change_arrow(-5.0), "📉");
        assert_eq!(get_change_arrow(0.0), "➡️");
    }

    #[test]
    fn test_bot_config_validation() {
        let mut config = BotConfig {
            discord_token: "test_token".to_string(),
            crypto_name: "BTC".to_string(),
            update_interval: Duration::from_secs(60),
            tracking_duration: Duration::from_secs(3600),
            pyth_feed_id: None,
        };
        
        assert!(config.validate().is_ok());
        
        config.discord_token = "".to_string();
        assert!(config.validate().is_err());
        
        config.discord_token = "test_token".to_string();
        config.crypto_name = "".to_string();
        assert!(config.validate().is_err());
        
        config.crypto_name = "BTC".to_string();
        config.update_interval = Duration::from_secs(0);
        assert!(config.validate().is_err());
    }
}

