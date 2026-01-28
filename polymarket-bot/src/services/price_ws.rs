//! Real-time price WebSocket connection to Polymarket CLOB
//!
//! Connects to Polymarket's WebSocket to receive live price updates for:
//! - Sniper opportunity tokens (time-sensitive trading signals)
//! - Open position tokens (user's active holdings)

use crate::types::Opportunity;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use serde_json::json;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

const WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
const PING_INTERVAL: Duration = Duration::from_secs(10);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

/// Price update message sent to frontend
#[derive(Debug, Clone, serde::Serialize)]
pub struct PriceUpdate {
    pub token_id: String,
    pub price: String,
}

/// Broadcast sender for price updates to WebSocket clients
pub type PriceUpdateTx = broadcast::Sender<PriceUpdate>;

/// Real-time price WebSocket client for Polymarket CLOB
pub struct PriceWebSocket;

impl PriceWebSocket {
    /// Run the price WebSocket, receiving token IDs from the scanner
    /// and broadcasting price updates to connected clients
    pub async fn run(
        mut token_rx: mpsc::Receiver<Vec<String>>,
        opportunities: Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: broadcast::Sender<Vec<Opportunity>>,
        price_tx: PriceUpdateTx,
    ) {
        let mut current_tokens: HashSet<String> = HashSet::new();

        loop {
            // Wait for initial tokens or token update
            let tokens = match token_rx.recv().await {
                Some(t) => t,
                None => {
                    info!("Token channel closed, shutting down price WebSocket");
                    break;
                }
            };

            let new_tokens: HashSet<String> = tokens.into_iter().collect();

            // Skip if no change and we have an active connection
            if new_tokens == current_tokens && !current_tokens.is_empty() {
                continue;
            }

            current_tokens = new_tokens;

            // If no tokens, wait for next update
            if current_tokens.is_empty() {
                debug!("No tokens to subscribe, waiting for opportunities");
                continue;
            }

            info!(
                "Subscribing to {} tokens for real-time prices",
                current_tokens.len()
            );

            // Connect and run until tokens change or error
            if let Err(e) = Self::run_connection(
                &current_tokens,
                &mut token_rx,
                &opportunities,
                &opportunity_tx,
                &price_tx,
            )
            .await
            {
                warn!("Price WebSocket error: {}, reconnecting...", e);
                tokio::time::sleep(RECONNECT_DELAY).await;
            }
        }
    }

