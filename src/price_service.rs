use reqwest;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;
use dotenv::dotenv;
use rusqlite::{Connection, Result as SqliteResult};


const HERMES_API_URL: &str = "https://hermes.pyth.network/api/latest_price_feeds";
const DATABASE_PATH: &str = "shared/prices.db";

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PriceData {
    pub price: f64,
    pub timestamp: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PricesFile {
    pub prices: HashMap<String, PriceData>,
    pub timestamp: u64,
}



fn init_database() -> SqliteResult<Connection> {
    // Create shared directory if it doesn't exist
    let shared_dir = "shared";
    if !Path::new(shared_dir).exists() {
        fs::create_dir(shared_dir).map_err(|e| rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            Some(format!("Failed to create shared directory: {}", e))
        ))?;
        println!("📁 Created shared directory");
    }
    
    let conn = Connection::open(DATABASE_PATH)?;
    
    // Create prices table if it doesn't exist
    conn.execute(
        "CREATE TABLE IF NOT EXISTS prices (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            crypto_name TEXT NOT NULL,
            price REAL NOT NULL,
            timestamp INTEGER NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        [],
    )?;
    
    // Create index for faster queries
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_crypto_timestamp ON prices(crypto_name, timestamp)",
        [],
    )?;
    
    println!("🗄️ Database initialized at {}", DATABASE_PATH);
    Ok(conn)
}

fn store_prices_in_db(conn: &Connection, prices: &HashMap<String, PriceData>) -> SqliteResult<()> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some(format!("System time error: {}", e))
        ))?
        .as_secs();
    
    for (crypto_name, price_data) in prices {
        conn.execute(
            "INSERT INTO prices (crypto_name, price, timestamp) VALUES (?, ?, ?)",
            [&crypto_name, &price_data.price.to_string(), &timestamp.to_string()],
        )?;
    }
    
    Ok(())
}

fn cleanup_old_prices(conn: &Connection) -> SqliteResult<()> {
    // Delete prices older than 60 days (60 * 24 * 60 * 60 = 5184000 seconds)
    let sixty_days_ago = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some(format!("System time error: {}", e))
        ))?
        .as_secs() - 5184000;
    
    let deleted = conn.execute(
        "DELETE FROM prices WHERE timestamp < ?",
        [&sixty_days_ago.to_string()],
    )?;
    
    if deleted > 0 {
        println!("🧹 Cleaned up {} old price records (older than 60 days)", deleted);
    }
    
    Ok(())
}

fn get_feed_ids() -> HashMap<String, String> {
    let mut feeds = HashMap::new();
    
    // Read from environment variable CRYPTO_FEEDS
    // Format: BTC:0x...,ETH:0x...,SOL:0x...,WIF:0x...
    let feeds_str = std::env::var("CRYPTO_FEEDS").unwrap_or_else(|_| {
        // Default feeds if not specified
        "BTC:0xe62df6c8b4a85fe1a67db44dc12de5db330f7ac66b72dc658afedf0f4a415b43,ETH:0xff61491a931112ddf1bd8147cd1b641375f79f5825126d665480874634fd0ace,SOL:0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d".to_string()
    });
    
    for pair in feeds_str.split(',') {
        if let Some((name, feed_id)) = pair.split_once(':') {
            feeds.insert(name.trim().to_string(), feed_id.trim().to_string());
        }
    }
    
    feeds
}

async fn get_crypto_price(feed_id: &str) -> Result<f64, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}?ids[]={}", HERMES_API_URL, feed_id);
    const MAX_RETRIES: u32 = 3;
    
    for attempt in 1..=MAX_RETRIES {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
            
        match client.get(&url)
            .header("User-Agent", "Crypto-Price-Service/1.0")
            .send()
            .await
        {
            Ok(response) => {
                if !response.status().is_success() {
                    println!("❌ HTTP request failed (attempt {}): {}", attempt, response.status());
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(std::time::Duration::from_millis(1000 * attempt as u64)).await;
                        continue;
                    }
                    return Err(format!("HTTP request failed: {}", response.status()).into());
                }
                
                match response.json::<Value>().await {
                    Ok(json) => {
                        // Parse the price from the JSON array format
                        if let Some(feeds_array) = json.as_array() {
                            if let Some(first_feed) = feeds_array.first() {
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
                        return Err("Failed to parse price data".into());
                    }
                    Err(e) => {
                        println!("❌ JSON parsing failed (attempt {}): {}", attempt, e);
                        if attempt < MAX_RETRIES {
                            tokio::time::sleep(std::time::Duration::from_millis(1000 * attempt as u64)).await;
                            continue;
                        }
                        return Err(e.into());
                    }
                }
            }
            Err(e) => {
                println!("❌ Network request failed (attempt {}): {}", attempt, e);
                if attempt < MAX_RETRIES {
                    tokio::time::sleep(std::time::Duration::from_millis(1000 * attempt as u64)).await;
                    continue;
                }
                return Err(e.into());
            }
        }
    }
    
    unreachable!()
}

