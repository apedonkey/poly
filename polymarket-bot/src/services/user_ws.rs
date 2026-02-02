//! User Channel WebSocket - connects to Polymarket for real-time order events
//!
//! Subscribes to `wss://ws-subscriptions-clob.polymarket.com/ws/user`
//! to receive order status changes and fill events.
//!
//! Auth message format:
//! ```json
//! {
//!   "auth": { "apiKey": "...", "secret": "...", "passphrase": "..." },
//!   "type": "user"
//! }
//! ```

use crate::db::Database;
use crate::services::metrics::Metrics;
use crate::types::OrderLifecycleStatus;
use anyhow::{Context, Result};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, watch};
use tokio::time::{sleep, Duration};
use tokio_tungstenite::connect_async;
use tracing::{debug, info, warn};

const USER_WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/user";
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

/// An order event received from the user WebSocket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderEvent {
    /// Order ID
    pub order_id: String,
    /// Event type (e.g., "placement", "update", "trade", "cancellation")
    pub event_type: String,
    /// New status
    pub status: String,
    /// Fill price (for trade events)
    pub fill_price: Option<String>,
    /// Fill size (for trade events)
    pub fill_size: Option<String>,
    /// Token ID
    pub token_id: Option<String>,
    /// Timestamp
    pub timestamp: i64,
}

/// Auth payload for user WebSocket
#[derive(Debug, Serialize)]
struct UserAuthMessage {
    auth: UserAuthCredentials,
    #[serde(rename = "type")]
    msg_type: String,
}

#[derive(Debug, Serialize)]
struct UserAuthCredentials {
    #[serde(rename = "apiKey")]
    api_key: String,
    secret: String,
    passphrase: String,
}

/// Raw WebSocket message from the user channel
#[derive(Debug, Deserialize)]
struct UserWsMessage {
    #[serde(default, rename = "type")]
    msg_type: Option<String>,
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    order_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    price: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    token_id: Option<String>,
    #[serde(default)]
    timestamp: Option<i64>,
}

/// User Channel WebSocket service
pub struct UserWebSocket;

