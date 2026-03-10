use crate::errors::{BotError, BotResult};
use std::time::{SystemTime, UNIX_EPOCH};

/// Validate a cryptocurrency name
pub fn validate_crypto_name(name: &str) -> BotResult<()> {
    if name.is_empty() {
        return Err(BotError::InvalidInput("Crypto name cannot be empty".into()));
    }

    if name.len() > 10 {
        return Err(BotError::InvalidInput(
            "Crypto name too long (max 10 chars)".into(),
        ));
    }

    if !name.chars().all(|c| c.is_alphanumeric()) {
        return Err(BotError::InvalidInput(
            "Crypto name must be alphanumeric".into(),
        ));
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
        "EURO" => "💶",
        "SHANGHAI" => "🇨🇳",
        "SHANGHAISILVER" => "🇨🇳",
        "DXY" => "🇺🇸",
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
        return Err(BotError::InvalidInput(
            "Previous price cannot be zero".into(),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_crypto_name_valid() {
        assert!(validate_crypto_name("BTC").is_ok());
        assert!(validate_crypto_name("SOL").is_ok());
        assert!(validate_crypto_name("ETH").is_ok());
        assert!(validate_crypto_name("1234567890").is_ok()); // 10 chars
    }

    #[test]
    fn test_validate_crypto_name_invalid() {
        assert!(validate_crypto_name("").is_err());
        assert!(validate_crypto_name("12345678901").is_err()); // 11 chars
        assert!(validate_crypto_name("BTC!").is_err());
        assert!(validate_crypto_name("BTC-USDT").is_err());
    }

    #[test]
    fn test_validate_price_valid() {
        assert!(validate_price(100.0).is_ok());
        assert!(validate_price(0.01).is_ok());
        assert!(validate_price(1000000.0).is_ok());
    }

    #[test]
    fn test_validate_price_invalid() {
        assert!(validate_price(-10.0).is_err());
        assert!(validate_price(f64::NAN).is_err());
        assert!(validate_price(f64::INFINITY).is_err());
    }

    #[test]
    fn test_format_price() {
        assert_eq!(format_price(50000.0), "$50000");
        assert_eq!(format_price(500.0), "$500.00");
        assert_eq!(format_price(5.0), "$5.000");
        assert_eq!(format_price(0.5), "$0.5000");
    }

    #[test]
    fn test_calculate_percentage_change() {
        assert!((calculate_percentage_change(110.0, 100.0).unwrap() - 10.0).abs() < 0.01);
        assert!((calculate_percentage_change(90.0, 100.0).unwrap() - (-10.0)).abs() < 0.01);
        assert!(calculate_percentage_change(100.0, 0.0).is_err()); // divide by zero
    }

    #[test]
    fn test_get_change_arrow() {
        assert_eq!(get_change_arrow(5.0), "📈");
        assert_eq!(get_change_arrow(-5.0), "📉");
        assert_eq!(get_change_arrow(0.0), "➡️");
    }

    #[test]
    fn test_get_crypto_emoji() {
        assert_eq!(get_crypto_emoji("BTC"), "🪙");
        assert_eq!(get_crypto_emoji("ETH"), "🪙");
        assert_eq!(get_crypto_emoji("DOGE"), "🐕");
        assert_eq!(get_crypto_emoji("DXY"), "🇺🇸");
        assert_eq!(get_crypto_emoji("UNKNOWN"), "🪙");
    }
}
