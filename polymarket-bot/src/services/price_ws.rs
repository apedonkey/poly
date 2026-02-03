//! Real-time price WebSocket connection to Polymarket CLOB
//!
//! Connects to Polymarket's WebSocket to receive live price updates for:
//! - Sniper opportunity tokens (time-sensitive trading signals)
//! - Open position tokens (user's active holdings)

use crate::services::metrics::Metrics;
use crate::services::tick_size::TickSizeCache;
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
/// Maximum bid-ask spread before mid-price becomes meaningless.
/// A 40c spread on a binary market means the price is ~50c regardless of true value.
const MAX_SPREAD: Decimal = Decimal::from_parts(40, 0, 0, false, 2); // 0.40

/// Price update message sent to frontend
#[derive(Debug, Clone, serde::Serialize)]
pub struct PriceUpdate {
    pub token_id: String,
    pub price: String,
    /// Best bid price from the orderbook (if available from book/price_change events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_bid: Option<String>,
}

/// Broadcast sender for price updates to WebSocket clients
pub type PriceUpdateTx = broadcast::Sender<PriceUpdate>;

/// Real-time price WebSocket client for Polymarket CLOB
pub struct PriceWebSocket;

impl PriceWebSocket {
    /// Run the price WebSocket, receiving token IDs from the scanner
    /// and broadcasting price updates to connected clients.
    /// Always reconnects after disconnection until the token channel closes.
    pub async fn run(
        mut token_rx: mpsc::Receiver<Vec<String>>,
        opportunities: Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: broadcast::Sender<Vec<Opportunity>>,
        price_tx: PriceUpdateTx,
        tick_size_cache: Arc<TickSizeCache>,
        metrics: Metrics,
    ) {
        let mut current_tokens: HashSet<String> = HashSet::new();

        loop {
            // Wait for tokens if we don't have any yet
            if current_tokens.is_empty() {
                match token_rx.recv().await {
                    Some(t) => {
                        current_tokens = t.into_iter().collect();
                        if current_tokens.is_empty() {
                            continue;
                        }
                    }
                    None => {
                        info!("Token channel closed, shutting down price WebSocket");
                        break;
                    }
                }
            }

            info!(
                "Subscribing to {} tokens for real-time prices",
                current_tokens.len()
            );

            // Connect and run until tokens change, error, or server close
            match Self::run_connection(
                &current_tokens,
                &mut token_rx,
                &opportunities,
                &opportunity_tx,
                &price_tx,
                &tick_size_cache,
            )
            .await
            {
                Ok(Some(new_tokens)) => {
                    // Token set changed — update and reconnect immediately
                    current_tokens = new_tokens;
                }
                Ok(None) => {
                    // Server closed connection — reconnect with same tokens after brief delay
                    info!("Price WebSocket closed by server, reconnecting...");
                    metrics.inc_price_ws_reconnects();
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    // Drain any pending token updates before reconnecting
                    while let Ok(tokens) = token_rx.try_recv() {
                        let new_set: HashSet<String> = tokens.into_iter().collect();
                        if !new_set.is_empty() {
                            current_tokens = new_set;
                        }
                    }
                }
                Err(e) => {
                    // Connection error — reconnect after delay
                    warn!("Price WebSocket error: {}, reconnecting...", e);
                    metrics.inc_price_ws_reconnects();
                    tokio::time::sleep(RECONNECT_DELAY).await;
                    // Drain any pending token updates before reconnecting
                    while let Ok(tokens) = token_rx.try_recv() {
                        let new_set: HashSet<String> = tokens.into_iter().collect();
                        if !new_set.is_empty() {
                            current_tokens = new_set;
                        }
                    }
                }
            }
        }
    }

