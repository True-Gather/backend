use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::models::{
    CreateRoomRequest, CreateRoomResponse, IceServer, JoinRequest, JoinResponse, PublisherInfo,
    Room,
};
use crate::state::AppState;

/// Room routes
pub fn room_routes() -> Router<AppState> {
    Router::new()
        .route("/", post(create_room))
        .route("/{room_id}", get(get_room))
        .route("/{room_id}/join", post(join_room))
        .route("/{room_id}/leave", post(leave_room))
}

/// POST /api/v1/rooms - Create a new room
async fn create_room(
    State(state): State<AppState>,
    Json(request): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>> {
    // Validate request
    if request.name.is_empty() {
        return Err(AppError::BadRequest("Room name is required".to_string()));
    }

    if request.name.len() > 100 {
        return Err(AppError::BadRequest(
            "Room name must be at most 100 characters".to_string(),
        ));
    }

    // Create room
    let room = Room::new(
        request.name,
        request
            .max_publishers
            .min(state.config.max_publishers_per_room),
        if request.ttl_seconds > 0 {
            request.ttl_seconds
        } else {
            state.config.room_ttl_seconds
        },
    );

    // Save to Redis
    state.room_repo.create_room(&room).await?;

    tracing::info!(room_id = %room.room_id, name = %room.name, "Room created");

    Ok(Json(CreateRoomResponse::from(room)))
}

/// GET /api/v1/rooms/:room_id - Get room information
async fn get_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<crate::models::RoomInfo>> {
    // Validate UUID format
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    let room_info = state
        .room_repo
        .get_room_info(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    Ok(Json(room_info))
}

/// POST /api/v1/rooms/:room_id/join - Join a room
async fn join_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<JoinRequest>,
) -> Result<Json<JoinResponse>> {
    // Validate UUID format
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    // Validate display name
    if request.display.is_empty() {
        return Err(AppError::BadRequest("Display name is required".to_string()));
    }

    if request.display.len() > 100 {
        return Err(AppError::BadRequest(
            "Display name must be at most 100 characters".to_string(),
        ));
    }

    // Check room exists
    let room = state
        .room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    // Check room capacity
    let member_count = state.room_repo.get_member_count(&room_id).await?;
    if member_count >= room.max_publishers as usize {
        return Err(AppError::RoomFull);
    }

    // Generate user ID
    let user_id = Uuid::new_v4().to_string();

    // Generate JWT token
    let token = state
        .auth
        .generate_token(&user_id, &room_id, &request.display)?;

    // Add user to room members
    state.room_repo.add_member(&room_id, &user_id).await?;

    // Build WebSocket URL
    let ws_url = format!(
        "ws://{}:{}/ws?room_id={}&token={}",
        state.config.server_host, state.config.server_port, room_id, token
    );

    // Build ICE servers list
    let mut ice_servers = vec![IceServer {
        urls: vec![state.config.stun_server.clone()],
        username: None,
        credential: None,
    }];

    // Add TURN server if configured
    if let Some(turn_server) = &state.config.turn_server {
        ice_servers.push(IceServer {
            urls: vec![turn_server.clone()],
            username: state.config.turn_username.clone(),
            credential: state.config.turn_credential.clone(),
        });
    }

    tracing::info!(
        room_id = %room_id,
        user_id = %user_id,
        display = %request.display,
        "User joined room"
    );

    Ok(Json(JoinResponse {
        room_id,
        user_id,
        ws_url,
        token,
        ice_servers,
        expires_in: state.config.jwt_expiry_seconds,
    }))
}

/// POST /api/v1/rooms/:room_id/leave - Leave a room
async fn leave_room(
    State(_state): State<AppState>,
    Path(room_id): Path<String>,
    // TODO: Extract user_id from JWT auth header
) -> Result<Json<serde_json::Value>> {
    // Validate UUID format
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    // For now, we'll handle leave through WebSocket
    // This endpoint is for explicit HTTP leave if needed

    Ok(Json(serde_json::json!({ "success": true })))
}

/// Create a publisher info entry
pub fn create_publisher_info(user_id: &str, feed_id: &str, display: &str) -> PublisherInfo {
    PublisherInfo {
        feed_id: feed_id.to_string(),
        user_id: user_id.to_string(),
        display: display.to_string(),
        joined_at: chrono::Utc::now(),
    }
}