impl UserWebSocket {
    /// Run the user WebSocket for a specific wallet.
    /// Auto-reconnects on disconnection. Stops when shutdown_rx signals true.
    pub async fn run(
        api_key: String,
        api_secret: String,
        api_passphrase: String,
        wallet_address: String,
        db: Arc<Database>,
        order_event_tx: broadcast::Sender<OrderEvent>,
        metrics: Metrics,
        mut shutdown_rx: watch::Receiver<bool>,
    ) {
        info!("[User WS] Starting for wallet {}", wallet_address);

        let mut first_connect = true;
        loop {
            // Check for shutdown before connecting
            if *shutdown_rx.borrow() {
                info!("[User WS] Shutdown signal received for {}", wallet_address);
                break;
            }

            match Self::connect_and_listen(
                &api_key,
                &api_secret,
                &api_passphrase,
                &wallet_address,
                &db,
                &order_event_tx,
                &metrics,
            )
            .await
            {
                Ok(()) => {
                    info!("[User WS] Connection closed for {}. Reconnecting...", wallet_address);
                }
                Err(e) => {
                    warn!("[User WS] Error for {}: {}. Reconnecting...", wallet_address, e);
                }
            }

            if !first_connect {
                metrics.inc_user_ws_reconnects();
            }
            first_connect = false;

            // Wait for reconnect delay OR shutdown signal
            tokio::select! {
                _ = sleep(RECONNECT_DELAY) => {}
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("[User WS] Shutdown during reconnect delay for {}", wallet_address);
                        break;
                    }
                }
            }
        }

        info!("[User WS] Stopped for wallet {}", wallet_address);
    }

    async fn connect_and_listen(
        api_key: &str,
        api_secret: &str,
        api_passphrase: &str,
        wallet_address: &str,
        db: &Arc<Database>,
        order_event_tx: &broadcast::Sender<OrderEvent>,
        metrics: &Metrics,
    ) -> Result<()> {
        let (ws_stream, _) = connect_async(USER_WS_URL)
            .await
            .context("Failed to connect to user WebSocket")?;

        info!("[User WS] Connected for {}", wallet_address);

        let (mut write, mut read) = ws_stream.split();

        // Send auth message
        let auth_msg = UserAuthMessage {
            auth: UserAuthCredentials {
                api_key: api_key.to_string(),
                secret: api_secret.to_string(),
                passphrase: api_passphrase.to_string(),
            },
            msg_type: "user".to_string(),
        };

        let auth_json = serde_json::to_string(&auth_msg)?;
        write
            .send(tokio_tungstenite::tungstenite::Message::Text(auth_json))
            .await
            .context("Failed to send auth message")?;

        // Listen for messages
        while let Some(msg) = read.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    if let Err(e) = Self::handle_message(
                        &text,
                        wallet_address,
                        db,
                        order_event_tx,
                        metrics,
                    )
                    .await
                    {
                        debug!("[User WS] Error handling message: {}", e);
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Ping(data)) => {
                    let _ = write
                        .send(tokio_tungstenite::tungstenite::Message::Pong(data))
                        .await;
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => {
                    info!("[User WS] Server closed connection for {}", wallet_address);
                    break;
                }
                Err(e) => {
                    warn!("[User WS] WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn handle_message(
        text: &str,
        wallet_address: &str,
        db: &Arc<Database>,
        order_event_tx: &broadcast::Sender<OrderEvent>,
        metrics: &Metrics,
    ) -> Result<()> {
        let msg: UserWsMessage = serde_json::from_str(text)
            .context("Failed to parse user WS message")?;

        // Skip connection confirmation messages
        if msg.msg_type.as_deref() == Some("connected") {
            info!("[User WS] Authenticated for {}", wallet_address);
            return Ok(());
        }

        let order_id = match &msg.order_id {
            Some(id) => id.clone(),
            None => return Ok(()), // No order_id, skip
        };

        let event_type = msg.event_type.as_deref().unwrap_or("unknown");
        let status = msg.status.as_deref().unwrap_or("unknown");

        debug!(
            "[User WS] Event: {} order={} status={}",
            event_type, order_id, status
        );

        // Map CLOB status to our lifecycle status
        let lifecycle_status = match status {
            "LIVE" | "live" => Some(OrderLifecycleStatus::Live),
            "MATCHED" | "matched" => Some(OrderLifecycleStatus::Matched),
            "MINED" | "mined" => Some(OrderLifecycleStatus::Mined),
            "CONFIRMED" | "confirmed" => Some(OrderLifecycleStatus::Confirmed),
            "CANCELLED" | "cancelled" => Some(OrderLifecycleStatus::Cancelled),
            "FAILED" | "failed" => Some(OrderLifecycleStatus::Failed),
            _ => None,
        };

        // Update order in database if we have a lifecycle status
        if let Some(new_status) = lifecycle_status {
            let fill_size = msg.size.as_ref().and_then(|s| Decimal::from_str(s).ok());
            let fill_price = msg.price.as_ref().and_then(|s| Decimal::from_str(s).ok());

            // Track order outcomes in metrics
            match new_status {
                OrderLifecycleStatus::Matched | OrderLifecycleStatus::Confirmed => {
                    metrics.inc_orders_filled();
                }
                OrderLifecycleStatus::Cancelled => {
                    metrics.inc_orders_cancelled();
                }
                OrderLifecycleStatus::Failed => {
                    metrics.inc_orders_failed();
                }
                _ => {}
            }

            if let Err(e) = db
                .update_order_status(&order_id, new_status, fill_size, fill_price)
                .await
            {
                debug!("[User WS] Failed to update order {}: {}", order_id, e);
            }
        }

        // Broadcast the event for frontend WebSocket clients
        let event = OrderEvent {
            order_id,
            event_type: event_type.to_string(),
            status: status.to_string(),
            fill_price: msg.price,
            fill_size: msg.size,
            token_id: msg.token_id,
            timestamp: msg.timestamp.unwrap_or_else(|| Utc::now().timestamp_millis()),
        };

        let _ = order_event_tx.send(event);

        Ok(())
    }
}
