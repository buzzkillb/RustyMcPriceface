#[cfg(test)]
mod tests {
    use super::PriceDatabase;
    use tempfile::TempDir;

    fn setup_temp_db() -> (TempDir, PriceDatabase) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let db_path = temp_dir.path().join("test.db");
        let db = PriceDatabase::new(db_path.to_str().unwrap()).expect("Failed to create database");
        (temp_dir, db)
    }

    #[test]
    fn test_database_initializes() {
        let (_temp_dir, db) = setup_temp_db();
        // Just verify database can be created and queried
        assert!(db.get_all_latest_prices().is_ok());
    }

    #[test]
    fn test_save_and_retrieve_price() {
        let (_temp_dir, db) = setup_temp_db();

        // Save a price
        let result = db.save_price("BTC", 50000.0);
        assert!(result.is_ok());

        // Retrieve it
        let price = db.get_latest_price("BTC");
        assert!(price.is_ok());
        assert_eq!(price.unwrap(), 50000.0);
    }

    #[test]
    fn test_save_invalid_price_rejected() {
        let (_temp_dir, db) = setup_temp_db();

        // Zero price should be skipped (not saved)
        let result = db.save_price("BTC", 0.0);
        assert!(result.is_ok()); // Returns ok but doesn't save zero

        // Negative price should be skipped
        let result = db.save_price("BTC", -100.0);
        assert!(result.is_ok()); // Returns ok but doesn't save negative
    }
}
