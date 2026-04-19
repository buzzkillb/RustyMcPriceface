#[cfg(test)]
mod tests {
    use crate::PriceDatabase;

    fn get_test_database_url() -> String {
        std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5432/pricebot_test".to_string()
        })
    }

    #[tokio::test]
    #[ignore]
    async fn test_database_initializes() {
        let db_url = get_test_database_url();
        let db = PriceDatabase::new(&db_url)
            .await
            .expect("Failed to create database");
        let result = db.get_all_latest_prices().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn test_save_and_retrieve_price() {
        let db_url = get_test_database_url();
        let db = PriceDatabase::new(&db_url)
            .await
            .expect("Failed to create database");

        let result = db.save_price("BTC", 50000.0).await;
        assert!(result.is_ok());

        let price = db.get_latest_price("BTC").await;
        assert!(price.is_ok());
        assert_eq!(price.unwrap(), 50000.0);
    }

    #[tokio::test]
    #[ignore]
    async fn test_save_invalid_price_rejected() {
        let db_url = get_test_database_url();
        let db = PriceDatabase::new(&db_url)
            .await
            .expect("Failed to create database");

        let result = db.save_price("BTC", 0.0).await;
        assert!(result.is_ok());

        let result = db.save_price("BTC", -100.0).await;
        assert!(result.is_ok());
    }
}
