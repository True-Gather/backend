use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use sha2::{Digest, Sha256};
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
        .route("/", get(list_rooms).post(create_room))
        .route("/{room_id}", get(get_room))
        .route("/{room_id}/join", post(join_room))
        .route("/{room_id}/leave", post(leave_room))
        .route("/{room_id}/invite", post(create_invitation))
        .route("/{room_id}/invites", get(list_invitations))
        .route("/{room_id}/invite-email", post(send_invite_email))
        .route("/invite/{token}", get(get_invitation))
        .route("/invite/{token}/use", post(use_invitation))
}

/// Hash helper (peppered) for invite codes + creator keys
fn hash_code(pepper: &str, code: &str) -> String {
    let mut h = Sha256::new();
    h.update(pepper.as_bytes());
    h.update(b":");
    h.update(code.as_bytes());
    hex::encode(h.finalize())
}

/// Output is always "NNN-NNN" (if 6 digits), otherwise trimmed raw.
fn normalize_invite_code(input: &str) -> String {
    let trimmed = input.trim();

    // keep only digits
    let digits: String = trimmed.chars().filter(|c| c.is_ascii_digit()).collect();

    if digits.len() == 6 {
        format!("{}-{}", &digits[0..3], &digits[3..6])
    } else {
        // fallback: keep a simple trimmed form
        trimmed.to_string()
    }
}

/// Generates host-only creator key (stored locally on creator device)
fn gen_creator_key() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..32)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// 6 digits, displayed like 761-221
fn gen_invite_code() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let a: u16 = rng.random_range(0..1000);
    let b: u16 = rng.random_range(0..1000);
    format!("{:03}-{:03}", a, b)
}

