use crate::errors::{BotError, BotResult};
use std::time::{SystemTime, UNIX_EPOCH};

/// Validate a cryptocurrency name
pub fn validate_crypto_name(name: &str) -> BotResult<()> {
    if name.is_empty() {
        return Err(BotError::InvalidInput("Crypto name cannot be empty".into()));
    }
    
    if name.len() > 10 {
        return Err(BotError::InvalidInput("Crypto name too long (max 10 chars)".into()));
    }
    
    if !name.chars().all(|c| c.is_alphanumeric()) {
        return Err(BotError::InvalidInput("Crypto name must be alphanumeric".into()));
    }
    
    Ok(())
}

/// Get current Unix timestamp safely
pub fn get_current_timestamp() -> BotResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| BotError::SystemTime(e.to_string()))
}

/// Format price based on magnitude
pub fn format_price(price: f64) -> String {
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

/// Get emoji for cryptocurrency
pub fn get_crypto_emoji(crypto: &str) -> &'static str {
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

/// Validate price value
pub fn validate_price(price: f64) -> BotResult<()> {
    if price.is_nan() || price.is_infinite() {
        return Err(BotError::InvalidInput("Invalid price value".into()));
    }
    
    if price < 0.0 {
        return Err(BotError::InvalidInput("Price cannot be negative".into()));
    }
    
    Ok(())
}

/// Calculate percentage change safely
pub fn calculate_percentage_change(current: f64, previous: f64) -> BotResult<f64> {
    validate_price(current)?;
    validate_price(previous)?;
    
    if previous == 0.0 {
        return Err(BotError::InvalidInput("Previous price cannot be zero".into()));
    }
    
    Ok(((current - previous) / previous) * 100.0)
}

/// Get arrow emoji for price change
pub fn get_change_arrow(change_percent: f64) -> &'static str {
    if change_percent > 0.0 {
        "📈"
    } else if change_percent < 0.0 {
        "📉"
    } else {
        "➡️"
    }
} 