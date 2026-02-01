pub mod health;
pub mod rooms;

use axum::Router;

use crate::state::AppState;

/// Create the API router with all routes
pub fn create_router(state: AppState) -> Router {
    Router::new()
        .nest("/api/v1", api_routes())
        .merge(health::health_routes())
        .with_state(state)
}

/// API v1 routes
fn api_routes() -> Router<AppState> {
    Router::new().nest("/rooms", rooms::room_routes())
}
