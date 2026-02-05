use chrono::Utc;
use deadpool_redis::Pool;
use redis::AsyncCommands;

use crate::error::{AppError, Result};
use crate::models::{PublisherInfo, Room, RoomInfo, RoomInvitation, RoomStatus, WsSession};

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

    /// List recent rooms (MVP)
    pub async fn list_rooms(&self, limit: usize) -> Result<Vec<RoomInfo>> {
        let mut conn = self.pool.get().await?;

        // Get all keys room:*
        let keys: Vec<String> = conn.keys("room:*").await?;

        // Keep only exact keys: room:<uuid>
        let mut room_ids: Vec<String> = keys
            .into_iter()
            .filter_map(|k| {
                let parts: Vec<&str> = k.split(':').collect();
                if parts.len() == 2 && parts[0] == "room" {
                    Some(parts[1].to_string())
                } else {
                    None
                }
            })
            .collect();

        let mut infos: Vec<RoomInfo> = Vec::new();

        // Fetch RoomInfo for each id
        for room_id in room_ids.drain(..) {
            if let Some(info) = self.get_room_info(&room_id).await? {
                infos.push(info);
            }
        }

        // Sort most recent first
        infos.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply limit
        infos.truncate(limit.min(100));

        Ok(infos)
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

    /// Set member info (display name and joined_at) in a hash for persistence
    pub async fn set_member_info(&self, room_id: &str, user_id: &str, display: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members_info", room_id);

        let info = serde_json::json!({
            "user_id": user_id,
            "display": display,
            "joined_at": chrono::Utc::now().timestamp()
        });

        conn.hset::<_, _, _, ()>(&key, user_id, info.to_string()).await?;

        // Set TTL if room exists
        if let Some(room) = self.get_room(room_id).await? {
            redis::cmd("EXPIRE")
                .arg(&key)
                .arg(room.ttl_seconds as i64)
                .query_async::<()>(&mut *conn)
                .await?;
        }

        tracing::debug!(room_id = %room_id, user_id = %user_id, "Member info set");
        Ok(())
    }

    /// Remove member info from the hash
    pub async fn remove_member_info(&self, room_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members_info", room_id);

        conn.hdel::<_, _, ()>(&key, user_id).await?;

        tracing::debug!(room_id = %room_id, user_id = %user_id, "Member info removed");
        Ok(())
    }

    /// Get all members of a room
    pub async fn get_members(&self, room_id: &str) -> Result<Vec<String>> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members", room_id);

        let members: Vec<String> = conn.smembers(&key).await?;
        Ok(members)
    }

    /// Get all member infos (user_id + display + joined_at)
    pub async fn get_member_infos(&self, room_id: &str) -> Result<Vec<crate::models::user::MemberInfo>> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members_info", room_id);

        let data: Vec<(String, String)> = conn.hgetall(&key).await?;

        let members: Vec<crate::models::user::MemberInfo> = data
            .into_iter()
            .filter_map(|(_, json)| serde_json::from_str(&json).ok())
            .collect();

        Ok(members)
    }

    /// Remove a member from a room
    pub async fn remove_member(&self, room_id: &str, user_id: &str) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:members", room_id);

        conn.srem::<_, _, ()>(&key, user_id).await?;

        tracing::debug!(room_id = %room_id, user_id = %user_id, "Member removed");
        Ok(())
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

    // ==================== Creator Key (host access) ====================

    pub async fn set_creator_key_hash(
        &self,
        room_id: &str,
        hash: &str,
        ttl_seconds: u64,
    ) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:creator_key_hash", room_id);

        redis::cmd("SETEX")
            .arg(&key)
            .arg(ttl_seconds as i64)
            .arg(hash)
            .query_async::<()>(&mut *conn)
            .await?;

        Ok(())
    }

    pub async fn get_creator_key_hash(&self, room_id: &str) -> Result<Option<String>> {
        let mut conn = self.pool.get().await?;
        let key = format!("room:{}:creator_key_hash", room_id);

        let v: Option<String> = conn.get(&key).await?;
        Ok(v)
    }

    // ==================== Invitation Operations ====================

    /// Create a room invitation
    pub async fn create_invitation(&self, invitation: &RoomInvitation) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let key = format!("invite:{}", invitation.token);
        let json = serde_json::to_string(invitation)?;

        let ttl = (invitation.expires_at - Utc::now()).num_seconds().max(1) as i64;

        redis::cmd("SETEX")
            .arg(&key)
            .arg(ttl)
            .arg(&json)
            .query_async::<()>(&mut *conn)
            .await?;

        // Also add to room's invitation set for tracking
        let room_invites_key = format!("room:{}:invites", invitation.room_id);
        conn.sadd::<_, _, ()>(&room_invites_key, &invitation.token)
            .await?;

        tracing::info!(
            token = %invitation.token,
            room_id = %invitation.room_id,
            "Invitation created"
        );
        Ok(())
    }

    /// Get an invitation by token
    pub async fn get_invitation(&self, token: &str) -> Result<Option<RoomInvitation>> {
        let mut conn = self.pool.get().await?;
        let key = format!("invite:{}", token);

        let json: Option<String> = conn.get(&key).await?;

        match json {
            Some(data) => {
                let invitation: RoomInvitation = serde_json::from_str(&data)?;
                Ok(Some(invitation))
            }
            None => Ok(None),
        }
    }

    /// Increment invitation use count
    pub async fn use_invitation(&self, token: &str) -> Result<bool> {
        let mut invitation = match self.get_invitation(token).await? {
            Some(inv) => inv,
            None => return Ok(false),
        };

        if !invitation.is_valid() {
            return Ok(false);
        }

        invitation.uses += 1;

        let mut conn = self.pool.get().await?;
        let key = format!("invite:{}", token);
        let json = serde_json::to_string(&invitation)?;

        let ttl = (invitation.expires_at - Utc::now()).num_seconds().max(1) as i64;

        redis::cmd("SETEX")
            .arg(&key)
            .arg(ttl)
            .arg(&json)
            .query_async::<()>(&mut *conn)
            .await?;

        tracing::debug!(token = %token, uses = %invitation.uses, "Invitation used");
        Ok(true)
    }

    /// Delete an invitation
    pub async fn delete_invitation(&self, token: &str) -> Result<()> {
        let invitation = match self.get_invitation(token).await? {
            Some(inv) => inv,
            None => return Ok(()),
        };

        let mut conn = self.pool.get().await?;
        let key = format!("invite:{}", token);

        conn.del::<_, ()>(&key).await?;

        // Remove from room's invitation set
        let room_invites_key = format!("room:{}:invites", invitation.room_id);
        conn.srem::<_, _, ()>(&room_invites_key, token).await?;

        tracing::info!(token = %token, "Invitation deleted");
        Ok(())
    }

    /// Get all invitations for a room
    pub async fn get_room_invitations(&self, room_id: &str) -> Result<Vec<RoomInvitation>> {
        let mut conn = self.pool.get().await?;
        let room_invites_key = format!("room:{}:invites", room_id);

        let tokens: Vec<String> = conn.smembers(&room_invites_key).await?;

        let mut invitations = Vec::new();
        for token in tokens {
            if let Some(invitation) = self.get_invitation(&token).await? {
                invitations.push(invitation);
            } else {
                // Clean up expired invitation reference
                conn.srem::<_, _, ()>(&room_invites_key, &token).await?;
            }
        }

        Ok(invitations)
    }
}