    /// Run a single WebSocket connection.
    /// Returns `Ok(Some(new_tokens))` when token set changed (reconnect with new tokens),
    /// `Ok(None)` when server closed connection (reconnect with same tokens),
    /// or `Err` on connection error.
    async fn run_connection(
        tokens: &HashSet<String>,
        token_rx: &mut mpsc::Receiver<Vec<String>>,
        opportunities: &Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: &broadcast::Sender<Vec<Opportunity>>,
        price_tx: &PriceUpdateTx,
        tick_size_cache: &Arc<TickSizeCache>,
    ) -> Result<Option<HashSet<String>>> {
        let (ws_stream, _) = connect_async(WS_URL).await?;
        let (mut write, mut read) = ws_stream.split();

        info!("Price WebSocket connected to Polymarket CLOB");

        // Subscribe to tokens with initial_dump to receive current state
        let subscribe_msg = json!({
            "assets_ids": tokens.iter().collect::<Vec<_>>(),
            "type": "market",
            "initial_dump": true
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
                            Self::handle_message(&text, opportunities, opportunity_tx, price_tx, tick_size_cache).await;
                        }
                        Message::Close(_) => {
                            return Ok(None);
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
                        return Ok(Some(new_set));
                    }
                }
            }
        }
    }

    /// Extract a Decimal from a JSON value (handles both string "0.45" and number 0.45)
    fn json_to_decimal(v: &serde_json::Value) -> Option<Decimal> {
        v.as_str()
            .and_then(|s| Decimal::from_str(s).ok())
            .or_else(|| v.as_f64().and_then(|n| Decimal::try_from(n).ok()))
    }

    /// Handle a message from Polymarket market WebSocket
    async fn handle_message(
        text: &str,
        opportunities: &Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: &broadcast::Sender<Vec<Opportunity>>,
        price_tx: &PriceUpdateTx,
        tick_size_cache: &Arc<TickSizeCache>,
    ) {
        let msg: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };

        // Handle arrays of events (Polymarket sometimes sends batched messages)
        if let Some(arr) = msg.as_array() {
            for item in arr {
                if let Ok(item_str) = serde_json::to_string(item) {
                    // Recursively handle each item (box the future to avoid deep stack)
                    Box::pin(Self::handle_message(
                        &item_str,
                        opportunities,
                        opportunity_tx,
                        price_tx,
                        tick_size_cache,
                    ))
                    .await;
                }
            }
            return;
        }

        let event_type = msg.get("event_type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "tick_size_change" => {
                // A token's tick size changed (price crossed 0.96 or 0.04 threshold)
                if let (Some(asset_id), Some(tick_size)) = (
                    msg.get("asset_id").and_then(|v| v.as_str()),
                    msg.get("tick_size").and_then(|v| v.as_str()),
                ) {
                    if let Ok(ts) = Decimal::from_str(tick_size) {
                        tick_size_cache.update_tick_size(asset_id, ts).await;
                        info!("Tick size changed for {}: {}", asset_id, tick_size);
                    }
                }
            }
            "market_resolved" => {
                if let Some(market_id) = msg.get("market").and_then(|v| v.as_str()) {
                    info!("[Price WS] Market resolved: {}", market_id);
                    let mut opps = opportunities.write().await;
                    let before = opps.len();
                    opps.retain(|o| o.market_id != market_id);
                    if opps.len() < before {
                        let _ = opportunity_tx.send(opps.clone());
                        info!(
                            "[Price WS] Removed resolved market {} from opportunities",
                            market_id
                        );
                    }
                }
            }
            "new_market" => {
                if let Some(market_id) = msg.get("market").and_then(|v| v.as_str()) {
                    info!("[Price WS] New market detected: {}", market_id);
                }
            }
            "book" => {
                // Orderbook snapshot — extract best bid/ask from top of book
                Self::handle_book_event(&msg, opportunities, opportunity_tx, price_tx).await;
            }
            "last_trade_price" => {
                // Actual trade execution — broadcast price for position tracking
                if let (Some(asset_id), Some(price)) = (
                    msg.get("asset_id").and_then(|v| v.as_str()),
                    msg.get("price").and_then(|v| Self::json_to_decimal(v)),
                ) {
                    let _ = price_tx.send(PriceUpdate {
                        token_id: asset_id.to_string(),
                        price: price.to_string(),
                        best_bid: None,
                    });
                }
            }
            "price_change" => {
                Self::handle_price_change(&msg, opportunities, opportunity_tx, price_tx).await;
            }
            _ => {
                debug!("[Price WS] Unhandled event type: {}", event_type);
            }
        }
    }

    /// Handle a `book` event — orderbook snapshot with bids/asks arrays
    async fn handle_book_event(
        msg: &serde_json::Value,
        opportunities: &Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: &broadcast::Sender<Vec<Opportunity>>,
        price_tx: &PriceUpdateTx,
    ) {
        let asset_id = match msg.get("asset_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => return,
        };
        let bids = msg.get("bids").and_then(|v| v.as_array());
        let asks = msg.get("asks").and_then(|v| v.as_array());

        let (Some(bids), Some(asks)) = (bids, asks) else {
            return;
        };

        // Best bid = first bid (highest price), best ask = first ask (lowest price)
        let best_bid = bids
            .first()
            .and_then(|b| b.get("price"))
            .and_then(|v| Self::json_to_decimal(v));
        let best_ask = asks
            .first()
            .and_then(|a| a.get("price"))
            .and_then(|v| Self::json_to_decimal(v));

        if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
            if bid > Decimal::ZERO && ask > Decimal::ZERO {
                let spread = ask - bid;
                if spread > MAX_SPREAD {
                    debug!("Skipping price update for {}: spread too wide ({:.2})", asset_id, spread);
                    return;
                }
                let mid_price = (bid + ask) / Decimal::from(2);
                Self::apply_price_update(asset_id, mid_price, Some(bid), opportunities, opportunity_tx, price_tx)
                    .await;
            }
        }
    }

    /// Handle a `price_change` event — batch of price changes with best_bid/best_ask
    async fn handle_price_change(
        msg: &serde_json::Value,
        opportunities: &Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: &broadcast::Sender<Vec<Opportunity>>,
        price_tx: &PriceUpdateTx,
    ) {
        let price_changes = match msg.get("price_changes").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return,
        };

        let mut opps = opportunities.write().await;
        let mut opportunities_changed = false;

        for change in price_changes {
            let asset_id = match change.get("asset_id").and_then(|v| v.as_str()) {
                Some(id) => id,
                None => continue,
            };
            let best_bid = change.get("best_bid").and_then(|v| Self::json_to_decimal(v));
            let best_ask = change.get("best_ask").and_then(|v| Self::json_to_decimal(v));

            // Prefer mid-price from best_bid/best_ask, fall back to trade price
            let mid_price = match (best_bid, best_ask) {
                (Some(bid), Some(ask)) if bid > Decimal::ZERO && ask > Decimal::ZERO => {
                    let spread = ask - bid;
                    if spread > MAX_SPREAD {
                        debug!("Skipping price update for {}: spread too wide ({:.2})", asset_id, spread);
                        continue;
                    }
                    (bid + ask) / Decimal::from(2)
                }
                _ => {
                    // Fallback: use the trade price if bid/ask aren't available
                    match change.get("price").and_then(|v| Self::json_to_decimal(v)) {
                        Some(p) if p > Decimal::ZERO => p,
                        _ => continue,
                    }
                }
            };

            // Update matching opportunities with real-time recalculation
            for opp in opps.iter_mut() {
                if opp.token_id.as_deref() == Some(asset_id) {
                    let old_price = opp.entry_price;
                    if old_price != mid_price {
                        let was_valid = opp.meets_criteria;
                        let now_valid = opp.recalculate_with_price(mid_price);
                        opportunities_changed = true;

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
                            "Price update: {} {:.2}->{:.2} edge={:.1}%",
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
                best_bid: best_bid.map(|b| b.to_string()),
            });

            debug!(
                "Price update: {} mid={}",
                asset_id, mid_price
            );
        }

        // Re-sort by edge if any changes occurred (active ones first, then by edge)
        if opportunities_changed {
            opps.sort_by(|a, b| {
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

    /// Apply a single price update for one asset — used by `book` events
    async fn apply_price_update(
        asset_id: &str,
        mid_price: Decimal,
        best_bid: Option<Decimal>,
        opportunities: &Arc<RwLock<Vec<Opportunity>>>,
        opportunity_tx: &broadcast::Sender<Vec<Opportunity>>,
        price_tx: &PriceUpdateTx,
    ) {
        let mut opps = opportunities.write().await;
        let mut changed = false;

        for opp in opps.iter_mut() {
            if opp.token_id.as_deref() == Some(asset_id) {
                let old_price = opp.entry_price;
                if old_price != mid_price {
                    let was_valid = opp.meets_criteria;
                    let now_valid = opp.recalculate_with_price(mid_price);
                    changed = true;

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
                        "Price update: {} {:.2}->{:.2} edge={:.1}%",
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
            best_bid: best_bid.map(|b| b.to_string()),
        });

        if changed {
            opps.sort_by(|a, b| {
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
