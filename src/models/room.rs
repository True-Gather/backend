use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Room metadata stored in Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub room_id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub max_publishers: u32,
    pub ttl_seconds: u64,
}

impl Room {
    pub fn new(name: String, max_publishers: u32, ttl_seconds: u64) -> Self {
        Self {
            room_id: uuid::Uuid::new_v4().to_string(),
            name,
            created_at: Utc::now(),
            max_publishers,
            ttl_seconds,
        }
    }
}

/// Room information returned to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub name: String,
    pub participants: Vec<String>,
    pub publishers: Vec<PublisherInfo>,
    pub status: RoomStatus,
    pub participants_count: usize,
    pub created_at: DateTime<Utc>,
}

/// Publisher information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublisherInfo {
    pub feed_id: String,
    pub user_id: String,
    pub display: String,
    pub joined_at: DateTime<Utc>,
}

/// Room status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RoomStatus {
    Active,
    Inactive,
}

/// Request to create a room (client -> backend)
#[derive(Debug, Deserialize)]
pub struct CreateRoomRequest {
    pub name: String,
    #[serde(default = "default_max_publishers")]
    pub max_publishers: u32,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
}

fn default_max_publishers() -> u32 {
    50
}

fn default_ttl() -> u64 {
    7200
}

/// Response after creating a room (backend -> client)
#[derive(Debug, Serialize)]
pub struct CreateRoomResponse {
    pub room_id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub max_publishers: u32,
    pub ttl_seconds: u64,
    pub creator_key: String,
}

impl From<Room> for CreateRoomResponse {
    fn from(room: Room) -> Self {
        Self {
            room_id: room.room_id,
            name: room.name,
            created_at: room.created_at,
            max_publishers: room.max_publishers,
            ttl_seconds: room.ttl_seconds,
            creator_key: String::new(), // filled by handler
        }
    }
}

/// Invitation for a room (stored in Redis)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInvitation {
    pub token: String,
    pub room_id: String,
    pub created_by: String,

    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,

    pub max_uses: Option<u32>,
    pub used_count: u32,

    pub email: Option<String>,

    // ✅ Backward-compatible fields:
    // Old invites in Redis may not have these fields.
    // With default, deserialization won't crash.
    #[serde(default)]
    pub code_salt: String,

    #[serde(default)]
    pub code_hash: String,
}

impl RoomInvitation {
    /// Create a new invitation.
    pub fn new(
        room_id: String,
        created_by: String,
        ttl_seconds: u64,
        max_uses: Option<u32>,
        email: Option<String>,
        code_salt: String,
        code_hash: String,
    ) -> Self {
        let now = Utc::now();
        let expires_at = now + Duration::seconds(ttl_seconds as i64);

        Self {
            token: Uuid::new_v4().to_string().replace('-', ""), // url-friendly token
            room_id,
            created_by,
            created_at: now,
            expires_at,
            max_uses,
            used_count: 0,
            email,
            code_salt,
            code_hash,
        }
    }

    /// Check if invitation can still be used.
    pub fn is_valid(&self) -> bool {
        // Expired
        if Utc::now() > self.expires_at {
            return false;
        }

        // Max uses reached
        if let Some(max) = self.max_uses {
            if self.used_count >= max {
                return false;
            }
        }

        // Missing security fields => unusable invite (old/corrupted)
        if self.code_salt.trim().is_empty() || self.code_hash.trim().is_empty() {
            return false;
        }

        true
    }
}

/// Request to create an invitation (manual)
#[derive(Debug, Deserialize)]
pub struct CreateInvitationRequest {
    /// TTL in seconds (default: 24 hours)
    #[serde(default = "default_invitation_ttl")]
    pub ttl_seconds: u64,
    /// Maximum number of uses (None = unlimited)
    pub max_uses: Option<u32>,
}

fn default_invitation_ttl() -> u64 {
    86400 // 24 hours
}

/// Response after creating an invitation
#[derive(Debug, Serialize)]
pub struct CreateInvitationResponse {
    pub token: String,
    pub room_id: String,
    pub expires_at: DateTime<Utc>,
    pub max_uses: Option<u32>,
    pub invite_url: String,
    pub invite_code: String,
}

/// Response when validating an invitation
#[derive(Debug, Serialize)]
pub struct InvitationInfo {
    pub token: String,
    pub room_id: String,
    pub room_name: String,
    pub expires_at: DateTime<Utc>,
    pub is_valid: bool,
}

/// Request to send invitation emails
#[derive(Debug, Deserialize)]
pub struct InviteEmailRequest {
    pub emails: Vec<String>,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
    #[serde(default)]
    pub max_uses: Option<u32>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct InviteEmailInvite {
    pub email: String,
    pub token: String,
    pub invite_url: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Response after sending invitation emails
#[derive(Debug, Serialize)]
pub struct InviteEmailResponse {
    pub sent: u32,
    pub room_id: String,
    pub invites: Vec<InviteEmailInvite>,
}
