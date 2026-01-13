use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    response::Response,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::api::rooms::create_publisher_info;
use crate::error::AppError;
use crate::state::AppState;
use crate::ws::{
    msg_types, ClientHandle, JoinRoomPayload, JoinedPayload, LeftRoomPayload, PublishAnswerPayload,
    PublishOfferPayload, PublisherJoinedPayload, PublisherLeftPayload, PublisherPayload,
    SignalingMessage, SubscribeOfferPayload, SubscribePayload, TrickleIcePayload, WsSessionState,
};

/// Query parameters for WebSocket connection
#[derive(Debug, Deserialize)]
pub struct WsQueryParams {
    pub room_id: String,
    pub token: String,
}

/// WebSocket routes
pub fn ws_routes() -> Router<AppState> {
    Router::new().route("/ws", get(ws_upgrade))
}

/// WebSocket upgrade handler
async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<WsQueryParams>,
) -> Result<Response, AppError> {
    // Validate JWT token
    let claims = state.auth.validate_token(&params.token)?;

    // Verify room_id matches
    if claims.room_id != params.room_id {
        return Err(AppError::Unauthorized(
            "Token room_id does not match".to_string(),
        ));
    }

    // Check room exists
    let _room = state
        .room_repo
        .get_room(&params.room_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Room not found".to_string()))?;

    tracing::info!(
        room_id = %params.room_id,
        user_id = %claims.sub,
        display = %claims.display,
        "WebSocket upgrade request"
    );

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state, claims)))
}

/// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState, claims: crate::models::Claims) {
    let conn_id = Uuid::new_v4().to_string();
    let room_id = claims.room_id.clone();
    let user_id = claims.sub.clone();
    let display = claims.display.clone();

    tracing::info!(
        conn_id = %conn_id,
        room_id = %room_id,
        user_id = %user_id,
        "WebSocket connected"
    );

    // Create message channel for sending to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<SignalingMessage>();

    // Create session state
    let mut session = WsSessionState::new(conn_id.clone(), claims);

    // Create client handle and add to room
    let client_handle = ClientHandle::new(
        conn_id.clone(),
        user_id.clone(),
        room_id.clone(),
        display.clone(),
        tx,
    );

    let room_connections = state.connections.get_or_create_room(&room_id);
    room_connections.add_client(client_handle);

    // Split socket into sender and receiver
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Task for sending messages to client
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Process incoming messages
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                if let Err(e) = handle_message(&text, &mut session, &state).await {
                    tracing::error!(error = %e, "Error handling message");
                    // Send error to client
                    if let Some(room) = state.connections.get_room(&room_id) {
                        if let Some(client) = room.get_client(&conn_id) {
                            let _ = client.send(SignalingMessage::error(500, &e.to_string(), None));
                        }
                    }
                }
            }
            Ok(Message::Ping(_data)) => {
                // Respond with pong automatically handled by axum
                tracing::trace!(conn_id = %conn_id, "Ping received");
            }
            Ok(Message::Close(_)) => {
                tracing::info!(conn_id = %conn_id, "WebSocket close received");
                break;
            }
            Err(e) => {
                tracing::error!(conn_id = %conn_id, error = %e, "WebSocket error");
                break;
            }
            _ => {}
        }
    }

    // Cleanup on disconnect
    tracing::info!(
        conn_id = %conn_id,
        room_id = %room_id,
        user_id = %user_id,
        "WebSocket disconnected, cleaning up"
    );

    // Remove from room connections
    state
        .connections
        .remove_client_from_room(&room_id, &conn_id);

    // Remove from Redis
    let _ = state.room_repo.remove_member(&room_id, &user_id).await;

    // If publishing, remove publisher and notify others
    if session.is_publishing {
        if let Some(feed_id) = &session.feed_id {
            let _ = state.room_repo.remove_publisher(&room_id, &user_id).await;

            // Remove from media gateway
            state
                .media_gateway
                .remove_publisher(&room_id, &user_id)
                .await;

            // Broadcast publisher left
            let msg = SignalingMessage::new(
                msg_types::PUBLISHER_LEFT,
                serde_json::to_value(PublisherLeftPayload {
                    feed_id: feed_id.clone(),
                    room_id: room_id.clone(),
                })
                .unwrap(),
            );

            state
                .connections
                .broadcast_to_room(&room_id, msg, Some(&conn_id));
        }
    }

    // Cleanup subscriptions in media gateway
    for feed_id in &session.subscribed_feeds {
        state
            .media_gateway
            .remove_subscriber(&room_id, &user_id, feed_id)
            .await;
    }

    // Cancel send task
    send_task.abort();
}

