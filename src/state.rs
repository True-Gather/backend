use std::sync::Arc;

use crate::auth::AuthService;
use crate::config::Config;
use crate::media::MediaGateway;
use crate::mail::Mailer;
use crate::redis::RoomRepository;
use crate::ws::ConnectionsManager;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub auth: Arc<AuthService>,
    pub room_repo: Arc<RoomRepository>,
    pub media_gateway: Arc<MediaGateway>,
    pub connections: Arc<ConnectionsManager>,
    pub mailer: Arc<Mailer>,
}

impl AppState {
    pub fn new(
        config: Config,
        auth: AuthService,
        room_repo: RoomRepository,
        media_gateway: MediaGateway,
        mailer: Mailer,
    ) -> Self {
        Self {
            config: Arc::new(config),
            auth: Arc::new(auth),
            room_repo: Arc::new(room_repo),
            media_gateway: Arc::new(media_gateway),
            connections: Arc::new(ConnectionsManager::new()),
            mailer: Arc::new(mailer),
        }
    }
}
