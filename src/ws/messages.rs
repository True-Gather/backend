use serde::{Deserialize, Serialize};

/// Wrapper for all WebSocket messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalingMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub payload: serde_json::Value,
}

impl SignalingMessage {
    pub fn new(msg_type: &str, payload: serde_json::Value) -> Self {
        Self {
            msg_type: msg_type.to_string(),
            request_id: None,
            payload,
        }
    }

    pub fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }

    pub fn error(code: u16, message: &str, request_id: Option<String>) -> Self {
        Self {
            msg_type: "error".to_string(),
            request_id,
            payload: serde_json::json!({
                "code": code,
                "message": message
            }),
        }
    }
}

// ==================== Client -> Server Messages ====================

/// join_room message payload
#[derive(Debug, Clone, Deserialize)]
pub struct JoinRoomPayload {
    pub room_id: String,
    pub display: String,
}

/// publish_offer message payload
#[derive(Debug, Clone, Deserialize)]
pub struct PublishOfferPayload {
    pub sdp: String,
    #[serde(default = "default_kind")]
    pub kind: String,
}

fn default_kind() -> String {
    "video".to_string()
}

/// trickle_ice message payload
#[derive(Debug, Clone, Deserialize)]
pub struct TrickleIcePayload {
    pub candidate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mline_index: Option<u16>,
    #[serde(default = "default_target")]
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed_id: Option<String>,
}

fn default_target() -> String {
    "publisher".to_string()
}

/// subscribe message payload
#[derive(Debug, Clone, Deserialize)]
pub struct SubscribePayload {
    pub feeds: Vec<SubscribeFeed>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscribeFeed {
    pub feed_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mid: Option<String>,
}

/// subscribe_answer message payload
#[derive(Debug, Clone, Deserialize)]
pub struct SubscribeAnswerPayload {
    pub sdp: String,
}

/// unsubscribe message payload
#[derive(Debug, Clone, Deserialize)]
pub struct UnsubscribePayload {
    pub feed_ids: Vec<String>,
}

// ==================== Server -> Client Messages ====================

/// joined response payload
#[derive(Debug, Clone, Serialize)]
pub struct JoinedPayload {
    pub room_id: String,
    pub user_id: String,
    pub publishers: Vec<PublisherPayload>,
    /// Number of participants currently in the room (source of truth server-side)
    pub participant_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participants: Option<Vec<MemberJoinedPayload>>,
}

/// Member joined / left payloads (for presence)
#[derive(Debug, Clone, Serialize)]
pub struct MemberJoinedPayload {
    pub user_id: String,
    pub display: String,
    pub room_id: String,
    /// Unix timestamp (seconds) when the member joined
    pub joined_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemberLeftPayload {
    pub user_id: String,
    pub room_id: String,
}

/// Publisher information in messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublisherPayload {
    pub feed_id: String,
    pub user_id: String,
    pub display: String,
}

/// publisher_joined event payload
#[derive(Debug, Clone, Serialize)]
pub struct PublisherJoinedPayload {
    pub feed_id: String,
    pub user_id: String,
    pub display: String,
    pub room_id: String,
}

/// publisher_left event payload
#[derive(Debug, Clone, Serialize)]
pub struct PublisherLeftPayload {
    pub feed_id: String,
    pub room_id: String,
}

/// publish_answer response payload
#[derive(Debug, Clone, Serialize)]
pub struct PublishAnswerPayload {
    pub sdp: String,
}

/// subscribe_offer response payload
#[derive(Debug, Clone, Serialize)]
pub struct SubscribeOfferPayload {
    pub sdp: String,
    pub feed_ids: Vec<String>,
}

/// remote_candidate event payload
#[derive(Debug, Clone, Serialize)]
pub struct RemoteCandidatePayload {
    pub candidate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdp_mline_index: Option<u16>,
    pub feed_id: String,
}

/// left_room response payload
#[derive(Debug, Clone, Serialize)]
pub struct LeftRoomPayload {
    pub success: bool,
}

/// Message types enum for matching
pub mod msg_types {
    pub const JOIN_ROOM: &str = "join_room";
    pub const PUBLISH_OFFER: &str = "publish_offer";
    pub const TRICKLE_ICE: &str = "trickle_ice";
    pub const SUBSCRIBE: &str = "subscribe";
    pub const SUBSCRIBE_ANSWER: &str = "subscribe_answer";
    pub const UNSUBSCRIBE: &str = "unsubscribe";
    pub const LEAVE: &str = "leave";
    pub const PING: &str = "ping";

    // Server -> Client
    pub const JOINED: &str = "joined";
    pub const PUBLISHER_JOINED: &str = "publisher_joined";
    pub const PUBLISHER_LEFT: &str = "publisher_left";
    pub const MEMBER_JOINED: &str = "member_joined";
    pub const MEMBER_LEFT: &str = "member_left";
    pub const PUBLISH_ANSWER: &str = "publish_answer";
    pub const SUBSCRIBE_OFFER: &str = "subscribe_offer";
    pub const REMOTE_CANDIDATE: &str = "remote_candidate";
    pub const LEFT_ROOM: &str = "left_room";
    pub const ERROR: &str = "error";
    pub const PONG: &str = "pong";
}
