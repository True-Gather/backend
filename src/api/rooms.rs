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
    InviteEmailRequest, InviteEmailResponse, InviteEmailInvite,
};
use crate::security::{
    generate_creator_key, generate_invite_code, generate_salt_hex, hash_secret_sha256_hex,
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
        // ⚠️ Consumes an invite usage — front should NOT call it in normal invite flow
        .route("/invite/{token}/use", post(use_invitation))
}

/// POST /api/v1/rooms - Create a new room
async fn create_room(
    State(state): State<AppState>,
    Json(request): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>> {
    if request.name.is_empty() {
        return Err(AppError::BadRequest("Room name is required".to_string()));
    }

    let room = Room::new(
        request.name,
        request.max_publishers.min(state.config.max_publishers_per_room),
        if request.ttl_seconds > 0 {
            request.ttl_seconds
        } else {
            state.config.room_ttl_seconds
        },
    );

    // 🔑 Host secret (creator_key)
    let creator_key = generate_creator_key();
    let creator_key_hash = hash_secret_sha256_hex(&creator_key, &room.room_id);

    state.room_repo.create_room(&room).await?;
    state.room_repo
        .set_creator_key_hash(&room.room_id, &creator_key_hash, room.ttl_seconds)
        .await?;

    tracing::info!(room_id=%room.room_id, name=%room.name, "Room created");

    let mut resp = CreateRoomResponse::from(room);
    resp.creator_key = creator_key;
    Ok(Json(resp))
}

/// GET /api/v1/rooms/:room_id
async fn get_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<crate::models::RoomInfo>> {
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    let room_info = state
        .room_repo
        .get_room_info(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    Ok(Json(room_info))
}

/// POST /api/v1/rooms/:room_id/join
async fn join_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<JoinRequest>,
) -> Result<Json<JoinResponse>> {
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    if request.display.trim().is_empty() {
        return Err(AppError::BadRequest("Display name is required".to_string()));
    }
    if request.display.len() > 100 {
        return Err(AppError::BadRequest(
            "Display name must be at most 100 characters".to_string(),
        ));
    }

    tracing::info!(
        room_id = %room_id,
        display = %request.display,
        creator_key = ?request.creator_key,
        invite_token = ?request.invite_token,
        "Join payload decoded"
    );

    let room = state
        .room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    // Capacity
    let current_members = state.room_repo.get_member_count(&room_id).await?;
    if current_members >= room.max_publishers as usize {
        return Err(AppError::RoomFull);
    }

    // =========================
    // ✅ AUTH MODE
    // =========================
    // - Host bootstrap: if room empty -> allow first join without token/code
    // - Host: if creator_key provided -> verify it
    // - Guest: if room not empty and no creator_key -> invite_token + invite_code required
    if let Some(creator_key) = request.creator_key.as_deref() {
        let ok = state.room_repo.verify_creator_key(&room_id, creator_key).await?;
        if !ok {
            return Err(AppError::Unauthorized("Invalid creator key".to_string()));
        }
    } else if current_members == 0 {
        tracing::info!(room_id=%room_id, "Host bootstrap: first join allowed (room empty)");
    } else {
        let invite_token = request
            .invite_token
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("Invite token is required".to_string()))?;

        let invite_code = request
            .invite_code
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("Invite code is required".to_string()))?;

        let used = state
            .room_repo
            .verify_and_use_invitation(invite_token, &room_id, invite_code)
            .await?;

        if used.is_none() {
            return Err(AppError::Unauthorized(
                "Invalid or expired invitation (or wrong code)".to_string(),
            ));
        }
    }

    // JWT
    let user_id = Uuid::new_v4().to_string();
    let token = state
        .auth
        .generate_token(&user_id, &room_id, &request.display)?;

    // Persist member
    state.room_repo.add_member(&room_id, &user_id).await?;
    state
        .room_repo
        .set_member_info(&room_id, &user_id, &request.display)
        .await?;

    // WS URL safe
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

    // ICE servers
    let mut ice_servers = vec![IceServer {
        urls: vec![state.config.stun_server.clone()],
        username: None,
        credential: None,
    }];

    if let Some(turn_server) = &state.config.turn_server {
        ice_servers.push(IceServer {
            urls: vec![turn_server.clone()],
            username: state.config.turn_username.clone(),
            credential: state.config.turn_credential.clone(),
        });
    }

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