/// Handle incoming signaling message
async fn handle_message(
    text: &str,
    session: &mut WsSessionState,
    state: &AppState,
) -> Result<(), AppError> {
    let msg: SignalingMessage = serde_json::from_str(text)?;
    let request_id = msg.request_id.clone();

    tracing::debug!(
        msg_type = %msg.msg_type,
        conn_id = %session.conn_id,
        "Received message"
    );

    match msg.msg_type.as_str() {
        msg_types::JOIN_ROOM => {
            handle_join_room(msg.payload, request_id, session, state).await?;
        }
        msg_types::PUBLISH_OFFER => {
            handle_publish_offer(msg.payload, request_id, session, state).await?;
        }
        msg_types::TRICKLE_ICE => {
            handle_trickle_ice(msg.payload, session, state).await?;
        }
        msg_types::SUBSCRIBE => {
            handle_subscribe(msg.payload, request_id, session, state).await?;
        }
        msg_types::SUBSCRIBE_ANSWER => {
            handle_subscribe_answer(msg.payload, session, state).await?;
        }
        msg_types::LEAVE => {
            handle_leave(request_id, session, state).await?;
        }
        msg_types::PING => {
            handle_ping(request_id, session, state).await?;
        }
        _ => {
            tracing::warn!(msg_type = %msg.msg_type, "Unknown message type");
            send_error(400, "Unknown message type", request_id, session, state);
        }
    }

    Ok(())
}

/// Handle join_room message
async fn handle_join_room(
    payload: serde_json::Value,
    request_id: Option<String>,
    session: &mut WsSessionState,
    state: &AppState,
) -> Result<(), AppError> {
    let join_payload: JoinRoomPayload = serde_json::from_value(payload)?;

    // Verify room matches token
    if join_payload.room_id != session.room_id {
        return Err(AppError::Unauthorized(
            "Room ID does not match token".to_string(),
        ));
    }

    // Get existing publishers
    let publishers = state.room_repo.get_publishers(&session.room_id).await?;
    let publisher_payloads: Vec<PublisherPayload> = publishers
        .iter()
        .map(|p| PublisherPayload {
            feed_id: p.feed_id.clone(),
            display: p.display.clone(),
        })
        .collect();

    // Send joined response
    let response = SignalingMessage::new(
        msg_types::JOINED,
        serde_json::to_value(JoinedPayload {
            room_id: session.room_id.clone(),
            user_id: session.user_id.clone(),
            publishers: publisher_payloads,
        })?,
    )
    .with_request_id(request_id);

    send_to_client(response, session, state);

    tracing::info!(
        room_id = %session.room_id,
        user_id = %session.user_id,
        "User joined room via signaling"
    );

    Ok(())
}

/// Handle publish_offer message
async fn handle_publish_offer(
    payload: serde_json::Value,
    request_id: Option<String>,
    session: &mut WsSessionState,
    state: &AppState,
) -> Result<(), AppError> {
    let offer_payload: PublishOfferPayload = serde_json::from_value(payload)?;

    // Check if already publishing
    if session.is_publishing {
        return Err(AppError::BadRequest("Already publishing".to_string()));
    }

    // Generate feed_id
    let feed_id = Uuid::new_v4().to_string();

    // Create publisher in media gateway
    let answer_sdp = state
        .media_gateway
        .create_publisher(
            &session.room_id,
            &session.user_id,
            &feed_id,
            &offer_payload.sdp,
        )
        .await?;

    // Update session state
    session.set_publishing(feed_id.clone());

    // Save publisher to Redis
    let publisher_info = create_publisher_info(&session.user_id, &feed_id, &session.display);
    state
        .room_repo
        .set_publisher(&session.room_id, &session.user_id, &publisher_info)
        .await?;

    // Send answer to publisher
    let response = SignalingMessage::new(
        msg_types::PUBLISH_ANSWER,
        serde_json::to_value(PublishAnswerPayload { sdp: answer_sdp })?,
    )
    .with_request_id(request_id);

    send_to_client(response, session, state);

    // Broadcast publisher_joined to other clients
    let broadcast_msg = SignalingMessage::new(
        msg_types::PUBLISHER_JOINED,
        serde_json::to_value(PublisherJoinedPayload {
            feed_id,
            display: session.display.clone(),
            room_id: session.room_id.clone(),
        })?,
    );

    state
        .connections
        .broadcast_to_room(&session.room_id, broadcast_msg, Some(&session.conn_id));

    tracing::info!(
        room_id = %session.room_id,
        user_id = %session.user_id,
        "Publisher started streaming"
    );

    Ok(())
}

