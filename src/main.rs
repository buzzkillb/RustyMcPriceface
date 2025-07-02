use serenity::{
    async_trait,
    model::gateway::Ready,
    prelude::*,
    http::Http,
};
use std::time::Duration;
use tokio::time::sleep;
use std::sync::Arc;
use dotenv::dotenv;
use std::sync::Mutex;
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};
use serenity::all::ActivityData;
use std::fs;
use reqwest;
use serde_json::Value;
use std::collections::HashMap;
use rusqlite::Connection;

const TRACKING_DURATION_SECONDS: u64 = 3600; // 1 hour

#[derive(Clone)]
struct PricePoint {
    price: f64,
    timestamp: u64,
}

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

struct Bot {
    price_history: Arc<Mutex<VecDeque<PricePoint>>>,
}

impl Bot {
    fn new() -> Self {
        Bot {
            price_history: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

fn get_crypto_name() -> String {
    std::env::var("CRYPTO_NAME")
        .unwrap_or_else(|_| "SOL".to_string())
}

fn get_pyth_feed_id() -> Option<String> {
    std::env::var("PYTH_FEED_ID").ok()
}

async fn get_individual_crypto_price(feed_id: &str) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://hermes.pyth.network/v2/updates/price/latest?ids%5B%5D={}", feed_id);
    
    let client = reqwest::Client::new();
    let response = client.get(&url)
        .header("User-Agent", "Crypto-Price-Bot/1.0")
        .send()
        .await?;
    
    if !response.status().is_success() {
        return Err(format!("HTTP request failed: {}", response.status()).into());
    }
    
    let json: Value = response.json().await?;
    
    // Parse the price from the parsed array
    if let Some(parsed_data) = json.get("parsed").and_then(|p| p.as_array()) {
        if let Some(first_feed) = parsed_data.first() {
            if let Some(price_data) = first_feed.get("price") {
                if let Some(price_str) = price_data.get("price").and_then(|p| p.as_str()) {
                    if let Ok(price) = price_str.parse::<i64>() {
                        let expo = price_data.get("expo").and_then(|e| e.as_i64()).unwrap_or(0);
                        let real_price = price as f64 * 10f64.powi(expo as i32);
                        return Ok(real_price);
                    }
                }
            }
        }
    }
    
    Err("Failed to parse price data".into())
}

fn format_about_me(current_price: f64, shared_prices: &PricesFile) -> String {
    let crypto_name = get_crypto_name();
    
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

fn format_price(price: f64) -> String {
    if price >= 1000.0 {
        // No decimals for prices >= $1000
        format!("${:.0}", price)
    } else if price >= 100.0 {
        // 2 decimal places for prices >= $100
        format!("${:.2}", price)
    } else if price >= 1.0 {
        // 3 decimal places for prices >= $1
        format!("${:.3}", price)
    } else {
        // 4 decimal places for prices < $1
        format!("${:.4}", price)
    }
}

fn get_crypto_emoji(crypto: &str) -> &'static str {
    match crypto {
        "BTC" => "🪙",
        "ETH" => "🪙",
        "SOL" => "📊",
        "WIF" => "🐕",
        "DOGE" => "🐕",
        "MATIC" => "🔷",
        "AVAX" => "❄️",
        "ADA" => "🔷",
        "DOT" => "🔴",
        "LINK" => "🔗",
        "UNI" => "🦄",
        "ATOM" => "⚛️",
        "LTC" => "Ł",
        "BCH" => "₿",
        "XRP" => "💎",
        "TRX" => "⚡",
        _ => "🪙",
    }
}

// Database functions for slash commands
fn get_db_connection() -> Result<Connection, rusqlite::Error> {
    Connection::open("shared/prices.db")
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

#[async_trait]
impl EventHandler for Bot {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("Bot is ready! Logged in as: {}", ready.user.name);
        
        let http = ctx.http.clone();
        let ctx_arc = Arc::new(ctx);
        let price_history = self.price_history.clone();
        
        tokio::spawn(async move {
            price_update_loop(http, ctx_arc, price_history).await;
        });
    }
}

async fn read_prices_from_file() -> Result<PricesFile, Box<dyn std::error::Error + Send + Sync>> {
    let file_path = "shared/prices.json";
    
    // Check if file exists
    if !std::path::Path::new(file_path).exists() {
        return Err("Prices file not found. Make sure price-service is running.".into());
    }
    
    let content = fs::read_to_string(file_path)?;
    let prices: PricesFile = serde_json::from_str(&content)?;
    Ok(prices)
}

async fn get_crypto_price() -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
    let crypto_name = get_crypto_name();
    
    // First try to get from shared prices file
    match read_prices_from_file().await {
        Ok(prices) => {
            if let Some(price_data) = prices.prices.get(&crypto_name) {
                return Ok(price_data.price);
            }
        }
        Err(_) => {
            // If shared file doesn't exist or doesn't have our crypto, try direct API call
        }
    }
    
    // Fallback to direct API call if we have a feed ID
    if let Some(feed_id) = get_pyth_feed_id() {
        return get_individual_crypto_price(&feed_id).await;
    }
    
    Err(format!("Could not find price for {}", crypto_name).into())
}

fn get_price_indicator(current_price: f64, price_history: &mut VecDeque<PricePoint>) -> (String, f64) {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Add current price to history
    price_history.push_back(PricePoint {
        price: current_price,
        timestamp: current_time,
    });
    
    // Remove old prices (older than 1 hour)
    while let Some(front) = price_history.front() {
        if current_time - front.timestamp > TRACKING_DURATION_SECONDS {
            price_history.pop_front();
        } else {
            break;
        }
    }
    
    // Get the oldest price in our 1-hour window
    if let Some(oldest_price) = price_history.front() {
        let change = current_price - oldest_price.price;
        let change_percent = (change / oldest_price.price) * 100.0;
        
        let arrow = if change > 0.0 {
            "📈" // Up arrow
        } else if change < 0.0 {
            "📉" // Down arrow
        } else {
            "➡️" // Side arrow (no change)
        };
        
        (arrow.to_string(), change_percent)
    } else {
        // No history yet
        ("🔄".to_string(), 0.0)
    }
}

async fn price_update_loop(http: Arc<Http>, ctx: Arc<Context>, price_history: Arc<Mutex<VecDeque<PricePoint>>>) {
    // Get update interval from environment
    let update_interval = std::env::var("UPDATE_INTERVAL_SECONDS")
        .unwrap_or_else(|_| "300".to_string())
        .parse::<u64>()
        .unwrap_or(300);
    
    loop {
        match get_crypto_price().await {
            Ok(current_price) => {
                // Get price change over last 30 minutes
                let (arrow, change_percent) = {
                    let mut history_guard = price_history.lock().unwrap();
                    get_price_indicator(current_price, &mut history_guard)
                };
                
                // Format the nickname (just the price)
                let crypto_name = get_crypto_name();
                let nickname = format!("{} {}", crypto_name, format_price(current_price));
                
                // Format the custom status with rotation
                let update_count = (SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() / 12) % 4;
                
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
                
                println!("Updating nickname to: {}", nickname);
                println!("Updating custom status to: {}", custom_status);
                
                // Update custom status (activity)
                ctx.set_activity(Some(ActivityData::playing(custom_status)));
                
                // Update "About Me" with cross-rates
                if let Ok(shared_prices) = read_prices_from_file().await {
                    let about_me = format_about_me(current_price, &shared_prices);
                    // Note: Discord bot profiles can't be updated via API
                    // This would require OAuth2 user token, not bot token
                    println!("📝 About Me would be: {}", about_me);
                }
                
                // Iterate over all guilds and update nickname
                let guilds = ctx.cache.guilds();
                for guild_id in guilds {
                    match http.edit_nickname(guild_id, Some(&nickname), None).await {
                        Ok(_) => println!("✅ Updated nickname in guild {}", guild_id),
                        Err(e) => println!("❌ Failed to update nickname in guild {}: {}", guild_id, e),
                    }
                }
            }
            Err(e) => {
                println!("❌ Failed to get {} price: {}", get_crypto_name(), e);
            }
        }
        
        // Wait for the next update using the configurable interval
        sleep(Duration::from_secs(update_interval)).await;
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    
    // Load bot token from environment variable
    let token = std::env::var("DISCORD_TOKEN")
        .expect("Expected a token in the environment variable DISCORD_TOKEN");
    
    // Load update interval from environment variable with fallback
    let update_interval = std::env::var("UPDATE_INTERVAL_SECONDS")
        .unwrap_or_else(|_| "300".to_string())
        .parse::<u64>()
        .unwrap_or(300);
    
    let crypto_name = get_crypto_name();
    
    println!("Starting Multi-Crypto Price Discord Bot...");
    println!("Update interval: {} seconds", update_interval);
    println!("Price tracking: Last 1 hour");
    println!("Crypto: {}", crypto_name);
    println!("Reading from: shared/prices.json");
    
    // Create a new instance of the bot
    let intents = GatewayIntents::default();
    let mut client = Client::builder(&token, intents)
        .event_handler(Bot::new())
        .await
        .expect("Error creating client");
    
    // Start the bot
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}