/// POST /api/v1/rooms - Create a new room
async fn create_room(
    State(state): State<AppState>,
    Json(request): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>> {
    if request.name.is_empty() {
        return Err(AppError::BadRequest("Room name is required".to_string()));
    }
    if request.name.len() > 100 {
        return Err(AppError::BadRequest(
            "Room name must be at most 100 characters".to_string(),
        ));
    }

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

    // creator_key (host-only), returned once
    let creator_key = gen_creator_key();
    let creator_hash = hash_code(&state.config.invite_code_salt, creator_key.trim());

    state.room_repo.create_room(&room).await?;
    state
        .room_repo
        .set_creator_key_hash(&room.room_id, &creator_hash, room.ttl_seconds)
        .await?;

    tracing::info!(room_id = %room.room_id, name = %room.name, "Room created");

    Ok(Json(CreateRoomResponse {
        room_id: room.room_id,
        name: room.name,
        created_at: room.created_at,
        max_publishers: room.max_publishers,
        ttl_seconds: room.ttl_seconds,
        creator_key,
    }))
}

#[derive(serde::Deserialize)]
struct ListRoomsQuery {
    limit: Option<usize>,
}

/// GET /api/v1/rooms - List recent rooms
async fn list_rooms(
    State(state): State<AppState>,
    Query(query): Query<ListRoomsQuery>,
) -> Result<Json<Vec<crate::models::RoomInfo>>> {
    let limit = query.limit.unwrap_or(20).min(100);
    let rooms = state.room_repo.list_rooms(limit).await?;
    Ok(Json(rooms))
}

/// GET /api/v1/rooms/:room_id - Get room information
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

/// POST /api/v1/rooms/:room_id/join - Option B join:
/// - Host: creator_key
/// - Guest: invite_token + invite_code
async fn join_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(request): Json<JoinRequest>,
) -> Result<Json<JoinResponse>> {
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    let display = request.display.trim();
    if display.is_empty() {
        return Err(AppError::BadRequest("Display name is required".to_string()));
    }
    if display.len() > 100 {
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

    // Capacity check
    let member_count = state.room_repo.get_member_count(&room_id).await?;
    if member_count >= room.max_publishers as usize {
        return Err(AppError::RoomFull);
    }

    // 1) Host flow (creator key)
    if let Some(creator_key) = request
        .creator_key
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let expected = state
            .room_repo
            .get_creator_key_hash(&room_id)
            .await?
            .ok_or_else(|| AppError::BadRequest("Access denied".to_string()))?;

        let got = hash_code(&state.config.invite_code_salt, creator_key);
        if got != expected {
            return Err(AppError::BadRequest("Invalid creator key".to_string()));
        }

        // host join: no consume
    } else {
        // 2) Guest flow: invite_token + invite_code
        let invite_token = request
            .invite_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::BadRequest("Invite token is required".to_string()))?;

        let invite_code_raw = request
            .invite_code
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| AppError::BadRequest("Invitation code is required".to_string()))?;

        let invitation = state
            .room_repo
            .get_invitation(invite_token)
            .await?
            .ok_or_else(|| AppError::NotFound("Invitation not found or expired".to_string()))?;

        if invitation.room_id != room_id {
            return Err(AppError::BadRequest(
                "Invitation does not match this room".to_string(),
            ));
        }
        if !invitation.is_valid() {
            return Err(AppError::BadRequest(
                "Invitation is expired or has reached maximum uses".to_string(),
            ));
        }

        // Normalize user input, then hash normalized form
        let normalized = normalize_invite_code(invite_code_raw);
        let got = hash_code(&state.config.invite_code_salt, &normalized);

        if got != invitation.code_hash {
            return Err(AppError::BadRequest("Invalid invitation code".to_string()));
        }

        // Consume only after verification
        let ok = state.room_repo.use_invitation(invite_token).await?;
        if !ok {
            return Err(AppError::BadRequest(
                "Invitation is expired or has reached maximum uses".to_string(),
            ));
        }
    }

    // Generate user id + JWT
    let user_id = Uuid::new_v4().to_string();
    let token = state.auth.generate_token(&user_id, &room_id, display)?;

    state.room_repo.add_member(&room_id, &user_id).await?;

    let ws_url = format!(
        "ws://{}:{}/ws?room_id={}&token={}",
        state.config.server_host, state.config.server_port, room_id, token
    );

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

    Ok(Json(JoinResponse {
        room_id,
        user_id,
        ws_url,
        token,
        ice_servers,
        expires_in: state.config.jwt_expiry_seconds,
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

/// Create a publisher info entry
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

    state
        .room_repo
        .get_room(&room_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Room {} not found", room_id)))?;

    // Generate code + store normalized hash (important!)
    let code = gen_invite_code();
    let normalized = normalize_invite_code(&code);
    let code_hash = hash_code(&state.config.invite_code_salt, &normalized);

    let invitation = RoomInvitation::new_with_code_hash(
        room_id.clone(),
        "system".to_string(),
        request.ttl_seconds,
        request.max_uses,
        None,
        code_hash,
    );

    state.room_repo.create_invitation(&invitation).await?;

    let invite_url = format!(
        "{}/invite/{}",
        state.config
            .frontend_host
            .as_deref()
            .unwrap_or("http://localhost:3000"),
        invitation.token
    );

    Ok(Json(CreateInvitationResponse {
        token: invitation.token,
        room_id,
        expires_at: invitation.expires_at,
        max_uses: invitation.max_uses,
        invite_url,
    }))
}

/// GET /api/v1/rooms/:room_id/invites
async fn list_invitations(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<Vec<RoomInvitation>>> {
    Uuid::parse_str(&room_id)
        .map_err(|_| AppError::BadRequest("Invalid room ID format".to_string()))?;

    state
        .room_repo
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
/// sends invite link + code and stores hash in Redis
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

    // generate code + store normalized hash
    let code = gen_invite_code();
    let normalized = normalize_invite_code(&code);
    let code_hash = hash_code(&state.config.invite_code_salt, &normalized);

    let invitation = RoomInvitation::new_with_code_hash(
        room_id.clone(),
        "system".to_string(),
        ttl_seconds,
        request.max_uses,
        None,
        code_hash,
    );

    state.room_repo.create_invitation(&invitation).await?;

    let invite_url = format!(
        "{}/room/{}/lobby?token={}",
        state.config
            .frontend_host
            .as_deref()
            .unwrap_or("http://localhost:3000"),
        room_id,
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
        "You are invited to join a TrueGather meeting.\n\nMeeting:\n{}\n\nInvite link (token):\n{}\n\nInvitation code:\n{}\n",
        room.name, invite_url, code
    ));

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
