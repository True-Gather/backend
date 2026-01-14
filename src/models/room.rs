use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
}

impl From<Room> for CreateRoomResponse {
    fn from(room: Room) -> Self {
        Self {
            room_id: room.room_id,
            name: room.name,
            created_at: room.created_at,
            max_publishers: room.max_publishers,
            ttl_seconds: room.ttl_seconds,
        }
    }
}

/// Room invitation for sharing meeting links
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInvitation {
    pub token: String,
    pub room_id: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub max_uses: Option<u32>,
    pub uses: u32,
}

impl RoomInvitation {
    pub fn new(room_id: String, created_by: String, ttl_seconds: u64, max_uses: Option<u32>) -> Self {
        let now = Utc::now();
        Self {
            token: Self::generate_token(),
            room_id,
            created_by,
            created_at: now,
            expires_at: now + chrono::Duration::seconds(ttl_seconds as i64),
            max_uses,
            uses: 0,
        }
    }

    fn generate_token() -> String {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::rng();
        (0..16)
            .map(|_| {
                let idx = rng.random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

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

/// Request to create an invitation
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
