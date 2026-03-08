use crate::database::PriceDatabase;
use crate::price_service::{fetch_shanghai_silver_price, PriceData, PricesFile};
use chrono::Utc;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

const UPDATE_INTERVAL_SECONDS: u64 = 1800; // 30 minutes
const CRYPTO_NAME: &str = "SHANGHAISILVER";
const FILE_PATH: &str = "shared/prices.json";

/// Check if Shanghai Gold Exchange is currently in trading hours
/// SGE Trading Hours in UTC:
/// - Day Session: 01:00 - 07:30 UTC
/// - Night Session: 12:00 - 18:30 UTC
fn is_sge_market_open() -> bool {
    let now = Utc::now();
    let timestamp = now.timestamp();
    let hour = ((timestamp / 3600) % 24) as u32;
    let minute = ((timestamp / 60) % 60) as u32;
    let time = hour * 60 + minute; // minutes since midnight UTC
    
    // Day session: 01:00-07:30 UTC (60-450)
    let day_session = time >= 60 && time <= 450;
    
    // Night session: 12:00-18:30 UTC (720-1110)
    let night_session = time >= 720 && time <= 1110;
    
    day_session || night_session
}

pub async fn run(database: Arc<PriceDatabase>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("🚀 Starting Shanghai Silver Price Service...");
    info!("📊 Update interval: {} seconds (30 minutes)", UPDATE_INTERVAL_SECONDS);
    info!("🕐 SGE Trading Hours - Day: 01:00-07:30 UTC, Night: 12:00-18:30 UTC");

    let shared_dir = "shared";
    if !Path::new(shared_dir).exists() {
        fs::create_dir(shared_dir)?;
        info!("📁 Created shared directory");
    }

    loop {
        let loop_start = std::time::Instant::now();

        if is_sge_market_open() {
            info!("🟢 SGE Market is OPEN - fetching price...");
            match fetch_shanghai_silver_price().await {
                Ok(price_data) => {
                    info!(
                        "✅ Shanghai Silver: ${:.2} (Premium: ${:.2}, {:.2}%)",
                        price_data.price,
                        price_data.premium.unwrap_or(0.0),
                        price_data.premium_percent.unwrap_or(0.0)
                    );

                    if let Err(e) = database.save_price(CRYPTO_NAME, price_data.price) {
                        error!("❌ Failed to save Shanghai Silver price to database: {}", e);
                    } else {
                        info!("💾 Saved Shanghai Silver price to database");
                    }

                    if let Err(e) = update_prices_json(&price_data) {
                        error!("❌ Failed to update prices.json: {}", e);
                    } else {
                        info!("📝 Updated prices.json with Shanghai Silver price");
                    }
                }
                Err(e) => {
                    error!("❌ Failed to fetch Shanghai Silver price: {}", e);
                }
            }
        } else {
            let now = Utc::now();
            info!("🔴 SGE Market is CLOSED - skipping fetch at {} UTC", now.format("%H:%M"));
            info!("💾 Using last known price from database");
        }

        let loop_duration = loop_start.elapsed();
        let target_interval = Duration::from_secs(UPDATE_INTERVAL_SECONDS);

        if loop_duration < target_interval {
            let sleep_time = target_interval - loop_duration;
            info!("😴 Sleeping for {:?}", sleep_time);
            sleep(sleep_time).await;
        } else {
            warn!("⚠️ Update took longer than interval: {:?} > {:?}", loop_duration, target_interval);
            sleep(Duration::from_secs(1)).await;
        }
    }
}

fn update_prices_json(price_data: &PriceData) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut prices = HashMap::new();

    if Path::new(FILE_PATH).exists() {
        let content = fs::read_to_string(FILE_PATH)?;
        if let Ok(existing) = serde_json::from_str::<PricesFile>(&content) {
            prices = existing.prices;
        }
    }

    prices.insert(CRYPTO_NAME.to_string(), price_data.clone());

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    let prices_file = PricesFile {
        prices,
        timestamp,
    };

    let json_string = serde_json::to_string_pretty(&prices_file)?;
    fs::write(FILE_PATH, json_string)?;

    Ok(())
}
