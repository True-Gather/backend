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
