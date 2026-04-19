use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PriceData {
    pub price: f64,
    pub timestamp: u64,
    #[serde(default)]
    pub premium: Option<f64>,
    #[serde(default)]
    pub premium_percent: Option<f64>,
    pub source: Option<String>,
    #[serde(default)]
    pub is_fallback: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PricesFile {
    pub prices: HashMap<String, PriceData>,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct SharedPrices {
    inner: Arc<RwLock<PricesFile>>,
}

impl SharedPrices {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(PricesFile {
                prices: HashMap::new(),
                timestamp: 0,
            })),
        }
    }

    pub async fn write(&self, prices: PricesFile) {
        let mut current = self.inner.write().await;
        *current = prices;
    }

    pub async fn read(&self) -> PricesFile {
        self.inner.read().await.clone()
    }

    pub async fn get_price(&self, crypto: &str) -> Option<f64> {
        let current = self.inner.read().await;
        current.prices.get(crypto).map(|p| p.price)
    }
}

impl Default for SharedPrices {
    fn default() -> Self {
        Self::new()
    }
}