/// Handle trickle_ice message
async fn handle_trickle_ice(
    payload: serde_json::Value,
    session: &WsSessionState,
    state: &AppState,
) -> Result<(), AppError> {
    let ice_payload: TrickleIcePayload = serde_json::from_value(payload)?;

    if ice_payload.target == "publisher" {
        // ICE for publisher peer connection
        state
            .media_gateway
            .add_ice_candidate_publisher(
                &session.room_id,
                &session.user_id,
                &ice_payload.candidate,
                ice_payload.sdp_mid.as_deref(),
                ice_payload.sdp_mline_index,
            )
            .await?;
    } else if ice_payload.target == "subscriber" {
        // ICE for subscriber peer connection
        if let Some(feed_id) = &ice_payload.feed_id {
            state
                .media_gateway
                .add_ice_candidate_subscriber(
                    &session.room_id,
                    &session.user_id,
                    feed_id,
                    &ice_payload.candidate,
                    ice_payload.sdp_mid.as_deref(),
                    ice_payload.sdp_mline_index,
                )
                .await?;
        }
    }

    Ok(())
}

/// Handle subscribe message
async fn handle_subscribe(
    payload: serde_json::Value,
    request_id: Option<String>,
    session: &mut WsSessionState,
    state: &AppState,
) -> Result<(), AppError> {
    let sub_payload: SubscribePayload = serde_json::from_value(payload)?;

    let feed_ids: Vec<String> = sub_payload
        .feeds
        .iter()
        .map(|f| f.feed_id.clone())
        .collect();

    // Create subscriber in media gateway
    let offer_sdp = state
        .media_gateway
        .create_subscriber(&session.room_id, &session.user_id, &feed_ids)
        .await?;

    // Update session state
    for feed_id in &feed_ids {
        session.add_subscription(feed_id.clone());
    }

    // Send offer to subscriber
    let response = SignalingMessage::new(
        msg_types::SUBSCRIBE_OFFER,
        serde_json::to_value(SubscribeOfferPayload {
            sdp: offer_sdp,
            feed_ids,
        })?,
    )
    .with_request_id(request_id);

    send_to_client(response, session, state);

    tracing::debug!(
        room_id = %session.room_id,
        user_id = %session.user_id,
        "Subscribe offer sent"
    );

    Ok(())
}

/// Handle subscribe_answer message
async fn handle_subscribe_answer(
    payload: serde_json::Value,
    session: &WsSessionState,
    state: &AppState,
) -> Result<(), AppError> {
    let answer_payload: crate::ws::SubscribeAnswerPayload = serde_json::from_value(payload)?;

    state
        .media_gateway
        .set_subscriber_answer(&session.room_id, &session.user_id, &answer_payload.sdp)
        .await?;

    tracing::debug!(
        room_id = %session.room_id,
        user_id = %session.user_id,
        "Subscriber answer set"
    );

    Ok(())
}

/// Handle leave message
async fn handle_leave(
    request_id: Option<String>,
    session: &WsSessionState,
    state: &AppState,
) -> Result<(), AppError> {
    // Send confirmation
    let response = SignalingMessage::new(
        msg_types::LEFT_ROOM,
        serde_json::to_value(LeftRoomPayload { success: true })?,
    )
    .with_request_id(request_id);

    send_to_client(response, session, state);

    // The actual cleanup will happen when the socket closes

    tracing::info!(
        room_id = %session.room_id,
        user_id = %session.user_id,
        "User requested leave"
    );

    Ok(())
}

/// Handle ping message
async fn handle_ping(
    request_id: Option<String>,
    session: &WsSessionState,
    state: &AppState,
) -> Result<(), AppError> {
    let response =
        SignalingMessage::new(msg_types::PONG, serde_json::json!({})).with_request_id(request_id);

    send_to_client(response, session, state);

    // Update last ping in Redis
    let _ = state
        .room_repo
        .update_ws_session_ping(&session.conn_id)
        .await;

    Ok(())
}

/// Send a message to the current client
fn send_to_client(msg: SignalingMessage, session: &WsSessionState, state: &AppState) {
    if let Some(room) = state.connections.get_room(&session.room_id) {
        if let Some(client) = room.get_client(&session.conn_id) {
            let _ = client.send(msg);
        }
    }
}

/// Send an error message to the current client
fn send_error(
    code: u16,
    message: &str,
    request_id: Option<String>,
    session: &WsSessionState,
    state: &AppState,
) {
    let error_msg = SignalingMessage::error(code, message, request_id);
    send_to_client(error_msg, session, state);
}