/// POST /api/v1/rooms/:room_id/leave
async fn leave_room(
    State(_state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    Ok(Json(serde_json::json!({ "success": true })))
}

pub fn create_publisher_info(user_id: &str, feed_id: &str, display: &str) -> PublisherInfo {
    PublisherInfo {
        feed_id: feed_id.to_string(),
        user_id: user_id.to_string(),
        display: display.to_string(),
        joined_at: chrono::Utc::now(),
    }
}

/// POST /api/v1/rooms/:room_id/invite
async fn create_invitation(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<CreateInvitationRequest>,
) -> Result<Json<CreateInvitationResponse>> {
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    state.room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    let ttl_seconds = request.ttl_seconds;

    let invite_code = generate_invite_code();
    let code_salt = generate_salt_hex();
    let code_hash = hash_secret_sha256_hex(&invite_code, &code_salt);

    let invitation = RoomInvitation::new(
        room_id.clone(),
        "system".to_string(),
        ttl_seconds,
        request.max_uses,
        None,
        code_salt,
        code_hash,
    );

    state.room_repo.create_invitation(&invitation).await?;

    let invite_url = format!(
        "http://{}:{}/invite/{}",
        state.config.frontend_host.as_deref().unwrap_or("localhost"),
        state.config.frontend_port.unwrap_or(3000),
        invitation.token
    );

    Ok(Json(CreateInvitationResponse {
        token: invitation.token,
        room_id,
        expires_at: invitation.expires_at,
        max_uses: invitation.max_uses,
        invite_url,
        invite_code,
    }))
}

/// GET /api/v1/rooms/:room_id/media_status
async fn get_media_status(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
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

/// GET /api/v1/rooms/:room_id/invites
async fn list_invitations(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<Vec<RoomInvitation>>> {
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    state.room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    let invitations = state.room_repo.get_room_invitations(&room_id).await?;
    Ok(Json(invitations))
}

/// GET /api/v1/rooms/invite/:token
async fn get_invitation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<InvitationInfo>> {
    let invitation = state
        .room_repo
        .get_invitation(&token)
        .await?
        .ok_or_else(|| AppError::NotFound("Invitation not found or expired".to_string()))?;

    // ✅ IMPORTANT: compute is_valid BEFORE moving fields out of `invitation`
    let is_valid = invitation.is_valid();

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

/// POST /api/v1/rooms/invite/:token/use
/// ⚠️ consumes invite usage
async fn use_invitation(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> Result<Json<InvitationInfo>> {
    let invitation = state
        .room_repo
        .get_invitation(&token)
        .await?
        .ok_or_else(|| AppError::NotFound("Invitation not found or expired".to_string()))?;

    // ✅ compute now (before moving)
    if !invitation.is_valid() {
        return Err(AppError::BadRequest(
            "Invitation is expired or has reached maximum uses".to_string(),
        ));
    }

    let room = state
        .room_repo
        .get_room(&invitation.room_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Room no longer exists".to_string()))?;

    state.room_repo.use_invitation(&token).await?;

    Ok(Json(InvitationInfo {
        token: invitation.token,
        room_id: invitation.room_id,
        room_name: room.name,
        expires_at: invitation.expires_at,
        is_valid: true,
    }))
}

/// POST /api/v1/rooms/{room_id}/invite-email
async fn send_invite_email(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<InviteEmailRequest>,
) -> Result<Json<InviteEmailResponse>> {
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    let room = state
        .room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Room not found".to_string()))?;

    let ttl_seconds = request.ttl_seconds.unwrap_or(86400);
    let subject = request
        .subject
        .clone()
        .unwrap_or_else(|| format!("TrueGather invite — {}", room.name));

    let mut invites = Vec::with_capacity(request.emails.len());

    for email in request.emails.iter() {
        let invite_code = generate_invite_code();
        let code_salt = generate_salt_hex();
        let code_hash = hash_secret_sha256_hex(&invite_code, &code_salt);

        let invitation = RoomInvitation::new(
            room_id.clone(),
            "system".to_string(),
            ttl_seconds,
            Some(1),
            Some(email.clone()),
            code_salt,
            code_hash,
        );

        state.room_repo.create_invitation(&invitation).await?;

        let invite_url = format!(
            "http://{}:{}/invite/{}",
            state.config.frontend_host.as_deref().unwrap_or("localhost"),
            state.config.frontend_port.unwrap_or(3000),
            invitation.token
        );

        let mut text = String::new();
        if let Some(msg) = &request.message {
            if !msg.trim().is_empty() {
                text.push_str(msg.trim());
                text.push_str("\n\n");
            }
        }

        text.push_str("You are invited to join a TrueGather meeting.\n\n");
        text.push_str(&format!("Meeting:\n{}\n\n", room.name));
        text.push_str(&format!("Invite link (contains your token):\n{}\n\n", invite_url));
        text.push_str("Your access code (required):\n");
        text.push_str(&format!("{}\n\n", invite_code));
        text.push_str("You need BOTH the link and the code to join.\n");

        state
            .mailer
            .send_invite(vec![email.clone()], subject.clone(), text)
            .await?;

        invites.push(InviteEmailInvite {
            email: email.clone(),
            token: invitation.token.clone(),
            invite_url,
            expires_at: invitation.expires_at,
        });
    }

    Ok(Json(InviteEmailResponse {
        sent: invites.len() as u32,
        room_id,
        invites,
    }))
}
