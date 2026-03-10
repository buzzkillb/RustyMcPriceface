use reqwest;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;


const HERMES_API_URL: &str = "https://hermes.pyth.network/api/latest_price_feeds";

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PriceData {
    pub price: f64,
    pub timestamp: u64,
    // Optional fields for detailed data (e.g., Shanghai Premium)
    pub premium: Option<f64>,
    pub premium_percent: Option<f64>,
    pub source: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct HistoryData {
    pub date: String, // "YYYY-MM-DD"
    pub shanghai: f64,
    pub western: f64,
    pub premium: f64,
    #[serde(rename = "premiumPercent")]
    pub premium_percent: f64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PricesFile {
    pub prices: HashMap<String, PriceData>,
    pub timestamp: u64,
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
                    error!("HTTP request failed (attempt {}): {}", attempt, response.status());
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
                        error!("JSON parsing failed (attempt {}): {}", attempt, e);
                        if attempt < MAX_RETRIES {
                            tokio::time::sleep(std::time::Duration::from_millis(1000 * attempt as u64)).await;
                            continue;
                        }
                        return Err(e.into());
                    }
                }
            }
            Err(e) => {
                error!("Network request failed (attempt {}): {}", attempt, e);
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

pub async fn fetch_shanghai_history(range: &str, symbol: Option<&str>) -> Result<Vec<HistoryData>, Box<dyn std::error::Error + Send + Sync>> {
    let symbol_param = symbol.unwrap_or("");
    let url = format!(
        "https://metalcharts.org/api/shanghai/history?range={}{}", 
        range,
        if symbol_param.is_empty() { "".to_string() } else { format!("&symbol={}", symbol_param) }
    );
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = client.get(&url)
        .header("Origin", "https://metalcharts.org")
        .header("Referer", "https://metalcharts.org/")
        .header("User-Agent", "Mozilla/5.0 (compatible; RustyMcPriceface/1.0)")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("Shanghai History API request failed: {}", response.status()).into());
    }

    let json: Value = response.json().await?;
    
    // The API returns { data: [...], symbol: "..." }
    if let Some(data_array) = json.get("data") {
        let history: Vec<HistoryData> = serde_json::from_value(data_array.clone())?;
        return Ok(history);
    }

    Err("Failed to parse Shanghai History JSON structure".into())
}

async fn fetch_yahoo_price(ticker: &str) -> Result<PriceData, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://query1.finance.yahoo.com/v8/finance/chart/{}?interval=1d&range=1d", ticker);
    const MAX_RETRIES: u32 = 3;

    for attempt in 1..=MAX_RETRIES {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        match client.get(&url)
            .header("User-Agent", "Mozilla/5.0 (compatible; RustyMcPriceface/1.0)")
            .send()
            .await
        {
            Ok(response) => {
                if !response.status().is_success() {
                    error!("Yahoo API request failed for {} (attempt {}): {}", ticker, attempt, response.status());
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(std::time::Duration::from_millis(1000 * attempt as u64)).await;
                        continue;
                    }
                    return Err(format!("Yahoo API HTTP request failed: {}", response.status()).into());
                }

                match response.json::<Value>().await {
                    Ok(json) => {
                        // Navigate to chart.result[0].meta.regularMarketPrice
                        if let Some(result) = json.get("chart").and_then(|c| c.get("result")).and_then(|r| r.get(0)) {
                            if let Some(meta) = result.get("meta") {
                                let price = meta.get("regularMarketPrice").and_then(|p| p.as_f64())
                                    .ok_or("Missing regularMarketPrice")?;
                                
                                // Parse timestamp if available, else use current
                                let timestamp = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)?
                                    .as_secs();

                                return Ok(PriceData {
                                    price,
                                    timestamp,
                                    premium: None,
                                    premium_percent: None,
                                    source: Some("yahoo".to_string()),
                                });
                            }
                        }
                        return Err("Failed to parse Yahoo JSON structure".into());
                    }
                    Err(e) => {
                         error!("Yahoo JSON parsing failed: {}", e);
                         return Err(e.into());
                    }
                }
            }
            Err(e) => {
                error!("Yahoo Network request failed: {}", e);
                if attempt < MAX_RETRIES {
                    tokio::time::sleep(std::time::Duration::from_millis(1000 * attempt as u64)).await;
                    continue;
                }
                return Err(e.into());
            }
        }
    }
    Err("Max retries exceeded for Yahoo API".into())
}

