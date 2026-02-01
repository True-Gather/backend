use serde::{Deserialize, Serialize};

/// Request to join a room
#[derive(Debug, Deserialize)]
pub struct JoinRequest {
    pub display: String,
    #[serde(default)]
    pub access_code: Option<String>,
}

/// Member info sent to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    pub user_id: String,
    pub display: String,
    pub joined_at: i64,
}

/// Response after joining a room
#[derive(Debug, Serialize)]
pub struct JoinResponse {
    pub room_id: String,
    pub user_id: String,
    pub ws_url: String,
    pub token: String,
    pub ice_servers: Vec<IceServer>,
    pub expires_in: u64,
    pub participants: Vec<MemberInfo>,
}

/// ICE server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

/// WebSocket session info stored in Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsSession {
    pub user_id: String,
    pub room_id: String,
    pub display: String,
    pub connected_at: i64,
    pub last_ping: i64,
}

/// JWT Claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user_id
    pub room_id: String,
    pub display: String,
    pub iat: i64,
    pub exp: i64,
}
