//! WebSocket connection handler

use crate::api::server::AppState;
use crate::services::PriceUpdate;
use crate::types::{ClarificationAlert, DisputeAlert, Opportunity};
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
    /// Real-time price update for a token
    #[serde(rename = "price_update")]
    PriceUpdate(PriceUpdate),
    /// Scan timing info for progress bar
    #[serde(rename = "scan_status")]
    ScanStatus {
        scan_interval_seconds: u64,
        last_scan_at: i64,  // Unix timestamp ms
    },
    /// Market clarification/description change alerts
    #[serde(rename = "clarifications")]
    Clarifications(Vec<ClarificationAlert>),
    /// UMA dispute alerts
    #[serde(rename = "disputes")]
    Disputes(Vec<DisputeAlert>),
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

    // Send current scan status so client knows where in the cycle we are
    {
        let last_scan = *state.last_scan_at.read().await;
        // Only send if we've had at least one scan
        if last_scan > 0 {
            let msg = WsServerMessage::ScanStatus {
                scan_interval_seconds: state.config.scan_interval_seconds,
                last_scan_at: last_scan,
            };
            if let Ok(json) = serde_json::to_string(&msg) {
                let _ = sender.send(Message::Text(json)).await;
            }
        }
    }

    // Subscribe to opportunity updates
    let mut opportunity_rx = state.subscribe();
    // Subscribe to price updates
    let mut price_rx = state.subscribe_prices();
    // Subscribe to scan status updates
    let mut scan_status_rx = state.subscribe_scan_status();
    // Subscribe to clarification alerts
    let mut clarification_rx = state.subscribe_clarifications();
    // Subscribe to dispute alerts
    let mut dispute_rx = state.subscribe_disputes();

    // Spawn task to forward opportunity, price, and scan status updates to this client
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Handle opportunity broadcast updates
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
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // Opportunity updates can be dropped if client is slow
                            debug!("Opportunity updates lagged by {} messages", n);
                        }
                        Err(e) => {
                            error!("Opportunity broadcast receive error: {}", e);
                            break;
                        }
                    }
                }

                // Handle price update broadcasts
                result = price_rx.recv() => {
                    match result {
                        Ok(price_update) => {
                            let msg = WsServerMessage::PriceUpdate(price_update);
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    debug!("WebSocket send failed, client disconnected");
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // Price updates can be dropped if client is slow
                            debug!("Price updates lagged by {} messages", n);
                        }
                        Err(e) => {
                            error!("Price broadcast receive error: {}", e);
                            break;
                        }
                    }
                }

                // Handle scan status broadcasts
                result = scan_status_rx.recv() => {
                    match result {
                        Ok(status) => {
                            let msg = WsServerMessage::ScanStatus {
                                scan_interval_seconds: status.scan_interval_seconds,
                                last_scan_at: status.last_scan_at,
                            };
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    debug!("WebSocket send failed, client disconnected");
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            debug!("Scan status updates lagged by {} messages", n);
                        }
                        Err(e) => {
                            error!("Scan status broadcast receive error: {}", e);
                            break;
                        }
                    }
                }

                // Handle clarification alert broadcasts
                result = clarification_rx.recv() => {
                    match result {
                        Ok(alerts) => {
                            let msg = WsServerMessage::Clarifications(alerts);
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    debug!("WebSocket send failed, client disconnected");
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            debug!("Clarification updates lagged by {} messages", n);
                        }
                        Err(e) => {
                            error!("Clarification broadcast receive error: {}", e);
                            break;
                        }
                    }
                }

                // Handle dispute alert broadcasts
                result = dispute_rx.recv() => {
                    match result {
                        Ok(alerts) => {
                            let msg = WsServerMessage::Disputes(alerts);
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if sender.send(Message::Text(json)).await.is_err() {
                                    debug!("WebSocket send failed, client disconnected");
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            debug!("Dispute updates lagged by {} messages", n);
                        }
                        Err(e) => {
                            error!("Dispute broadcast receive error: {}", e);
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
