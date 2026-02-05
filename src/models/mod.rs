pub mod room;
pub mod user;

// -----------------------------
// Room module re-exports
// -----------------------------
pub use room::{
    Room,
    RoomInfo,
    PublisherInfo,
    RoomStatus,
    CreateRoomRequest,
    CreateRoomResponse,
    JoinRequest, // ✅ Option B join request (invite_token+invite_code OR creator_key)
    RoomInvitation,
    CreateInvitationRequest,
    CreateInvitationResponse,
    InvitationInfo,
    InviteEmailRequest,
    InviteEmailResponse,
};

// -----------------------------
// User module re-exports
// (these were previously pulled by glob and are still expected by other modules)
// -----------------------------
pub use user::{
    // ✅ Auth / WS
    Claims,
    WsSession,

    // ✅ Join REST response structures
    JoinResponse,
    IceServer,

    // ✅ If you renamed the "user join" request to avoid collision
    UserJoinRequest,
};
