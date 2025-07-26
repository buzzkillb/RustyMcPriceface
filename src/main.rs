mod errors;
mod config;
mod utils;
mod health;
mod health_server;

use errors::{BotError, BotResult};
use config::{BotConfig, CLEANUP_INTERVAL_SECONDS, PRICE_HISTORY_DAYS};
use utils::{
    validate_crypto_name, get_current_timestamp, format_price, get_crypto_emoji,
    validate_price, calculate_percentage_change, get_change_arrow
};
use health::HealthState;

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

use serenity::all::ActivityData;
use std::fs;
use reqwest;
use serde_json::Value;
use std::collections::HashMap;
use rusqlite::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{info, warn, error, debug};
use std::sync::Mutex;

const TRACKING_DURATION_SECONDS: u64 = 3600; // 1 hour
const MAX_RETRIES: u32 = 3;
const RATE_LIMIT_DELAY_MS: u64 = 2000; // 2 seconds between Discord API calls
const RECONNECT_DELAY_SECONDS: u64 = 30;

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
    health: HealthState,
}

impl Bot {
    /// Create a new bot instance with configuration
    pub fn new(config: BotConfig) -> Self {
        let health = HealthState::new(config.crypto_name.clone());
        Self { config, health }
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

/// Fetch individual cryptocurrency price from Pyth Network with retry logic
async fn get_individual_crypto_price(feed_id: &str) -> BotResult<f64> {
    let url = format!("https://hermes.pyth.network/v2/updates/price/latest?ids%5B%5D={}", feed_id);
    
    for attempt in 1..=MAX_RETRIES {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| BotError::Http(e.to_string()))?;
            
        match client.get(&url)
            .header("User-Agent", "Crypto-Price-Bot/1.0")
            .send()
            .await
        {
            Ok(response) => {
                if !response.status().is_success() {
                    error!("HTTP request failed (attempt {}): {}", attempt, response.status());
                    if attempt < MAX_RETRIES {
                        sleep(Duration::from_millis(1000 * attempt as u64)).await;
                        continue;
                    }
                    return Err(BotError::Http(format!("HTTP request failed: {}", response.status())));
                }
                
                match response.json::<Value>().await {
                    Ok(json) => {
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
                        return Ok(real_price);
                    }
                    Err(e) => {
                        error!("JSON parsing failed (attempt {}): {}", attempt, e);
                        if attempt < MAX_RETRIES {
                            sleep(Duration::from_millis(1000 * attempt as u64)).await;
                            continue;
                        }
                        return Err(BotError::Http(e.to_string()));
                    }
                }
            }
            Err(e) => {
                error!("Network request failed (attempt {}): {}", attempt, e);
                if attempt < MAX_RETRIES {
                    sleep(Duration::from_millis(1000 * attempt as u64)).await;
                    continue;
                }
                return Err(BotError::Http(e.to_string()));
            }
        }
    }
    
    unreachable!()
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
/// Get a database connection with retry logic
fn get_db_connection() -> BotResult<Connection> {
    for attempt in 1..=MAX_RETRIES {
        match Connection::open("shared/prices.db") {
            Ok(conn) => return Ok(conn),
            Err(e) => {
                error!("Database connection attempt {} failed: {}", attempt, e);
                if attempt < MAX_RETRIES {
                    std::thread::sleep(Duration::from_millis(1000 * attempt as u64));
                } else {
                    return Err(BotError::Database(e));
                }
            }
        }
    }
    unreachable!()
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
        (2592000, "30d"), // 30 days in seconds
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
        
        // Only add the change if we have data for that time period
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
    
    // Keep only the last 60 days of data
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
        info!("Bot ID: {}", ready.user.id);
        info!("Connected to {} guilds", ready.guilds.len());
        
        info!("Starting command registration...");
        
        // Register slash commands with retry logic
        for attempt in 1..=MAX_RETRIES {
            match self.register_commands(&ctx.http).await {
                Ok(_) => {
                    info!("Command registration completed successfully");
                    break;
                }
                Err(e) => {
                    error!("Command registration failed (attempt {}): {}", attempt, e);
                    if attempt < MAX_RETRIES {
                        sleep(Duration::from_secs(5)).await;
                    } else {
                        error!("Failed to register commands after {} attempts", MAX_RETRIES);
                        return;
                    }
                }
            }
        }
        
        info!("Starting price update loop...");
        
        let http = ctx.http.clone();
        let ctx_arc = Arc::new(ctx);
        let config = self.config.clone();
        let health = Arc::new(self.health.clone());
        
        // Start health check server
        let health_clone = health.clone();
        tokio::spawn(async move {
            health_server::start_health_server(health_clone, 8080).await;
        });
        
        tokio::spawn(async move {
            price_update_loop(http, ctx_arc, config, health).await;
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

/// Rate-limited Discord API call helper
async fn rate_limited_discord_call<F, Fut, T>(mut operation: F) -> Result<T, serenity::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, serenity::Error>>,
{
    static LAST_CALL: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    
    // Enforce rate limiting
    let sleep_time = {
        let mut last_call = LAST_CALL.lock().unwrap();
        let now = std::time::Instant::now();
        
        if let Some(last) = *last_call {
            let elapsed = last.elapsed();
            let min_interval = Duration::from_millis(RATE_LIMIT_DELAY_MS);
            if elapsed < min_interval {
                let sleep_duration = min_interval - elapsed;
                *last_call = Some(now);
                Some(sleep_duration)
            } else {
                *last_call = Some(now);
                None
            }
        } else {
            *last_call = Some(now);
            None
        }
    };
    
    // Sleep outside the mutex lock if needed
    if let Some(duration) = sleep_time {
        debug!("Rate limiting: sleeping for {:?}", duration);
        sleep(duration).await;
    }
    
    // Execute the operation with retry logic
    for attempt in 1..=MAX_RETRIES {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if e.to_string().contains("rate limit") || e.to_string().contains("429") {
                    let backoff_time = Duration::from_secs(2_u64.pow(attempt));
                    warn!("Rate limited, backing off for {:?} (attempt {})", backoff_time, attempt);
                    sleep(backoff_time).await;
                } else if attempt < MAX_RETRIES {
                    warn!("Discord API call failed (attempt {}): {}", attempt, e);
                    sleep(Duration::from_millis(1000 * attempt as u64)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    
    unreachable!()
}

/// Read prices from the shared JSON file with retry logic
async fn read_prices_from_file() -> BotResult<PricesFile> {
    let file_path = "shared/prices.json";
    
    for attempt in 1..=MAX_RETRIES {
        // Check if file exists
        if !std::path::Path::new(file_path).exists() {
            if attempt < MAX_RETRIES {
                warn!("Prices file not found (attempt {}), retrying...", attempt);
                sleep(Duration::from_millis(1000 * attempt as u64)).await;
                continue;
            }
            return Err(BotError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Prices file not found. Make sure price-service is running."
            )));
        }
        
        match fs::read_to_string(file_path) {
            Ok(content) => {
                match serde_json::from_str::<PricesFile>(&content) {
                    Ok(prices) => return Ok(prices),
                    Err(e) => {
                        error!("Failed to parse prices file (attempt {}): {}", attempt, e);
                        if attempt < MAX_RETRIES {
                            sleep(Duration::from_millis(1000 * attempt as u64)).await;
                            continue;
                        }
                        return Err(BotError::Json(e));
                    }
                }
            }
            Err(e) => {
                error!("Failed to read prices file (attempt {}): {}", attempt, e);
                if attempt < MAX_RETRIES {
                    sleep(Duration::from_millis(1000 * attempt as u64)).await;
                    continue;
                }
                return Err(BotError::Io(e));
            }
        }
    }
    
    unreachable!()
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

/// Main price update loop with comprehensive error handling
async fn price_update_loop(http: Arc<Http>, ctx: Arc<Context>, config: BotConfig, health: Arc<HealthState>) {
    let crypto_name = &config.crypto_name;
    let mut consecutive_failures = 0;
    const MAX_CONSECUTIVE_FAILURES: u32 = 5;
    
    info!("Starting price update loop for {}", crypto_name);
    
    loop {
        let loop_start = std::time::Instant::now();
        
        // Wrap the entire update logic in error handling
        let update_result = async {
            // Get current price with error handling
            let current_price = match get_crypto_price(&config).await {
                Ok(price) => {
                    consecutive_failures = 0; // Reset failure counter on success
                    health.reset_failures();
                    health.update_price_timestamp();
                    price
                }
                Err(e) => {
                    consecutive_failures += 1;
                    health.increment_failures();
                    error!("Failed to get {} price (failure {}/{}): {}", 
                           crypto_name, consecutive_failures, MAX_CONSECUTIVE_FAILURES, e);
                    
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        error!("Too many consecutive failures for {}. Entering recovery mode.", crypto_name);
                        sleep(Duration::from_secs(RECONNECT_DELAY_SECONDS)).await;
                        consecutive_failures = 0; // Reset after recovery delay
                        health.reset_failures();
                    }
                    return Err(e);
                }
            };

            // Get price change indicator with error handling
            let (arrow, change_percent) = get_price_indicator_from_db(crypto_name, current_price);

            // Format the nickname
            let nickname = format!("{} {}", crypto_name, format_price(current_price));

            // Format the custom status with rotation
            let update_count = match get_current_timestamp() {
                Ok(time) => (time / 12) % 4,
                Err(_) => 0,
            };

            let custom_status = match read_prices_from_file().await {
                Ok(shared_prices) => {
                    // Calculate ticker price in terms of BTC, ETH, SOL
                    let btc_amount = current_price / shared_prices.prices.get("BTC").map(|p| p.price).unwrap_or(45000.0);
                    let eth_amount = current_price / shared_prices.prices.get("ETH").map(|p| p.price).unwrap_or(2800.0);
                    let sol_amount = current_price / shared_prices.prices.get("SOL").map(|p| p.price).unwrap_or(95.0);

                    match crypto_name.as_str() {
                        "BTC" => {
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
                                3 => format!("{:.8} Ξ", eth_amount),
                                _ => unreachable!(),
                            }
                        },
                        "ETH" => {
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
                                3 => format!("{:.8} ₿", btc_amount),
                                _ => unreachable!(),
                            }
                        },
                        "SOL" => {
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
                                3 => format!("{:.8} ₿", btc_amount),
                                _ => unreachable!(),
                            }
                        },
                        _ => {
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
                }
                Err(e) => {
                    warn!("Failed to read shared prices for status: {}", e);
                    if change_percent == 0.0 && arrow == "🔄" {
                        format!("{} Building history", arrow)
                    } else {
                        let change_sign = if change_percent >= 0.0 { "+" } else { "" };
                        format!("{} {}{:.2}% (1h)", arrow, change_sign, change_percent)
                    }
                }
            };

            debug!("Updating nickname to: {}", nickname);
            debug!("Updating custom status to: {}", custom_status);

            // Update custom status (activity) with error handling
            ctx.set_activity(Some(ActivityData::playing(custom_status.clone())));
            debug!("Updated activity status");

            // Save current price to database with error handling
            match get_db_connection() {
                Ok(conn) => {
                    if let Ok(current_time) = get_current_timestamp() {
                        match conn.prepare("INSERT INTO prices (crypto_name, price, timestamp) VALUES (?, ?, ?)") {
                            Ok(mut stmt) => {
                                if let Err(e) = stmt.execute([crypto_name, &current_price.to_string(), &current_time.to_string()]) {
                                    error!("Failed to save price to database: {}", e);
                                } else {
                                    debug!("Saved {} price to database: ${}", crypto_name, current_price);
                                    health.update_db_timestamp();
                                }
                            }
                            Err(e) => error!("Failed to prepare database statement: {}", e),
                        }
                    }
                }
                Err(e) => error!("Failed to get database connection: {}", e),
            }

            // Update nickname in guilds with rate limiting and error handling
            let guilds = ctx.cache.guilds();
            let guild_count = guilds.len();
            
            if guild_count > 0 {
                info!("Updating nickname in {} guilds", guild_count);
                
                for (index, guild_id) in guilds.iter().enumerate() {
                    let http_clone = http.clone();
                    let nickname_clone = nickname.clone();
                    let guild_id_copy = *guild_id;
                    
                    match rate_limited_discord_call(|| {
                        let http_ref = http_clone.clone();
                        let nickname_ref = nickname_clone.clone();
                        async move {
                            http_ref.edit_nickname(guild_id_copy, Some(&nickname_ref), None).await
                        }
                    }).await {
                        Ok(_) => {
                            debug!("Updated nickname in guild {} ({}/{})", guild_id, index + 1, guild_count);
                            health.update_discord_timestamp();
                        }
                        Err(e) => {
                            if e.to_string().contains("rate limit") || e.to_string().contains("429") {
                                warn!("Rate limited while updating nickname in guild {}: {}", guild_id, e);
                            } else {
                                warn!("Failed to update nickname in guild {}: {}", guild_id, e);
                            }
                        }
                    }
                }
            } else {
                debug!("No guilds found in cache");
            }

            Ok(())
        }.await;

        // Handle update result
        match update_result {
            Ok(_) => {
                debug!("Price update completed successfully for {}", crypto_name);
            }
            Err(e) => {
                error!("Price update failed for {}: {}", crypto_name, e);
            }
        }

        // Periodic cleanup of old prices (every 24 hours)
        static LAST_CLEANUP: AtomicU64 = AtomicU64::new(0);
        if let Ok(current_time) = get_current_timestamp() {
            let last_cleanup = LAST_CLEANUP.load(Ordering::Relaxed);
            if current_time - last_cleanup > CLEANUP_INTERVAL_SECONDS {
                match cleanup_old_prices() {
                    Ok(_) => debug!("Database cleanup completed"),
                    Err(e) => error!("Failed to cleanup old prices: {}", e),
                }
                LAST_CLEANUP.store(current_time, Ordering::Relaxed);
            }
        }

        // Calculate how long the update took and adjust sleep time
        let loop_duration = loop_start.elapsed();
        let target_interval = config.update_interval;
        
        if loop_duration < target_interval {
            let sleep_time = target_interval - loop_duration;
            debug!("Update took {:?}, sleeping for {:?}", loop_duration, sleep_time);
            sleep(sleep_time).await;
        } else {
            warn!("Update took longer than interval: {:?} > {:?}", loop_duration, target_interval);
            // Still sleep for a minimum time to prevent tight loops
            sleep(Duration::from_secs(1)).await;
        }
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

/// Start the bot with proper error handling and reconnection capability
async fn start_bot_with_reconnection(config: &BotConfig) -> BotResult<()> {
    let intents = GatewayIntents::GUILDS;
    
    let bot = Bot::new(config.clone());
    
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(bot)
        .await
        .map_err(|e| BotError::Discord(format!("Failed to create client: {}", e)))?;
    
    info!("Starting Discord client...");
    
    // Start the client with error handling
    match client.start().await {
        Ok(_) => {
            info!("Discord client started successfully");
            Ok(())
        }
        Err(e) => {
            error!("Discord client error: {}", e);
            Err(BotError::Discord(format!("Client error: {}", e)))
        }
    }
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

