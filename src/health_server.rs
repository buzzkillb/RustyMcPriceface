use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{info, error};
use crate::health::HealthState;

/// Start a simple health check HTTP server
pub async fn start_health_server(health_state: Arc<HealthState>, port: u16) {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/", get(health_check))
        .with_state(health_state);

    let addr = format!("0.0.0.0:{}", port);
    
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

async fn health_check(State(health): State<Arc<HealthState>>) -> Result<Json<serde_json::Value>, StatusCode> {
    let health_data = health.to_json();
    let is_healthy = health.is_healthy();
    
    if is_healthy {
        Ok(Json(health_data))
    } else {
        Err(StatusCode::SERVICE_UNAVAILABLE)
    }
}