use axum::{extract::State, routing::get, Json, Router};
use chrono::Utc;
use serde::Serialize;

use crate::error::Result;
use crate::state::AppState;

/// Health response structure
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub redis: String,
    pub media_gateway: String,
    pub timestamp: String,
}

/// Health routes
pub fn health_routes() -> Router<AppState> {
    Router::new().route("/health", get(health_check))
}

/// GET /health - Health check endpoint
async fn health_check(State(state): State<AppState>) -> Result<Json<HealthResponse>> {
    let redis_status = match state.room_repo.health_check().await {
        Ok(true) => "connected",
        Ok(false) => "error",
        Err(_) => "disconnected",
    };

    let media_gateway_status = if state.media_gateway.is_healthy() {
        "ready"
    } else {
        "not_ready"
    };

    let overall_status = if redis_status == "connected" && media_gateway_status == "ready" {
        "healthy"
    } else {
        "unhealthy"
    };

    Ok(Json(HealthResponse {
        status: overall_status.to_string(),
        redis: redis_status.to_string(),
        media_gateway: media_gateway_status.to_string(),
        timestamp: Utc::now().to_rfc3339(),
    }))
}
