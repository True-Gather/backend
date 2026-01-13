use std::sync::Arc;
use tokio::sync::mpsc;

use crate::models::Claims;
use crate::ws::SignalingMessage;

/// WebSocket session state
#[derive(Debug)]
pub struct WsSessionState {
    pub conn_id: String,
    pub user_id: String,
    pub room_id: String,
    pub display: String,
    pub claims: Claims,
    pub is_publishing: bool,
    pub feed_id: Option<String>,
    pub subscribed_feeds: Vec<String>,
}

impl WsSessionState {
    pub fn new(conn_id: String, claims: Claims) -> Self {
        Self {
            conn_id,
            user_id: claims.sub.clone(),
            room_id: claims.room_id.clone(),
            display: claims.display.clone(),
            claims,
            is_publishing: false,
            feed_id: None,
            subscribed_feeds: Vec::new(),
        }
    }

    pub fn set_publishing(&mut self, feed_id: String) {
        self.is_publishing = true;
        self.feed_id = Some(feed_id);
    }

    pub fn add_subscription(&mut self, feed_id: String) {
        if !self.subscribed_feeds.contains(&feed_id) {
            self.subscribed_feeds.push(feed_id);
        }
    }

    pub fn remove_subscription(&mut self, feed_id: &str) {
        self.subscribed_feeds.retain(|f| f != feed_id);
    }
}

/// Client connection handle for sending messages
#[derive(Clone)]
pub struct ClientHandle {
    pub conn_id: String,
    pub user_id: String,
    pub room_id: String,
    pub display: String,
    pub sender: mpsc::UnboundedSender<SignalingMessage>,
}

impl ClientHandle {
    pub fn new(
        conn_id: String,
        user_id: String,
        room_id: String,
        display: String,
        sender: mpsc::UnboundedSender<SignalingMessage>,
    ) -> Self {
        Self {
            conn_id,
            user_id,
            room_id,
            display,
            sender,
        }
    }

    pub fn send(
        &self,
        msg: SignalingMessage,
    ) -> Result<(), mpsc::error::SendError<SignalingMessage>> {
        self.sender.send(msg)
    }
}

/// Room connections manager - tracks all clients in a room
pub struct RoomConnections {
    clients: dashmap::DashMap<String, ClientHandle>, // conn_id -> ClientHandle
}

impl RoomConnections {
    pub fn new() -> Self {
        Self {
            clients: dashmap::DashMap::new(),
        }
    }

    pub fn add_client(&self, handle: ClientHandle) {
        self.clients.insert(handle.conn_id.clone(), handle);
    }

    pub fn remove_client(&self, conn_id: &str) -> Option<ClientHandle> {
        self.clients.remove(conn_id).map(|(_, v)| v)
    }

    pub fn get_client(&self, conn_id: &str) -> Option<ClientHandle> {
        self.clients.get(conn_id).map(|r| r.clone())
    }

    pub fn get_client_by_user_id(&self, user_id: &str) -> Option<ClientHandle> {
        self.clients
            .iter()
            .find(|r| r.user_id == user_id)
            .map(|r| r.clone())
    }

    pub fn broadcast(&self, msg: SignalingMessage, exclude_conn_id: Option<&str>) {
        for client in self.clients.iter() {
            if let Some(exclude) = exclude_conn_id {
                if client.conn_id == exclude {
                    continue;
                }
            }
            let _ = client.send(msg.clone());
        }
    }

    pub fn broadcast_to_subscribers(
        &self,
        msg: SignalingMessage,
        _feed_id: &str,
        exclude_conn_id: Option<&str>,
    ) {
        // For now, broadcast to all - would need subscriber tracking for optimization
        self.broadcast(msg, exclude_conn_id);
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    pub fn get_all_client_ids(&self) -> Vec<String> {
        self.clients.iter().map(|r| r.conn_id.clone()).collect()
    }
}

impl Default for RoomConnections {
    fn default() -> Self {
        Self::new()
    }
}

/// Global connections manager - tracks all rooms
pub struct ConnectionsManager {
    rooms: dashmap::DashMap<String, Arc<RoomConnections>>, // room_id -> RoomConnections
}

impl ConnectionsManager {
    pub fn new() -> Self {
        Self {
            rooms: dashmap::DashMap::new(),
        }
    }

    pub fn get_or_create_room(&self, room_id: &str) -> Arc<RoomConnections> {
        self.rooms
            .entry(room_id.to_string())
            .or_insert_with(|| Arc::new(RoomConnections::new()))
            .clone()
    }

    pub fn get_room(&self, room_id: &str) -> Option<Arc<RoomConnections>> {
        self.rooms.get(room_id).map(|r| r.clone())
    }

    pub fn remove_client_from_room(&self, room_id: &str, conn_id: &str) -> Option<ClientHandle> {
        if let Some(room) = self.rooms.get(room_id) {
            let handle = room.remove_client(conn_id);

            // Clean up empty rooms
            if room.is_empty() {
                self.rooms.remove(room_id);
            }

            handle
        } else {
            None
        }
    }

    pub fn broadcast_to_room(
        &self,
        room_id: &str,
        msg: SignalingMessage,
        exclude_conn_id: Option<&str>,
    ) {
        if let Some(room) = self.rooms.get(room_id) {
            room.broadcast(msg, exclude_conn_id);
        }
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }
}

impl Default for ConnectionsManager {
    fn default() -> Self {
        Self::new()
    }
}
