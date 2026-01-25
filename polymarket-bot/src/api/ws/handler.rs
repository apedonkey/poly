//! WebSocket connection handler

use crate::api::server::AppState;
use crate::types::Opportunity;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

/// WebSocket message from server to client
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data")]
pub enum WsServerMessage {
    #[serde(rename = "opportunities")]
    Opportunities(Vec<Opportunity>),
    #[serde(rename = "connected")]
    Connected { message: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "pong")]
    Pong,
}

/// WebSocket message from client to server
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "subscribe")]
    Subscribe,
}

/// WebSocket upgrade handler
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// Handle a WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    info!("WebSocket client connected");

    // Send connected message
    let connected_msg = WsServerMessage::Connected {
        message: "Connected to Polymarket Trading Bot".to_string(),
    };
    if let Ok(json) = serde_json::to_string(&connected_msg) {
        let _ = sender.send(Message::Text(json)).await;
    }

    // Send current opportunities immediately
    {
        let opportunities = state.opportunities.read().await;
        let msg = WsServerMessage::Opportunities(opportunities.clone());
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = sender.send(Message::Text(json)).await;
        }
    }

    // Subscribe to opportunity updates
    let mut opportunity_rx = state.subscribe();

    // Spawn task to forward opportunity updates to this client
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Handle broadcast updates
                result = opportunity_rx.recv() => {
                    match result {
                        Ok(opportunities) => {
                            let msg = WsServerMessage::Opportunities(opportunities);
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    debug!("WebSocket send failed, client disconnected");
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Broadcast receive error: {}", e);
                            break;
                        }
                    }
                }
            }
        }
    });

    // Handle incoming messages from client
    let recv_task = tokio::spawn(async move {
        while let Some(result) = receiver.next().await {
            match result {
                Ok(Message::Text(text)) => {
                    if let Ok(msg) = serde_json::from_str::<WsClientMessage>(&text) {
                        match msg {
                            WsClientMessage::Ping => {
                                debug!("Received ping");
                                // Pong is handled in the send task
                            }
                            WsClientMessage::Subscribe => {
                                debug!("Received subscribe");
                                // Already subscribed on connect
                            }
                        }
                    }
                }
                Ok(Message::Ping(_)) => {
                    // Pong is sent automatically by axum
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket client sent close");
                    break;
                }
                Err(e) => {
                    error!("WebSocket receive error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    info!("WebSocket client disconnected");
}
