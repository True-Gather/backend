use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::models::{
    CreateInvitationRequest, CreateInvitationResponse, CreateRoomRequest, CreateRoomResponse,
    IceServer, InvitationInfo, JoinRequest, JoinResponse, PublisherInfo, Room, RoomInvitation,
    InviteEmailRequest, InviteEmailResponse,
};
use crate::state::AppState;

/// Room routes
pub fn room_routes() -> Router<AppState> {
    Router::new()
        .route("/", post(create_room))
        .route("/{room_id}", get(get_room))
        .route("/{room_id}/join", post(join_room))
        .route("/{room_id}/leave", post(leave_room))
        .route("/{room_id}/media_status", get(get_media_status))
        .route("/{room_id}/invite", post(create_invitation))
        .route("/{room_id}/invites", get(list_invitations))
        .route("/{room_id}/invite-email", post(send_invite_email))
        .route("/invite/{token}", get(get_invitation))
        .route("/invite/{token}/use", post(use_invitation))
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

    // Persist member display info so later joins can fetch full participant list
    state
        .room_repo
        .set_member_info(&room_id, &user_id, &request.display)
        .await?;

    // Build WebSocket URL — advertise frontend host when available, fall back to localhost instead of 0.0.0.0
    let advertised_host = state
        .config
        .frontend_host
        .clone()
        .unwrap_or_else(|| {
            if state.config.server_host == "0.0.0.0" {
                "localhost".to_string()
            } else {
                state.config.server_host.clone()
            }
        });

    let ws_url = format!(
        "ws://{}:{}/ws?room_id={}&token={}",
        advertised_host, state.config.server_port, room_id, token
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

    // Fetch current participants (from persisted member infos)
    let participants = state.room_repo.get_member_infos(&room_id).await?;

    Ok(Json(JoinResponse {
        room_id,
        user_id,
        ws_url,
        token,
        ice_servers,
        expires_in: state.config.jwt_expiry_seconds,
        participants,
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

/// POST /api/v1/rooms/:room_id/invite - Create an invitation link
async fn create_invitation(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<CreateInvitationRequest>,
) -> Result<Json<CreateInvitationResponse>> {
    // Validate UUID format
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    // Check room exists
    let _room = state
        .room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    // Create invitation
    let invitation = RoomInvitation::new(
        room_id.clone(),
        "system".to_string(), // TODO: Get from auth when implemented
        request.ttl_seconds,
        request.max_uses,
    );

    // Save to Redis
    state.room_repo.create_invitation(&invitation).await?;

    // Build invite URL
    let invite_url = format!(
        "http://{}:{}/invite/{}",
        state.config.frontend_host.as_deref().unwrap_or("localhost"),
        state.config.frontend_port.unwrap_or(3000),
        invitation.token
    );

    tracing::info!(
        room_id = %room_id,
        token = %invitation.token,
        "Invitation created"
    );

    Ok(Json(CreateInvitationResponse {
        token: invitation.token,
        room_id,
        expires_at: invitation.expires_at,
        max_uses: invitation.max_uses,
        invite_url,
    }))
}

/// GET /api/v1/rooms/:room_id/media_status - Return current publishers and subscribers for debugging
async fn get_media_status(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    // Validate UUID format
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    let publishers = state.media_gateway.list_publishers(&room_id).await;
    let subscribers = state.media_gateway.list_subscribers(&room_id).await;

    Ok(Json(serde_json::json!({
        "room_id": room_id,
        "publishers": publishers,
        "subscribers": subscribers
    })))
}

/// GET /api/v1/rooms/:room_id/invites - List all invitations for a room
async fn list_invitations(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<Vec<RoomInvitation>>> {
    // Validate UUID format
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    // Check room exists
    let _room = state
        .room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    let invitations = state.room_repo.get_room_invitations(&room_id).await?;

    Ok(Json(invitations))
}

/// GET /api/v1/rooms/invite/:token - Get invitation info
async fn get_invitation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<InvitationInfo>> {
    let invitation = state
        .room_repo
        .get_invitation(&token)
        .await?
        .ok_or_else(|| AppError::NotFound("Invitation not found or expired".to_string()))?;

    // Check validity before moving values
    let is_valid = invitation.is_valid();

    // Get room info
    let room = state
        .room_repo
        .get_room(&invitation.room_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Room no longer exists".to_string()))?;

    Ok(Json(InvitationInfo {
        token: invitation.token,
        room_id: invitation.room_id,
        room_name: room.name,
        expires_at: invitation.expires_at,
        is_valid,
    }))
}

/// POST /api/v1/rooms/invite/:token/use - Use an invitation
async fn use_invitation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<InvitationInfo>> {
    let invitation = state
        .room_repo
        .get_invitation(&token)
        .await?
        .ok_or_else(|| AppError::NotFound("Invitation not found or expired".to_string()))?;

    if !invitation.is_valid() {
        return Err(AppError::BadRequest(
            "Invitation is expired or has reached maximum uses".to_string(),
        ));
    }

    // Get room info
    let room = state
        .room_repo
        .get_room(&invitation.room_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Room no longer exists".to_string()))?;

    // Increment use count
    state.room_repo.use_invitation(&token).await?;

    tracing::info!(token = %token, room_id = %invitation.room_id, "Invitation used");

    Ok(Json(InvitationInfo {
        token: invitation.token,
        room_id: invitation.room_id,
        room_name: room.name,
        expires_at: invitation.expires_at,
        is_valid: true,
    }))
}

/// POST /api/v1/rooms/{room_id}/invite-email
/// Sends an invitation email via configured mail provider (Resend).
async fn send_invite_email(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<InviteEmailRequest>,
) -> Result<Json<InviteEmailResponse>> {
    // Validate UUID format (same behavior as other endpoints)
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    // Ensure room exists
    let room = state
        .room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Room not found".to_string()))?;

    // Create an invitation token (reusing existing model constructor)
    let ttl_seconds = request.ttl_seconds.unwrap_or(86400);
    let invitation = RoomInvitation::new(
        room_id.clone(),
        "system".to_string(), // TODO: Replace with real user_id when auth is enabled
        ttl_seconds,
        request.max_uses,
    );

    state.room_repo.create_invitation(&invitation).await?;

    // Compute invite URL (consistent with create_invitation)
    let invite_url = format!(
        "http://{}:{}/invite/{}",
        state.config.frontend_host.as_deref().unwrap_or("localhost"),
        state.config.frontend_port.unwrap_or(3000),
        invitation.token
    );

    let subject = request
        .subject
        .clone()
        .unwrap_or_else(|| format!("TrueGather invite — {}", room.name));

    let mut text = String::new();
    if let Some(msg) = &request.message {
        if !msg.trim().is_empty() {
            text.push_str(msg.trim());
            text.push_str("\n\n");
        }
    }

    text.push_str(&format!(
        "You are invited to join a TrueGather meeting.\n\nMeeting code:\n{}\n\nInvite link:\n{}\n",
        room_id, invite_url
    ));

    // Send email
    state
        .mailer
        .send_invite(request.emails.clone(), subject, text)
        .await?;

    Ok(Json(InviteEmailResponse {
        sent: request.emails.len() as u32,
        token: invitation.token,
        invite_url,
        room_id,
    }))
}
