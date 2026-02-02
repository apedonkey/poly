//! Dispute Sniper - automatically buys proposed outcomes with sufficient edge
//!
//! Listens to DisputeTracker broadcast alerts and:
//! - Buys the proposed outcome side when edge >= threshold (status = Proposed)
//! - Auto-exits if a dispute escalates from Proposed to Disputed/DvmVote

use super::key_store::KeyStore;
use super::position_monitor::SellSignal;
use super::types::{AutoTradeLog, ExitTrigger};
use crate::db::Database;
use crate::types::{DisputeAlert, DisputeStatus, Order, OrderLifecycleStatus, Side, StrategyType};
use anyhow::{Context, Result};
use chrono::Utc;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, info, warn};

use alloy::primitives::U256;
use alloy::signers::{local::PrivateKeySigner, Signer};
use polymarket_client_sdk::clob::{Client as ClobClient, Config as ClobConfig};
use polymarket_client_sdk::clob::types::{Amount, OrderType, Side as ClobSide};

const POLYGON_CHAIN_ID: u64 = 137;
const CLOB_ENDPOINT: &str = "https://clob.polymarket.com";
const USDC_ADDRESS: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
const MIN_TRADE_BALANCE: &str = "1.00";

/// Dispute Sniper service
pub struct DisputeSniper {
    db: Arc<Database>,
    key_store: KeyStore,
    polygon_rpc_url: String,
    /// Track last-seen status per assertion_id to detect escalations
    last_status: HashMap<String, DisputeStatus>,
}

impl DisputeSniper {
    pub fn new(db: Arc<Database>, key_store: KeyStore, polygon_rpc_url: String) -> Self {
        Self {
            db,
            key_store,
            polygon_rpc_url,
            last_status: HashMap::new(),
        }
    }

