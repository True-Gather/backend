use std::sync::Arc;
use tokio::sync::RwLock;

use dashmap::DashMap;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_OPUS, MIME_TYPE_VP8};
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};
use webrtc::track::track_local::track_local_static_rtp::TrackLocalStaticRTP;
use webrtc::track::track_local::TrackLocal;

use crate::config::Config;
use crate::error::{AppError, Result};
use crate::media::track_forwarder::TrackForwarder;

/// Publisher session holding the peer connection and tracks
pub struct PublisherSession {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub user_id: String,
    pub feed_id: String,
    pub local_tracks: Vec<Arc<TrackLocalStaticRTP>>,
    pub forwarders: Vec<Arc<TrackForwarder>>,
}

/// Subscriber session holding the peer connection
pub struct SubscriberSession {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub user_id: String,
    pub subscribed_feeds: Vec<String>,
}

/// Room media state
pub struct RoomMedia {
    pub publishers: DashMap<String, Arc<RwLock<PublisherSession>>>, // user_id -> PublisherSession
    pub subscribers: DashMap<String, Arc<RwLock<SubscriberSession>>>, // user_id -> SubscriberSession
}

impl RoomMedia {
    pub fn new() -> Self {
        Self {
            publishers: DashMap::new(),
            subscribers: DashMap::new(),
        }
    }
}

impl Default for RoomMedia {
    fn default() -> Self {
        Self::new()
    }
}

/// Media Gateway - SFU implementation using webrtc-rs
pub struct MediaGateway {
    rooms: DashMap<String, Arc<RoomMedia>>,
    ice_servers: Vec<RTCIceServer>,
    api: Arc<webrtc::api::API>,
}

impl MediaGateway {
    pub fn new(config: &Config) -> Result<Self> {
        // Configure media engine
        let mut media_engine = MediaEngine::default();

        // Register audio codec (Opus)
        media_engine.register_codec(
            RTCRtpCodecParameters {
                capability: RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_OPUS.to_owned(),
                    clock_rate: 48000,
                    channels: 2,
                    sdp_fmtp_line: "minptime=10;useinbandfec=1".to_owned(),
                    rtcp_feedback: vec![],
                },
                payload_type: 111,
                ..Default::default()
            },
            RTPCodecType::Audio,
        )?;

