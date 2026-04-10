use crate::health::HealthAggregator;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tracing::{error, info};

pub type SharedHealth = Arc<HealthAggregator>;

pub async fn start_health_server(
    health: SharedHealth,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/health/all", get(health_check_all))
        .route("/", get(health_check))
        .route("/test-discord", get(test_discord_connectivity))
        .with_state(health);

    let addr = format!("127.0.0.1:{}", port);

    let listener = TcpListener::bind(&addr).await.map_err(|e| {
        error!("Failed to bind health server to {}: {}", addr, e);
        e
    })?;

    info!("Health check server listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check(State(health): State<SharedHealth>) -> Response {
    let health = health.clone();
    let result = timeout(Duration::from_secs(5), async move { health.is_healthy() }).await;

    let is_healthy = result.unwrap_or(false);

    let response = serde_json::json!({
        "healthy": is_healthy
    });

    if is_healthy {
        (StatusCode::OK, Json(response)).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(response)).into_response()
    }
}

struct HealthCheckAllResponse {
    is_all_healthy: bool,
    status: serde_json::Value,
}

impl IntoResponse for HealthCheckAllResponse {
    fn into_response(self) -> Response {
        if self.is_all_healthy {
            (StatusCode::OK, Json(self.status)).into_response()
        } else {
            (StatusCode::SERVICE_UNAVAILABLE, Json(self.status)).into_response()
        }
    }
}

async fn health_check_all(State(health): State<SharedHealth>) -> HealthCheckAllResponse {
    let health = health.clone();
    let result = timeout(Duration::from_secs(5), async move {
        let is_all_healthy = health.is_all_healthy();
        let status = health.to_json();
        (is_all_healthy, status)
    })
    .await;

    match result {
        Ok((is_all_healthy, status)) => HealthCheckAllResponse {
            is_all_healthy,
            status,
        },
        Err(_) => HealthCheckAllResponse {
            is_all_healthy: false,
            status: json!({"error": "health check timeout"}),
        },
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
