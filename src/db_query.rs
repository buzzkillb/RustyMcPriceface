use rusqlite::{Connection, Result as SqliteResult};
use std::env;

const DATABASE_PATH: &str = "shared/prices.db";

#[derive(Debug)]
struct PriceRecord {
    crypto_name: String,
    price: f64,
    timestamp: i64,
    created_at: String,
}

fn main() -> SqliteResult<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("Usage: {} <command> [crypto_name] [limit]", args[0]);
        println!("Commands:");
        println!("  stats                    - Show database statistics");
        println!("  latest [crypto]          - Show latest prices for all or specific crypto");
        println!("  history [crypto] [limit] - Show price history (default: 10 records)");
        println!("  cleanup                  - Manually trigger cleanup of old records");
        return Ok(());
    }
    
    let conn = Connection::open(DATABASE_PATH)?;
    let command = &args[1];
    
    match command.as_str() {
        "stats" => show_stats(&conn)?,
        "latest" => {
            let crypto = args.get(2).cloned();
            show_latest(&conn, crypto)?;
        }
        "history" => {
            let crypto = args.get(2).cloned();
            let limit = args.get(3).and_then(|s| s.parse::<i64>().ok()).unwrap_or(10);
            show_history(&conn, crypto, limit)?;
        }
        "cleanup" => cleanup_old_prices(&conn)?,
        _ => {
            println!("Unknown command: {}", command);
            println!("Use 'stats', 'latest', 'history', or 'cleanup'");
        }
    }
    
    Ok(())
}

fn show_stats(conn: &Connection) -> SqliteResult<()> {
    println!("📊 Database Statistics");
    println!("=====================");
    
    // Total records
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM prices", [], |row| row.get(0))?;
    println!("Total records: {}", total);
    
    // Records per crypto
    let mut stmt = conn.prepare("SELECT crypto_name, COUNT(*) FROM prices GROUP BY crypto_name")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    
    println!("\nRecords per crypto:");
    for row in rows {
        let (crypto, count) = row?;
        println!("  {}: {} records", crypto, count);
    }
    
    // Oldest and newest timestamps
    let oldest: i64 = conn.query_row("SELECT MIN(timestamp) FROM prices", [], |row| row.get(0))?;
    let newest: i64 = conn.query_row("SELECT MAX(timestamp) FROM prices", [], |row| row.get(0))?;
    
    if oldest > 0 && newest > 0 {
        let oldest_date = chrono::DateTime::from_timestamp(oldest, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let newest_date = chrono::DateTime::from_timestamp(newest, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        
        println!("\nDate range:");
        println!("  Oldest: {} ({})", oldest_date, oldest);
        println!("  Newest: {} ({})", newest_date, newest);
    }
    
    Ok(())
}

fn show_latest(conn: &Connection, crypto: Option<String>) -> SqliteResult<()> {
    println!("📈 Latest Prices");
    println!("================");
    
    if let Some(crypto_name) = crypto {
        // Latest price for specific crypto
        let mut stmt = conn.prepare(
            "SELECT crypto_name, price, timestamp, created_at FROM prices 
             WHERE crypto_name = ? ORDER BY timestamp DESC LIMIT 1"
        )?;
        
        let rows = stmt.query_map([&crypto_name], |row| {
            Ok(PriceRecord {
                crypto_name: row.get(0)?,
                price: row.get(1)?,
                timestamp: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        
        for row in rows {
            let record = row?;
            let date = chrono::DateTime::from_timestamp(record.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("{}: ${:.6} at {}", record.crypto_name, record.price, date);
        }
    } else {
        // Latest price for each crypto
        let mut stmt = conn.prepare(
            "SELECT p1.crypto_name, p1.price, p1.timestamp, p1.created_at 
             FROM prices p1 
             INNER JOIN (
                 SELECT crypto_name, MAX(timestamp) as max_timestamp 
                 FROM prices GROUP BY crypto_name
             ) p2 ON p1.crypto_name = p2.crypto_name AND p1.timestamp = p2.max_timestamp
             ORDER BY p1.crypto_name"
        )?;
        
        let rows = stmt.query_map([], |row| {
            Ok(PriceRecord {
                crypto_name: row.get(0)?,
                price: row.get(1)?,
                timestamp: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        
        for row in rows {
            let record = row?;
            let date = chrono::DateTime::from_timestamp(record.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("{}: ${:.6} at {}", record.crypto_name, record.price, date);
        }
    }
    
    Ok(())
}

fn show_history(conn: &Connection, crypto: Option<String>, limit: i64) -> SqliteResult<()> {
    if let Some(crypto_name) = crypto {
        println!("📊 Price History for {}", crypto_name);
        println!("{}", "=".repeat(30 + crypto_name.len()));
        
        let mut stmt = conn.prepare(
            "SELECT crypto_name, price, timestamp, created_at FROM prices 
             WHERE crypto_name = ? ORDER BY timestamp DESC LIMIT ?"
        )?;
        
        let rows = stmt.query_map([&crypto_name, &limit.to_string()], |row| {
            Ok(PriceRecord {
                crypto_name: row.get(0)?,
                price: row.get(1)?,
                timestamp: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        
        for row in rows {
            let record = row?;
            let date = chrono::DateTime::from_timestamp(record.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("${:.6} at {}", record.price, date);
        }
    } else {
        println!("📊 Recent Price History (all cryptos)");
        println!("====================================");
        
        let mut stmt = conn.prepare(
            "SELECT crypto_name, price, timestamp, created_at FROM prices 
             ORDER BY timestamp DESC LIMIT ?"
        )?;
        
        let rows = stmt.query_map([&limit.to_string()], |row| {
            Ok(PriceRecord {
                crypto_name: row.get(0)?,
                price: row.get(1)?,
                timestamp: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        
        for row in rows {
            let record = row?;
            let date = chrono::DateTime::from_timestamp(record.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            println!("{}: ${:.6} at {}", record.crypto_name, record.price, date);
        }
    }
    
    Ok(())
}

fn cleanup_old_prices(conn: &Connection) -> SqliteResult<()> {
    // Delete prices older than 30 days (30 * 24 * 60 * 60 = 2592000 seconds)
    let thirty_days_ago = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() - 2592000;
    
    let deleted = conn.execute(
        "DELETE FROM prices WHERE timestamp < ?",
        [&thirty_days_ago.to_string()],
    )?;
    
    println!("🧹 Cleaned up {} old price records (older than 30 days)", deleted);
    Ok(())
} 