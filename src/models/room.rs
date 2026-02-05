use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Room persisted in Redis
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublisherInfo {
    pub feed_id: String,
    pub user_id: String,
    pub display: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RoomStatus {
    Active,
    Inactive,
}

/// Request to create a room
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

/// Response after creating a room
#[derive(Debug, Serialize)]
pub struct CreateRoomResponse {
    pub room_id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub max_publishers: u32,
    pub ttl_seconds: u64,

    /// creator_key returned ONLY once (host device)
    pub creator_key: String,
}

/// ✅ Join request for Option B (the only one rooms API uses)
/// - Guest flow: invite_token + invite_code
/// - Host flow: creator_key
#[derive(Debug, Deserialize)]
pub struct JoinRequest {
    /// Display name shown in the room
    pub display: String,

    /// Guest flow (token from link)
    #[serde(default)]
    pub invite_token: Option<String>,

    /// Guest flow (human code typed)
    #[serde(default)]
    pub invite_code: Option<String>,

    /// Host flow (creator key stored on host device)
    #[serde(default)]
    pub creator_key: Option<String>,
}

/// Room invitation stored in Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInvitation {
    pub token: String,
    pub room_id: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub max_uses: Option<u32>,
    pub uses: u32,

    // reserved for future "per-email unique invite"
    pub email: Option<String>,

    /// ✅ hash of the code that guest must type (never store raw code)
    pub code_hash: String,
}

impl RoomInvitation {
    /// Create a new invitation storing the code hash (Option B)
    pub fn new_with_code_hash(
        room_id: String,
        created_by: String,
        ttl_seconds: u64,
        max_uses: Option<u32>,
        email: Option<String>,
        code_hash: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            token: Self::generate_token(),
            room_id,
            created_by,
            created_at: now,
            expires_at: now + chrono::Duration::seconds(ttl_seconds as i64),
            max_uses,
            uses: 0,
            email,
            code_hash,
        }
    }

    fn generate_token() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::rng();
        (0..24)
            .map(|_| {
                let idx = rng.random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    /// Invite is valid if:
    /// - not expired
    /// - max_uses not reached (if max_uses exists)
    pub fn is_valid(&self) -> bool {
        let now = Utc::now();
        if now > self.expires_at {
            return false;
        }
        if let Some(max) = self.max_uses {
            if self.uses >= max {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateInvitationRequest {
    #[serde(default = "default_invitation_ttl")]
    pub ttl_seconds: u64,
    pub max_uses: Option<u32>,
}

fn default_invitation_ttl() -> u64 {
    86400
}

#[derive(Debug, Serialize)]
pub struct CreateInvitationResponse {
    pub token: String,
    pub room_id: String,
    pub expires_at: DateTime<Utc>,
    pub max_uses: Option<u32>,
    pub invite_url: String,
}

#[derive(Debug, Serialize)]
pub struct InvitationInfo {
    pub token: String,
    pub room_id: String,
    pub room_name: String,
    pub expires_at: DateTime<Utc>,
    pub is_valid: bool,
}

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
pub struct InviteEmailResponse {
    pub sent: u32,
    pub token: String,
    pub invite_url: String,
    pub room_id: String,
}