    /// Run a single WebSocket connection until it needs to be refreshed
    async fn run_connection(
        tokens: &HashSet<String>,
        token_rx: &mut mpsc::Receiver<Vec<String>>,
        opportunities: &Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: &broadcast::Sender<Vec<Opportunity>>,
        price_tx: &PriceUpdateTx,
    ) -> Result<()> {
        let (ws_stream, _) = connect_async(WS_URL).await?;
        let (mut write, mut read) = ws_stream.split();

        info!("Price WebSocket connected to Polymarket CLOB");

        // Subscribe to tokens
        let subscribe_msg = json!({
            "assets_ids": tokens.iter().collect::<Vec<_>>(),
            "type": "market"
        });
        write
            .send(Message::Text(subscribe_msg.to_string()))
            .await?;

        debug!("Subscribed to {} token price feeds", tokens.len());

        let mut ping_interval = tokio::time::interval(PING_INTERVAL);
        ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                // Handle incoming messages from Polymarket
                Some(msg) = read.next() => {
                    match msg? {
                        Message::Text(text) => {
                            Self::handle_message(&text, opportunities, opportunity_tx, price_tx).await;
                        }
                        Message::Close(_) => {
                            info!("Price WebSocket closed by server");
                            return Ok(());
                        }
                        Message::Pong(_) => {
                            debug!("Received pong");
                        }
                        _ => {}
                    }
                }

                // Send ping to keep connection alive
                _ = ping_interval.tick() => {
                    write.send(Message::Ping(vec![])).await?;
                    debug!("Sent ping to keep connection alive");
                }

                // Check for token updates from scanner
                Some(new_tokens) = token_rx.recv() => {
                    let new_set: HashSet<String> = new_tokens.into_iter().collect();
                    if new_set != *tokens {
                        info!("Token set changed, reconnecting with new subscriptions");
                        return Ok(());  // Tokens changed, trigger reconnect
                    }
                }
            }
        }
    }

    /// Handle a price_change message from Polymarket
    async fn handle_message(
        text: &str,
        opportunities: &Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: &broadcast::Sender<Vec<Opportunity>>,
        price_tx: &PriceUpdateTx,
    ) {
        let msg: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };

        // Only process price_change events
        if msg.get("event_type").and_then(|v| v.as_str()) != Some("price_change") {
            return;
        }

        // Parse price_changes array (updated schema as of Sept 2025)
        let price_changes = match msg.get("price_changes").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return,
        };

        let mut opps = opportunities.write().await;
        let mut opportunities_changed = false;

        for change in price_changes {
            let asset_id = change.get("asset_id").and_then(|v| v.as_str());
            let best_bid = change.get("best_bid").and_then(|v| v.as_str());
            let best_ask = change.get("best_ask").and_then(|v| v.as_str());

            if let (Some(asset_id), Some(bid_str), Some(ask_str)) = (asset_id, best_bid, best_ask) {
                // Calculate mid price from best bid/ask
                let bid = Decimal::from_str(bid_str).unwrap_or_default();
                let ask = Decimal::from_str(ask_str).unwrap_or_default();
                let two = Decimal::from(2);
                let mid_price = (bid + ask) / two;

                // Update matching opportunities with real-time recalculation
                for opp in opps.iter_mut() {
                    if opp.token_id.as_deref() == Some(asset_id) {
                        let old_price = opp.entry_price;
                        if old_price != mid_price {
                            let was_valid = opp.meets_criteria;

                            // Recalculate all metrics (edge, return, recommendation)
                            // Updates meets_criteria field - opportunity stays in list either way
                            let now_valid = opp.recalculate_with_price(mid_price);

                            opportunities_changed = true;

                            // Only log paused/reactivated for Sniper SECTION opportunities
                            // (ResolutionSniper + NOT crypto + NOT sports + closing within 12h)
                            if opp.is_sniper_section() {
                                if was_valid && !now_valid {
                                    info!(
                                        "[Sniper] Paused: {} price={:.0}c",
                                        opp.question.chars().take(50).collect::<String>(),
                                        mid_price * Decimal::from(100)
                                    );
                                } else if !was_valid && now_valid {
                                    info!(
                                        "[Sniper] Reactivated: {} price={:.0}c edge={:.1}%",
                                        opp.question.chars().take(50).collect::<String>(),
                                        mid_price * Decimal::from(100),
                                        opp.edge * 100.0
                                    );
                                }
                            }

                            debug!(
                                "Price update: {} {:.2}→{:.2} edge={:.1}%",
                                opp.question.chars().take(30).collect::<String>(),
                                old_price,
                                mid_price,
                                opp.edge * 100.0
                            );
                        }
                    }
                }

                // Broadcast price update to frontend for positions
                let _ = price_tx.send(PriceUpdate {
                    token_id: asset_id.to_string(),
                    price: mid_price.to_string(),
                });

                debug!(
                    "Price update: {} bid={} ask={} mid={}",
                    asset_id, bid_str, ask_str, mid_price
                );
            }
        }

        // Re-sort by edge if any changes occurred (active ones first, then by edge)
        if opportunities_changed {
            opps.sort_by(|a, b| {
                // Active opportunities first
                match (a.meets_criteria, b.meets_criteria) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => b.edge
                        .partial_cmp(&a.edge)
                        .unwrap_or(std::cmp::Ordering::Equal),
                }
            });
            let _ = opportunity_tx.send(opps.clone());
        }
    }
}
