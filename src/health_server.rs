use crate::health::HealthAggregator;
use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};

pub type SharedHealth = Arc<HealthAggregator>;

pub async fn start_health_server(health: SharedHealth, port: u16) {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/", get(health_check))
        .route("/test-discord", get(test_discord_connectivity))
        .with_state(health);

    let addr = format!("127.0.0.1:{}", port);

    match TcpListener::bind(&addr).await {
        Ok(listener) => {
            info!("Health check server listening on {}", addr);
            if let Err(e) = axum::serve(listener, app).await {
                error!("Health server error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to bind health server to {}: {}", addr, e);
        }
    }
}

async fn health_check(
    State(health): State<SharedHealth>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let is_healthy = health.is_healthy();

    let response = serde_json::json!({
        "healthy": is_healthy
    });

    if is_healthy {
        Ok(Json(response))
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}

async fn test_discord_connectivity(
    State(_health): State<SharedHealth>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use reqwest::Client;
    use std::time::Duration;

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let test_result = match client
        .get("https://discord.com/api/v10/gateway")
        .header("User-Agent", "Discord-Bot-Health-Check/1.0")
        .send()
        .await
    {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    };

    let response = serde_json::json!({
        "discord_reachable": test_result,
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });

    if test_result {
        Ok(Json(response))
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}
