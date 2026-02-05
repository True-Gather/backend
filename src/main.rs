use std::net::SocketAddr;

use axum::Router;
use tokio::net::TcpListener;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use truegather_backend::api;
use truegather_backend::auth::AuthService;
use truegather_backend::config::Config;
use truegather_backend::mail::Mailer;
use truegather_backend::media::MediaGateway;
use truegather_backend::redis::{create_pool, RoomRepository};
use truegather_backend::state::AppState;
use truegather_backend::ws::ws_routes;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing::info!(
        "JWT_SECRET present? {}",
        std::env::var("JWT_SECRET").is_ok()
    );

    // Initialize logging
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting TrueGather Backend...");

    // Load configuration
    let config = Config::from_env()?;
    tracing::info!(
        host = %config.server_host,
        port = %config.server_port,
        "Configuration loaded"
    );

    // Create Redis connection pool
    let redis_pool = create_pool(&config)?;
    let room_repo = RoomRepository::new(redis_pool);

    // Test Redis connection
    match room_repo.health_check().await {
        Ok(true) => tracing::info!("Redis connection established"),
        Ok(false) => tracing::warn!("Redis health check returned false"),
        Err(e) => {
            tracing::error!(error = %e, "Failed to connect to Redis");
            // Continue anyway, might recover later
        }
    }

    // Create auth service
    let auth = AuthService::new(&config);

    // Create media gateway
    let media_gateway = MediaGateway::new(&config)?;
    tracing::info!("Media gateway initialized");

    // Create application state
    let mailer = Mailer::new_from_env()?;
    let state = AppState::new(config.clone(), auth, room_repo, media_gateway, mailer);

    // Build router
    let app = Router::new()
        .merge(api::create_router(state.clone()))
        .merge(ws_routes().with_state(state))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http());

    // Start server
    let addr: SocketAddr = config.server_addr().parse()?;
    let listener = TcpListener::bind(addr).await?;

    tracing::info!(address = %addr, "Server listening");

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Server shutdown complete");

    Ok(())
}

/// Handle shutdown signals
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, shutting down...");
        },
        _ = terminate => {
            tracing::info!("Received terminate signal, shutting down...");
        },
    }
}