async fn fetch_all_prices() -> Result<PricesFile, Box<dyn std::error::Error + Send + Sync>> {
    let feeds = get_feed_ids();
    let mut prices = HashMap::new();
    
    for (crypto, feed_id) in &feeds {
        match get_crypto_price(&feed_id).await {
            Ok(price) => {
                prices.insert(crypto.clone(), PriceData { 
                    price, 
                    timestamp: 0,
                    premium: None,
                    premium_percent: None,
                    source: None
                });
                info!("Fetched {} price: ${:.6}", crypto, price);
            }
            Err(e) => {
                error!("Failed to fetch {} price: {}", crypto, e);
                // Use previous price or default
                let default_price = match crypto.as_str() {
                    "BTC" => 45000.0,
                    "ETH" => 2800.0,
                    "SOL" => 95.0,
                    "WIF" => 2.0,
                    _ => 1.0,
                };
                prices.insert(crypto.clone(), PriceData { 
                    price: default_price, 
                    timestamp: 0,
                    premium: None,
                    premium_percent: None,
                    source: None
                });
            }
        }
    }

    // Shanghai Silver API disabled - using regular SILVER from Pyth network instead
    // if feeds.contains_key("SHANGHAI") {
    //     match fetch_shanghai_price().await {
    //         Ok(data) => {
    //             prices.insert("SHANGHAI".to_string(), data.clone());
    //              println!("✅ Fetched SHANGHAI price: ${:.2} (Premium: ${:.2})", data.price, data.premium.unwrap_or(0.0));
    //         }
    //         Err(e) => {
    //              println!("❌ Failed to fetch SHANGHAI price: {}", e);
    //         }
    //     }
    // }

    // Fetch DXY via Yahoo if configured
    if feeds.contains_key("DXY") {
        match fetch_yahoo_price("DX-Y.NYB").await {
            Ok(data) => {
                prices.insert("DXY".to_string(), data.clone());
                 info!("Fetched DXY price: ${:.2}", data.price);
            }
            Err(e) => {
                 error!("Failed to fetch DXY price: {}", e);
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
    info!("Wrote prices to {}", file_path);
    Ok(())
}

use crate::database::PriceDatabase;
use tracing::{info, error, warn};
use std::sync::Arc;

const GOLDSILVER_AI_URL: &str = "https://goldsilver.ai/metal-prices/shanghai-silver-price";

pub async fn fetch_shanghai_silver_price() -> Result<PriceData, Box<dyn std::error::Error + Send + Sync>> {
    // Use goldsilver.ai for Shanghai Spot price
    let (shanghai_spot, western_spot) = fetch_goldsilver_ai_prices().await?;

    let premium = shanghai_spot - western_spot;
    let premium_percent = (premium / western_spot) * 100.0;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    info!(
        "✅ Shanghai Silver Spot: ${:.2} | Western Spot: ${:.2} | Premium: ${:.2} (+{:.2}%)",
        shanghai_spot, western_spot, premium, premium_percent
    );

    Ok(PriceData {
        price: shanghai_spot,
        timestamp,
        premium: Some(premium),
        premium_percent: Some(premium_percent),
        source: Some("shanghaisilver_spot".to_string()),
    })
}

async fn fetch_goldsilver_ai_prices() -> Result<(f64, f64), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;

    let response = client.get(GOLDSILVER_AI_URL)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await?;

    let text = response.text().await?;

    extract_goldsilver_prices(&text)
}

fn extract_goldsilver_prices(html: &str) -> Result<(f64, f64), Box<dyn std::error::Error + Send + Sync>> {
    // Debug: print a snippet of the HTML around "Shanghai Spot"
    if let Some(pos) = html.find("Shanghai Spot") {
        let start = pos.saturating_sub(50);
        let end = (pos + 100).min(html.len());
        let snippet = &html[start..end];
        info!("HTML snippet around Shanghai Spot: {:?}", snippet);
    }
    
    // More flexible: find the first number after "Shanghai Spot"
    let shanghai_spot = extract_first_number_after(html, "Shanghai Spot")
        .ok_or("Failed to extract Shanghai Spot price")?;

    // More flexible: find the first number after "Western Spot"  
    let western_spot = extract_first_number_after(html, "Western Spot")
        .ok_or("Failed to extract Western Spot price")?;

    info!("Extracted Shanghai Spot: ${:.2}, Western Spot: ${:.2}", shanghai_spot, western_spot);

    Ok((shanghai_spot, western_spot))
}

fn extract_first_number_after(html: &str, prefix: &str) -> Option<f64> {
    if let Some(pos) = html.find(prefix) {
        let after_prefix = &html[pos + prefix.len()..];
        // Look for digits
        let mut chars = after_prefix.chars().peekable();
        let mut number_str = String::new();
        
        // Collect digits and decimal point
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() || c == '.' {
                number_str.push(c);
                chars.next();
            } else if !number_str.is_empty() {
                // Stop when we hit non-digit after starting number
                break;
            } else {
                // Skip non-digit characters before number
                chars.next();
            }
        }
        
        if !number_str.is_empty() {
            return number_str.parse::<f64>().ok();
        }
    }
    None
}

pub async fn run(database: Arc<PriceDatabase>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Get update interval from environment
    let update_interval = std::env::var("UPDATE_INTERVAL_SECONDS")
        .unwrap_or_else(|_| "12".to_string())
        .parse::<u64>()
        .unwrap_or(12);
    
    // Create shared directory if it doesn't exist
    let shared_dir = "shared";
    if !Path::new(shared_dir).exists() {
        if let Err(e) = fs::create_dir(shared_dir) {
            error!("Failed to create shared directory: {}", e);
            return Err(e.into());
        }
        info!("📁 Created shared directory");
    }
    
    let file_path = format!("{}/prices.json", shared_dir);
    
    info!("🚀 Starting Price Service Task...");
    info!("📊 Update interval: {} seconds", update_interval);
    info!("📁 Prices file: {}", file_path);
    
    // Print configured cryptos
    let feeds = get_feed_ids();
    info!("🪙 Tracking cryptos: {}", feeds.keys().cloned().collect::<Vec<_>>().join(", "));
    
    let mut consecutive_failures = 0;
    const MAX_CONSECUTIVE_FAILURES: u32 = 5;
    
    loop {
        let loop_start = std::time::Instant::now();
        
        match fetch_all_prices().await {
            Ok(prices) => {
                consecutive_failures = 0; // Reset failure counter on success
                
                // Store in JSON file (for backward compatibility)
                if let Err(e) = write_prices_to_file(&prices, &file_path).await {
                    error!("Failed to write prices to JSON: {}", e);
                }
                
                // Store in SQLite database using shared pool
                for (crypto, price_data) in &prices.prices {
                    if let Err(e) = database.save_price(crypto, price_data.price) {
                        error!("Failed to store {} price in database: {}", crypto, e);
                    }
                }
            }
            Err(e) => {
                consecutive_failures += 1;
                error!("❌ Failed to fetch prices (failure {}/{}): {}", 
                        consecutive_failures, MAX_CONSECUTIVE_FAILURES, e);
                
                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                    warn!("⚠️ Too many consecutive failures. Entering recovery mode for 60 seconds...");
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
            sleep(sleep_time).await;
        } else {
            warn!("⚠️ Update took longer than interval: {:?} > {:?}", loop_duration, target_interval);
            // Still sleep for a minimum time to prevent tight loops
            sleep(Duration::from_secs(1)).await;
        }
    }
} 