    /// Run the dispute sniper, listening for dispute alerts
    pub async fn run(
        mut self,
        mut dispute_rx: broadcast::Receiver<Vec<DisputeAlert>>,
        sell_tx: mpsc::Sender<SellSignal>,
    ) {
        info!("Dispute sniper started");

        loop {
            match dispute_rx.recv().await {
                Ok(alerts) => {
                    if let Err(e) = self.process_disputes(&alerts, &sell_tx).await {
                        warn!("Error processing disputes: {}", e);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!("Dispute sniper lagged {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Dispute channel closed, shutting down dispute sniper");
                    break;
                }
            }
        }
    }

    /// Process dispute alerts for all enabled wallets
    async fn process_disputes(
        &mut self,
        alerts: &[DisputeAlert],
        sell_tx: &mpsc::Sender<SellSignal>,
    ) -> Result<()> {
        let wallets = self.db.get_auto_trading_enabled_wallets().await?;

        if wallets.is_empty() {
            // Still update last_status for tracking even if no wallets
            for alert in alerts {
                self.last_status.insert(alert.assertion_id.clone(), alert.dispute_status);
            }
            return Ok(());
        }

        for wallet_address in &wallets {
            if let Err(e) = self.process_for_wallet(wallet_address, alerts, sell_tx).await {
                warn!("[Dispute Sniper] Error for wallet {}: {}", wallet_address, e);
            }
        }

        // Update last_status after processing
        for alert in alerts {
            self.last_status.insert(alert.assertion_id.clone(), alert.dispute_status);
        }

        Ok(())
    }

    /// Process dispute alerts for a specific wallet
    async fn process_for_wallet(
        &self,
        wallet_address: &str,
        alerts: &[DisputeAlert],
        sell_tx: &mpsc::Sender<SellSignal>,
    ) -> Result<()> {
        let settings = self.db.get_auto_trading_settings(wallet_address).await?;

        if !settings.enabled || !settings.dispute_sniper_enabled {
            return Ok(());
        }

        // Global limits check
        let open_count = self.db.count_open_positions(wallet_address).await?;
        if open_count >= settings.max_positions {
            debug!(
                "[Dispute Sniper] Wallet {} at max positions ({}/{})",
                wallet_address, open_count, settings.max_positions
            );
            return Ok(());
        }

        let current_exposure = self.db.get_total_exposure(wallet_address).await?;
        if current_exposure >= settings.max_total_exposure {
            debug!(
                "[Dispute Sniper] Wallet {} at max exposure ({}/{})",
                wallet_address, current_exposure, settings.max_total_exposure
            );
            return Ok(());
        }

        let daily_pnl = self.db.get_daily_auto_pnl(wallet_address).await?;
        if daily_pnl <= -settings.max_daily_loss {
            debug!(
                "[Dispute Sniper] Wallet {} hit daily loss limit (${} vs -${})",
                wallet_address, daily_pnl, settings.max_daily_loss
            );
            return Ok(());
        }

        let usdc_balance = self.fetch_usdc_balance(wallet_address).await.unwrap_or_else(|e| {
            warn!("Failed to fetch USDC balance for {}: {}. Skipping balance check.", wallet_address, e);
            Decimal::MAX
        });

        let min_balance = Decimal::from_str(MIN_TRADE_BALANCE).unwrap_or(Decimal::ONE);
        if usdc_balance < min_balance {
            debug!(
                "[Dispute Sniper] Wallet {} insufficient balance (${} < ${})",
                wallet_address, usdc_balance, min_balance
            );
            return Ok(());
        }

        for alert in alerts {
            // === BUY: Proposed status with sufficient edge ===
            if alert.dispute_status == DisputeStatus::Proposed {
                // Use expected value if available (accounts for 50-50 outcome), else raw edge
                let edge_f64 = alert.expected_value
                    .or(alert.edge)
                    .and_then(|e| e.to_string().parse::<f64>().ok())
                    .unwrap_or(0.0);

                // Round 2 re-proposals are higher conviction - reduce threshold by 20%
                let effective_threshold = if alert.dispute_round >= 2 {
                    settings.min_dispute_edge * 0.8
                } else {
                    settings.min_dispute_edge
                };

                if edge_f64 < effective_threshold {
                    debug!(
                        "[Dispute Sniper] Skipping {} - edge {:.1}% < threshold {:.1}% (round {})",
                        alert.question,
                        edge_f64 * 100.0,
                        effective_threshold * 100.0,
                        alert.dispute_round
                    );
                    continue;
                }

                // Determine buy side and token
                let (side, token_id, entry_price) = match alert.proposed_outcome.as_str() {
                    "Yes" => {
                        let tid = match &alert.yes_token_id {
                            Some(t) => t.clone(),
                            None => continue,
                        };
                        (Side::Yes, tid, alert.current_yes_price)
                    }
                    "No" => {
                        let tid = match &alert.no_token_id {
                            Some(t) => t.clone(),
                            None => continue,
                        };
                        (Side::No, tid, alert.current_no_price)
                    }
                    _ => {
                        debug!("[Dispute Sniper] Unknown proposed outcome: {}", alert.proposed_outcome);
                        continue;
                    }
                };

                // Check for existing dispute position
                if self.db.has_open_dispute_position(wallet_address, &alert.condition_id).await? {
                    debug!(
                        "[Dispute Sniper] Already has dispute position for {}",
                        alert.question
                    );
                    continue;
                }

                // Calculate position size
                let remaining_exposure = settings.max_total_exposure - current_exposure;
                let position_size = settings.dispute_position_size
                    .min(remaining_exposure)
                    .min(usdc_balance);

                if position_size <= Decimal::ZERO || position_size < min_balance {
                    debug!(
                        "[Dispute Sniper] Position size too small: ${}",
                        position_size
                    );
                    continue;
                }

                // Get the decrypted key
                let private_key = match self.key_store.get_key(wallet_address).await {
                    Some(k) => k,
                    None => {
                        debug!("[Dispute Sniper] No key in KeyStore for {}", wallet_address);
                        continue;
                    }
                };

                info!(
                    "[Dispute Sniper] BUY {} {} at {:.0}c (EV: {:.1}%, round: {}, bond: {:?}, dispute: {})",
                    side,
                    alert.question,
                    entry_price * Decimal::from(100),
                    edge_f64 * 100.0,
                    alert.dispute_round,
                    alert.proposer_bond,
                    alert.assertion_id
                );

                // Execute buy
                let order_id = match self.execute_buy(&private_key, &token_id, position_size).await {
                    Ok(id) => Some(id),
                    Err(e) => {
                        warn!("[Dispute Sniper] Failed to execute buy: {}", e);
                        continue;
                    }
                };

                // Create order lifecycle record (Item 5)
                if let Some(ref oid) = order_id {
                    let order = Order {
                        id: oid.clone(),
                        wallet_address: wallet_address.to_string(),
                        token_id: token_id.clone(),
                        market_id: Some(alert.condition_id.clone()),
                        side,
                        order_type: "FOK".to_string(),
                        price: entry_price,
                        original_size: position_size,
                        filled_size: position_size, // FOK = fully filled if returned
                        avg_fill_price: Some(entry_price),
                        status: OrderLifecycleStatus::Confirmed,
                        position_id: None, // Will be set after position creation
                        neg_risk: false,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                    };
                    if let Err(e) = self.db.create_order(&order).await {
                        warn!("[Dispute Sniper] Failed to create order record: {}", e);
                    }
                }

                // Create position with Dispute strategy
                let position_id = self
                    .db
                    .create_position_for_wallet(
                        wallet_address,
                        &alert.condition_id,
                        &alert.question,
                        Some(&alert.slug),
                        side,
                        entry_price,
                        position_size,
                        StrategyType::Dispute,
                        false, // Live trade
                        None,  // end_date
                        Some(&token_id),
                        order_id.as_deref(),
                        false, // neg_risk: disputes may vary
                    )
                    .await?;

                // Log the dispute snipe
                let log = AutoTradeLog {
                    id: None,
                    wallet_address: wallet_address.to_string(),
                    position_id: Some(position_id),
                    action: "dispute_snipe".to_string(),
                    market_question: Some(alert.question.clone()),
                    side: Some(format!("{:?}", side)),
                    entry_price: Some(entry_price),
                    exit_price: None,
                    size: Some(position_size),
                    pnl: None,
                    trigger_reason: Some(format!(
                        "Dispute EV {:.1}% >= {:.1}% threshold (proposed: {}, round: {}, bond: {:?})",
                        edge_f64 * 100.0,
                        effective_threshold * 100.0,
                        alert.proposed_outcome,
                        alert.dispute_round,
                        alert.proposer_bond
                    )),
                    created_at: Utc::now(),
                };
                self.db.log_auto_trade(&log).await?;

                info!(
                    "[Dispute Sniper] Created position {} for ${} on {}",
                    position_id, position_size, alert.question
                );

                // Only buy one per pass
                break;
            }

            // === EXIT: Escalation from Proposed to Disputed/DvmVote ===
            if (alert.dispute_status == DisputeStatus::Disputed
                || alert.dispute_status == DisputeStatus::DvmVote)
                && settings.dispute_exit_on_escalation
            {
                // Check if this is an escalation (was previously Proposed)
                let was_proposed = self.last_status
                    .get(&alert.assertion_id)
                    .map(|s| *s == DisputeStatus::Proposed)
                    .unwrap_or(false);

                if !was_proposed {
                    continue;
                }

                // Find open dispute position for this condition_id
                let positions = self.db.get_open_positions_for_wallet(wallet_address).await?;
                let dispute_position = positions.iter().find(|p| {
                    p.market_id == alert.condition_id && p.strategy == StrategyType::Dispute
                });

                if let Some(pos) = dispute_position {
                    let token_id = match &pos.token_id {
                        Some(t) => t.clone(),
                        None => {
                            warn!("[Dispute Sniper] Position {} has no token_id", pos.id);
                            continue;
                        }
                    };

                    // Determine current price for the position's side
                    let current_price = match pos.side {
                        Side::Yes => alert.current_yes_price,
                        Side::No => alert.current_no_price,
                    };

                    info!(
                        "[Dispute Sniper] EXIT position {} - dispute escalated to {} for {}",
                        pos.id, alert.dispute_status, alert.question
                    );

                    let signal = SellSignal {
                        position_id: pos.id,
                        wallet_address: wallet_address.to_string(),
                        token_id,
                        current_price,
                        trigger: ExitTrigger::DisputeEscalation {
                            price: current_price,
                            new_status: format!("{}", alert.dispute_status),
                        },
                        size: pos.size,
                        market_question: pos.question.clone(),
                    };

                    if sell_tx.send(signal).await.is_err() {
                        warn!("[Dispute Sniper] Failed to send sell signal - channel closed");
                    }

                    // Log the exit
                    let log = AutoTradeLog {
                        id: None,
                        wallet_address: wallet_address.to_string(),
                        position_id: Some(pos.id),
                        action: "dispute_exit".to_string(),
                        market_question: Some(alert.question.clone()),
                        side: Some(format!("{:?}", pos.side)),
                        entry_price: Some(pos.entry_price),
                        exit_price: Some(current_price),
                        size: Some(pos.size),
                        pnl: None, // Will be calculated by AutoSeller
                        trigger_reason: Some(format!(
                            "Dispute escalated from Proposed to {}",
                            alert.dispute_status
                        )),
                        created_at: Utc::now(),
                    };
                    self.db.log_auto_trade(&log).await?;
                }
            }
        }

        Ok(())
    }

    /// Fetch on-chain USDC balance for a wallet address
    async fn fetch_usdc_balance(&self, wallet_address: &str) -> Result<Decimal> {
        let padded_address = format!(
            "000000000000000000000000{}",
            wallet_address.trim_start_matches("0x")
        );
        let data = format!("0x70a08231{}", padded_address);

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [
                {
                    "to": USDC_ADDRESS,
                    "data": data
                },
                "latest"
            ],
            "id": 1
        });

