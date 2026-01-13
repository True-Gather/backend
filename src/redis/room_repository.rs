use chrono::Utc;
use deadpool_redis::Pool;
use redis::AsyncCommands;

use crate::error::{AppError, Result};
use crate::models::{PublisherInfo, Room, RoomInfo, RoomStatus, WsSession};

/// Room repository for Redis operations
#[derive(Clone)]
pub struct RoomRepository {
    pool: Pool,
}

impl RoomRepository {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    // ==================== Room Operations ====================

    /// Create a new room with TTL
    pub async fn create_room(&self, room: &Room) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}", room.room_id);
        let json = serde_json::to_string(room)?;

        redis::cmd("SETEX")
            .arg(&key)
            .arg(room.ttl_seconds as i64)
            .arg(&json)
            .query_async::<()>(&mut *conn)
            .await?;

        tracing::info!(room_id = %room.room_id, "Room created");
        Ok(())
    }

    /// Get room by ID
    pub async fn get_room(&self, room_id: &str) -> Result<Option<Room>> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}", room_id);

        let json: Option<String> = conn.get(&key).await?;

        match json {
            Some(data) => {
                let room: Room = serde_json::from_str(&data)?;
                Ok(Some(room))
            }
            None => Ok(None),
        }
    }

    /// Get full room info including members and publishers
    pub async fn get_room_info(&self, room_id: &str) -> Result<Option<RoomInfo>> {
        let room = match self.get_room(room_id).await? {
            Some(r) => r,
            None => return Ok(None),
        };

        let members = self.get_members(room_id).await?;
        let publishers = self.get_publishers(room_id).await?;

        let status = if members.is_empty() {
            RoomStatus::Inactive
        } else {
            RoomStatus::Active
        };

        Ok(Some(RoomInfo {
            room_id: room.room_id,
            name: room.name,
            participants_count: members.len(),
            participants: members,
            publishers,
            status,
            created_at: room.created_at,
        }))
    }

    /// Delete a room
    pub async fn delete_room(&self, room_id: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;

        let keys = vec![
            format!("room:{}", room_id),
            format!("room:{}:members", room_id),
            format!("room:{}:publishers", room_id),
        ];

        redis::cmd("DEL")
            .arg(&keys)
            .query_async::<()>(&mut *conn)
            .await?;

        tracing::info!(room_id = %room_id, "Room deleted");
        Ok(())
    }

    /// Refresh room TTL
    pub async fn refresh_room_ttl(&self, room_id: &str, ttl_seconds: u64) -> Result<()> {
        let mut conn = self.pool.get().await?;

        let keys = vec![
            format!("room:{}", room_id),
            format!("room:{}:members", room_id),
            format!("room:{}:publishers", room_id),
        ];

        for key in keys {
            redis::cmd("EXPIRE")
                .arg(&key)
                .arg(ttl_seconds as i64)
                .query_async::<()>(&mut *conn)
                .await?;
        }

        Ok(())
    }

    // ==================== Member Operations ====================

    /// Add a member to a room
    pub async fn add_member(&self, room_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members", room_id);

        conn.sadd::<_, _, ()>(&key, user_id).await?;

        // Set TTL if room exists
        if let Some(room) = self.get_room(room_id).await? {
            redis::cmd("EXPIRE")
                .arg(&key)
                .arg(room.ttl_seconds as i64)
                .query_async::<()>(&mut *conn)
                .await?;
        }

        tracing::debug!(room_id = %room_id, user_id = %user_id, "Member added");
        Ok(())
    }

    /// Remove a member from a room
    pub async fn remove_member(&self, room_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members", room_id);

        conn.srem::<_, _, ()>(&key, user_id).await?;

        tracing::debug!(room_id = %room_id, user_id = %user_id, "Member removed");
        Ok(())
    }

    /// Get all members of a room
    pub async fn get_members(&self, room_id: &str) -> Result<Vec<String>> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members", room_id);

        let members: Vec<String> = conn.smembers(&key).await?;
        Ok(members)
    }

    /// Get member count
    pub async fn get_member_count(&self, room_id: &str) -> Result<usize> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members", room_id);

        let count: usize = conn.scard(&key).await?;
        Ok(count)
    }

    /// Check if user is a member
    pub async fn is_member(&self, room_id: &str, user_id: &str) -> Result<bool> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members", room_id);

        let is_member: bool = conn.sismember(&key, user_id).await?;
        Ok(is_member)
    }

    // ==================== Publisher Operations ====================

    /// Set a publisher in a room
    pub async fn set_publisher(
        &self,
        room_id: &str,
        user_id: &str,
        info: &PublisherInfo,
    ) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:publishers", room_id);
        let json = serde_json::to_string(info)?;

        conn.hset::<_, _, _, ()>(&key, user_id, &json).await?;

        // Set TTL if room exists
        if let Some(room) = self.get_room(room_id).await? {
            redis::cmd("EXPIRE")
                .arg(&key)
                .arg(room.ttl_seconds as i64)
                .query_async::<()>(&mut *conn)
                .await?;
        }

        tracing::debug!(room_id = %room_id, user_id = %user_id, "Publisher set");
        Ok(())
    }

    /// Remove a publisher from a room
    pub async fn remove_publisher(&self, room_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:publishers", room_id);

        conn.hdel::<_, _, ()>(&key, user_id).await?;

        tracing::debug!(room_id = %room_id, user_id = %user_id, "Publisher removed");
        Ok(())
    }

    /// Get all publishers in a room
    pub async fn get_publishers(&self, room_id: &str) -> Result<Vec<PublisherInfo>> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:publishers", room_id);

        let data: Vec<(String, String)> = conn.hgetall(&key).await?;

        let publishers: Vec<PublisherInfo> = data
            .into_iter()
            .filter_map(|(_, json)| serde_json::from_str(&json).ok())
            .collect();

        Ok(publishers)
    }

    /// Get a specific publisher
    pub async fn get_publisher(
        &self,
        room_id: &str,
        user_id: &str,
    ) -> Result<Option<PublisherInfo>> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:publishers", room_id);

        let json: Option<String> = conn.hget(&key, user_id).await?;

        match json {
            Some(data) => {
                let info: PublisherInfo = serde_json::from_str(&data)?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }

    /// Get publisher count
    pub async fn get_publisher_count(&self, room_id: &str) -> Result<usize> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:publishers", room_id);

        let count: usize = conn.hlen(&key).await?;
        Ok(count)
    }

    // ==================== WebSocket Session Operations ====================

    /// Create a WebSocket session
    pub async fn create_ws_session(&self, conn_id: &str, session: &WsSession) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("ws:{}", conn_id);
        let json = serde_json::to_string(session)?;

        // Session TTL: 30 minutes
        redis::cmd("SETEX")
            .arg(&key)
            .arg(1800i64)
            .arg(&json)
            .query_async::<()>(&mut *conn)
            .await?;

        Ok(())
    }

    /// Get a WebSocket session
    pub async fn get_ws_session(&self, conn_id: &str) -> Result<Option<WsSession>> {
        let mut conn = self.pool.get().await?;
        let key = format!("ws:{}", conn_id);

        let json: Option<String> = conn.get(&key).await?;

        match json {
            Some(data) => {
                let session: WsSession = serde_json::from_str(&data)?;
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    /// Update session last ping
    pub async fn update_ws_session_ping(&self, conn_id: &str) -> Result<()> {
        if let Some(mut session) = self.get_ws_session(conn_id).await? {
            session.last_ping = Utc::now().timestamp();
            self.create_ws_session(conn_id, &session).await?;
        }
        Ok(())
    }

    /// Delete a WebSocket session
    pub async fn delete_ws_session(&self, conn_id: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("ws:{}", conn_id);

        conn.del::<_, ()>(&key).await?;
        Ok(())
    }

    // ==================== Health Check ====================

    /// Check Redis connection health
    pub async fn health_check(&self) -> Result<bool> {
        let mut conn = self.pool.get().await?;

        let pong: String = redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .map_err(|e| AppError::RedisError(e.to_string()))?;

        Ok(pong == "PONG")
    }
}