async fn fetch_all_prices() -> Result<PricesFile, Box<dyn std::error::Error + Send + Sync>> {
    let feeds = get_feed_ids();
    let mut prices = HashMap::new();
    
    for (crypto, feed_id) in feeds {
        match get_crypto_price(&feed_id).await {
            Ok(price) => {
                prices.insert(crypto.clone(), PriceData { price, timestamp: 0 });
                println!("✅ Fetched {} price: ${:.6}", crypto, price);
            }
            Err(e) => {
                println!("❌ Failed to fetch {} price: {}", crypto, e);
                // Use previous price or default
                let default_price = match crypto.as_str() {
                    "BTC" => 45000.0,
                    "ETH" => 2800.0,
                    "SOL" => 95.0,
                    "WIF" => 2.0,
                    _ => 1.0,
                };
                prices.insert(crypto, PriceData { price: default_price, timestamp: 0 });
            }
        }
    }
    
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("System time error: {}", e))?
        .as_secs();
    
    // Update timestamps for all prices
    for price_data in prices.values_mut() {
        price_data.timestamp = timestamp;
    }
    
    Ok(PricesFile {
        prices,
        timestamp,
    })
}

async fn write_prices_to_file(prices: &PricesFile, file_path: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json_string = serde_json::to_string_pretty(prices)?;
    fs::write(file_path, json_string)?;
    println!("📝 Wrote prices to {}", file_path);
    Ok(())
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    
    // Get update interval from environment
    let update_interval = std::env::var("UPDATE_INTERVAL_SECONDS")
        .unwrap_or_else(|_| "12".to_string())
        .parse::<u64>()
        .unwrap_or(12);
    
    // Create shared directory if it doesn't exist
    let shared_dir = "shared";
    if !Path::new(shared_dir).exists() {
        if let Err(e) = fs::create_dir(shared_dir) {
            eprintln!("Failed to create shared directory: {}", e);
            return;
        }
        println!("📁 Created shared directory");
    }
    
    let file_path = format!("{}/prices.json", shared_dir);
    
    // Initialize database
    let conn = match init_database() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("❌ Failed to initialize database: {}", e);
            return;
        }
    };
    
    println!("🚀 Starting Price Service...");
    println!("📊 Update interval: {} seconds", update_interval);
    println!("📁 Prices file: {}", file_path);
    println!("🗄️ Database: {}", DATABASE_PATH);
    
    // Print configured cryptos
    let feeds = get_feed_ids();
    println!("🪙 Tracking cryptos: {}", feeds.keys().cloned().collect::<Vec<_>>().join(", "));
    
    let mut cleanup_counter = 0;
    
    let mut consecutive_failures = 0;
    const MAX_CONSECUTIVE_FAILURES: u32 = 5;
    
    loop {
        let loop_start = std::time::Instant::now();
        
        match fetch_all_prices().await {
            Ok(prices) => {
                consecutive_failures = 0; // Reset failure counter on success
                
                // Store in JSON file (for backward compatibility)
                if let Err(e) = write_prices_to_file(&prices, &file_path).await {
                    println!("❌ Failed to write prices to JSON: {}", e);
                } else {
                    println!("📝 Successfully wrote prices to JSON file");
                }
                
                // Store in SQLite database
                if let Err(e) = store_prices_in_db(&conn, &prices.prices) {
                    println!("❌ Failed to store prices in database: {}", e);
                } else {
                    println!("💾 Stored prices in database");
                }
                
                // Clean up old prices every 100 updates (about 20 minutes)
                cleanup_counter += 1;
                if cleanup_counter >= 100 {
                    if let Err(e) = cleanup_old_prices(&conn) {
                        println!("❌ Failed to cleanup old prices: {}", e);
                    } else {
                        println!("🧹 Database cleanup completed");
                    }
                    cleanup_counter = 0;
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                println!("❌ Failed to fetch prices (failure {}/{}): {}", 
                        consecutive_failures, MAX_CONSECUTIVE_FAILURES, e);
                
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    println!("⚠️ Too many consecutive failures. Entering recovery mode for 60 seconds...");
                    sleep(Duration::from_secs(60)).await;
                    consecutive_failures = 0; // Reset after recovery delay
                }
            }
        }
        
        // Calculate how long the update took and adjust sleep time
        let loop_duration = loop_start.elapsed();
        let target_interval = Duration::from_secs(update_interval);
        
        if loop_duration < target_interval {
            let sleep_time = target_interval - loop_duration;
            println!("⏱️ Update took {:?}, sleeping for {:?}", loop_duration, sleep_time);
            sleep(sleep_time).await;
        } else {
            println!("⚠️ Update took longer than interval: {:?} > {:?}", loop_duration, target_interval);
            // Still sleep for a minimum time to prevent tight loops
            sleep(Duration::from_secs(1)).await;
        }
    }
} 