        let client = reqwest::Client::new();
        let response: JsonRpcResponse = client
            .post(&self.polygon_rpc_url)
            .json(&request)
            .send()
            .await
            .context("Failed to send RPC request")?
            .json()
            .await
            .context("Failed to parse RPC response")?;

        let hex_balance = response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result in RPC response"))?;

        let balance_raw = u128::from_str_radix(hex_balance.trim_start_matches("0x"), 16)
            .unwrap_or(0);

        let whole = balance_raw / 1_000_000;
        let fraction = balance_raw % 1_000_000;
        let balance_str = format!("{}.{:06}", whole, fraction);

        Decimal::from_str(&balance_str).context("Failed to parse balance as Decimal")
    }

    /// Execute a buy order via CLOB API
    async fn execute_buy(&self, private_key: &str, token_id: &str, size: Decimal) -> Result<String> {
        let signer: PrivateKeySigner = private_key.parse()
            .context("Failed to parse private key")?;
        let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

        let clob_config = ClobConfig::builder().use_server_time(true).build();
        let client = ClobClient::new(CLOB_ENDPOINT, clob_config)
            .context("Failed to create CLOB client")?
            .authentication_builder(&signer)
            .authenticate()
            .await
            .context("Failed to authenticate with CLOB")?;

        let token_id_u256 = U256::from_str_radix(token_id, 10)
            .context("Failed to parse token ID")?;

        let order = client
            .market_order()
            .token_id(token_id_u256)
            .amount(Amount::usdc(size).context("Failed to create USDC amount")?)
            .side(ClobSide::Buy)
            .order_type(OrderType::FOK)
            .build()
            .await
            .context("Failed to build order")?;

        let signed_order = client
            .sign(&signer, order)
            .await
            .context("Failed to sign order")?;

        let response = client
            .post_order(signed_order)
            .await
            .context("Failed to submit order")?;

        let order_id = format!("{:?}", response);
        info!("[Dispute Sniper] Order submitted: {}", order_id);

        Ok(order_id)
    }
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<String>,
}