        // Register video codec (VP8)
        media_engine.register_codec(
            RTCRtpCodecParameters {
                capability: RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_VP8.to_owned(),
                    clock_rate: 90000,
                    channels: 0,
                    sdp_fmtp_line: String::new(),
                    rtcp_feedback: vec![],
                },
                payload_type: 96,
                ..Default::default()
            },
            RTPCodecType::Video,
        )?;

        // Create interceptor registry
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media_engine)?;

        // Create setting engine
        let setting_engine = SettingEngine::default();

        // Build API
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .with_interceptor_registry(registry)
            .with_setting_engine(setting_engine)
            .build();

        // Configure ICE servers
        let mut ice_servers = vec![RTCIceServer {
            urls: vec![config.stun_server.clone()],
            ..Default::default()
        }];

        if let Some(turn_server) = &config.turn_server {
            ice_servers.push(RTCIceServer {
                urls: vec![turn_server.clone()],
                username: config.turn_username.clone().unwrap_or_default(),
                credential: config.turn_credential.clone().unwrap_or_default(),
                ..Default::default()
            });
        }

        Ok(Self {
            rooms: DashMap::new(),
            ice_servers,
            api: Arc::new(api),
        })
    }

    /// Check if media gateway is healthy
    pub fn is_healthy(&self) -> bool {
        true // Could add more sophisticated checks
    }

    /// Get or create room media state
    fn get_or_create_room(&self, room_id: &str) -> Arc<RoomMedia> {
        self.rooms
            .entry(room_id.to_string())
            .or_insert_with(|| Arc::new(RoomMedia::new()))
            .clone()
    }

    /// Create RTCConfiguration
    fn create_config(&self) -> RTCConfiguration {
        RTCConfiguration {
            ice_servers: self.ice_servers.clone(),
            ..Default::default()
        }
    }

    /// Create a new publisher peer connection
    pub async fn create_publisher(
        &self,
        room_id: &str,
        user_id: &str,
        feed_id: &str,
        offer_sdp: &str,
    ) -> Result<String> {
        let room = self.get_or_create_room(room_id);

        // Create peer connection
        let peer_connection = Arc::new(self.api.new_peer_connection(self.create_config()).await?);

        // Set up track handling
        let local_tracks: Arc<RwLock<Vec<Arc<TrackLocalStaticRTP>>>> =
            Arc::new(RwLock::new(Vec::new()));
        let forwarders: Arc<RwLock<Vec<Arc<TrackForwarder>>>> = Arc::new(RwLock::new(Vec::new()));

        let local_tracks_clone = local_tracks.clone();
        let forwarders_clone = forwarders.clone();
        let room_clone = room.clone();
        let feed_id_clone = feed_id.to_string();

        // Handle incoming tracks from publisher
        peer_connection.on_track(Box::new(move |track, _receiver, _transceiver| {
            let local_tracks = local_tracks_clone.clone();
            let forwarders = forwarders_clone.clone();
            let _room = room_clone.clone();
            let feed_id = feed_id_clone.clone();

            Box::pin(async move {
                tracing::info!(
                    feed_id = %feed_id,
                    kind = ?track.kind(),
                    codec = %track.codec().capability.mime_type,
                    "Received track from publisher"
                );

                // Create local track for forwarding
                let codec = track.codec();
                let local_track = Arc::new(TrackLocalStaticRTP::new(
                    codec.capability.clone(),
                    format!("{}-{}", feed_id, track.kind()),
                    format!("truegather-{}", feed_id),
                ));

                // Create forwarder
                let forwarder = Arc::new(TrackForwarder::new(track.clone(), local_track.clone()));

                // Store tracks
                {
                    let mut tracks = local_tracks.write().await;
                    tracks.push(local_track);
                }

                {
                    let mut fwds = forwarders.write().await;
                    fwds.push(forwarder.clone());
                }

                // Start forwarding
                forwarder.start().await;
            })
        }));

        // Handle ICE connection state changes
        let user_id_log = user_id.to_string();
        peer_connection.on_peer_connection_state_change(Box::new(move |state| {
            tracing::info!(
                user_id = %user_id_log,
                state = ?state,
                "Publisher peer connection state changed"
            );
            Box::pin(async {})
        }));

        // Set remote description (offer from client)
        let offer = RTCSessionDescription::offer(offer_sdp.to_string())?;
        peer_connection.set_remote_description(offer).await?;

        // Create answer
        let answer = peer_connection.create_answer(None).await?;
        peer_connection
            .set_local_description(answer.clone())
            .await?;

        // Wait for ICE gathering to complete
        let mut gather_complete = peer_connection.gathering_complete_promise().await;
        let _ = gather_complete.recv().await;

        // Get local description with ICE candidates
        let local_desc = peer_connection
            .local_description()
            .await
            .ok_or_else(|| AppError::WebRtcError("No local description".to_string()))?;

        // Store publisher session
        let session = PublisherSession {
            peer_connection: peer_connection.clone(),
            user_id: user_id.to_string(),
            feed_id: feed_id.to_string(),
            local_tracks: local_tracks.read().await.clone(),
            forwarders: forwarders.read().await.clone(),
        };

        room.publishers
            .insert(user_id.to_string(), Arc::new(RwLock::new(session)));

        tracing::info!(
            room_id = %room_id,
            user_id = %user_id,
            feed_id = %feed_id,
            "Publisher peer connection created"
        );

        Ok(local_desc.sdp)
    }

    /// Add ICE candidate to publisher peer connection
    pub async fn add_ice_candidate_publisher(
        &self,
        room_id: &str,
        user_id: &str,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u16>,
    ) -> Result<()> {
        if let Some(room) = self.rooms.get(room_id) {
            if let Some(session) = room.publishers.get(user_id) {
                let session = session.read().await;
                let ice_candidate = RTCIceCandidateInit {
                    candidate: candidate.to_string(),
                    sdp_mid: sdp_mid.map(|s| s.to_string()),
                    sdp_mline_index,
                    ..Default::default()
                };
                session
                    .peer_connection
                    .add_ice_candidate(ice_candidate)
                    .await?;
            }
        }
        Ok(())
    }

    /// Create a subscriber peer connection
    pub async fn create_subscriber(
        &self,
        room_id: &str,
        user_id: &str,
        feed_ids: &[String],
    ) -> Result<String> {
        let room = self
            .rooms
            .get(room_id)
            .ok_or_else(|| AppError::NotFound("Room not found".to_string()))?;

        // Create peer connection
        let peer_connection = Arc::new(self.api.new_peer_connection(self.create_config()).await?);

        // Add tracks from requested publishers
        for feed_id in feed_ids {
            // Find publisher by feed_id
            for entry in room.publishers.iter() {
                let session = entry.value().read().await;
                if session.feed_id == *feed_id {
                    // Add all local tracks from this publisher
                    for track in &session.local_tracks {
                        let rtp_sender = peer_connection
                            .add_track(Arc::clone(track) as Arc<dyn TrackLocal + Send + Sync>)
                            .await?;

                        // Handle RTCP packets (for stats, etc.)
                        tokio::spawn(async move {
                            let mut rtcp_buf = vec![0u8; 1500];
                            while let Ok((_, _)) = rtp_sender.read(&mut rtcp_buf).await {
                                // Process RTCP if needed
                            }
                        });
                    }
                    break;
                }
            }
        }

        // Handle ICE connection state changes
        let user_id_log = user_id.to_string();
        peer_connection.on_peer_connection_state_change(Box::new(move |state| {
            tracing::info!(
                user_id = %user_id_log,
                state = ?state,
                "Subscriber peer connection state changed"
            );
            Box::pin(async {})
        }));

        // Create offer
        let offer = peer_connection.create_offer(None).await?;
        peer_connection.set_local_description(offer.clone()).await?;

        // Wait for ICE gathering
        let mut gather_complete = peer_connection.gathering_complete_promise().await;
        let _ = gather_complete.recv().await;

        // Get local description with ICE candidates
        let local_desc = peer_connection
            .local_description()
            .await
            .ok_or_else(|| AppError::WebRtcError("No local description".to_string()))?;

        // Store subscriber session
        let session = SubscriberSession {
            peer_connection,
            user_id: user_id.to_string(),
            subscribed_feeds: feed_ids.to_vec(),
        };

        room.subscribers
            .insert(user_id.to_string(), Arc::new(RwLock::new(session)));

        tracing::info!(
            room_id = %room_id,
            user_id = %user_id,
            feeds = ?feed_ids,
            "Subscriber peer connection created"
        );

        Ok(local_desc.sdp)
    }

    /// Set subscriber answer
    pub async fn set_subscriber_answer(
        &self,
        room_id: &str,
        user_id: &str,
        answer_sdp: &str,
    ) -> Result<()> {
        if let Some(room) = self.rooms.get(room_id) {
            if let Some(session) = room.subscribers.get(user_id) {
                let session = session.read().await;
                let answer = RTCSessionDescription::answer(answer_sdp.to_string())?;
                session
                    .peer_connection
                    .set_remote_description(answer)
                    .await?;
            }
        }
        Ok(())
    }

    /// Add ICE candidate to subscriber peer connection
    pub async fn add_ice_candidate_subscriber(
        &self,
        room_id: &str,
        user_id: &str,
        _feed_id: &str,
        candidate: &str,
        sdp_mid: Option<&str>,
        sdp_mline_index: Option<u16>,
    ) -> Result<()> {
        if let Some(room) = self.rooms.get(room_id) {
            if let Some(session) = room.subscribers.get(user_id) {
                let session = session.read().await;
                let ice_candidate = RTCIceCandidateInit {
                    candidate: candidate.to_string(),
                    sdp_mid: sdp_mid.map(|s| s.to_string()),
                    sdp_mline_index,
                    ..Default::default()
                };
                session
                    .peer_connection
                    .add_ice_candidate(ice_candidate)
                    .await?;
            }
        }
        Ok(())
    }

    /// Remove a publisher
    pub async fn remove_publisher(&self, room_id: &str, user_id: &str) {
        if let Some(room) = self.rooms.get(room_id) {
            if let Some((_, session)) = room.publishers.remove(user_id) {
                let session = session.read().await;

                // Stop forwarders
                for forwarder in &session.forwarders {
                    forwarder.stop().await;
                }

                // Close peer connection
                let _ = session.peer_connection.close().await;

                tracing::info!(
                    room_id = %room_id,
                    user_id = %user_id,
                    "Publisher removed"
                );
            }
        }
    }

    /// Remove a subscriber
    pub async fn remove_subscriber(&self, room_id: &str, user_id: &str, _feed_id: &str) {
        if let Some(room) = self.rooms.get(room_id) {
            if let Some((_, session)) = room.subscribers.remove(user_id) {
                let session = session.read().await;

                // Close peer connection
                let _ = session.peer_connection.close().await;

                tracing::info!(
                    room_id = %room_id,
                    user_id = %user_id,
                    "Subscriber removed"
                );
            }
        }
    }

    /// Clean up a room
    pub async fn cleanup_room(&self, room_id: &str) {
        if let Some((_, room)) = self.rooms.remove(room_id) {
            // Close all publisher connections
            for entry in room.publishers.iter() {
                let session = entry.value().read().await;
                for forwarder in &session.forwarders {
                    forwarder.stop().await;
                }
                let _ = session.peer_connection.close().await;
            }

            // Close all subscriber connections
            for entry in room.subscribers.iter() {
                let session = entry.value().read().await;
                let _ = session.peer_connection.close().await;
            }

            tracing::info!(room_id = %room_id, "Room media cleaned up");
        }
    }

    /// Get publisher count in a room
    pub fn get_publisher_count(&self, room_id: &str) -> usize {
        self.rooms
            .get(room_id)
            .map(|r| r.publishers.len())
            .unwrap_or(0)
    }

    /// Get subscriber count in a room
    pub fn get_subscriber_count(&self, room_id: &str) -> usize {
        self.rooms
            .get(room_id)
            .map(|r| r.subscribers.len())
            .unwrap_or(0)
    }
}
