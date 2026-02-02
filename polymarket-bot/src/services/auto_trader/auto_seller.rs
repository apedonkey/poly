//! Auto-Seller - executes sell orders from position monitor triggers
//!
//! Receives sell signals and places market sell orders via CLOB API

use super::key_store::KeyStore;
use super::position_monitor::SellSignal;
use super::types::AutoTradeLog;
use crate::db::Database;
use anyhow::{Context, Result};
use chrono::Utc;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use alloy::primitives::U256;
use alloy::signers::{local::PrivateKeySigner, Signer};
use polymarket_client_sdk::clob::{Client as ClobClient, Config as ClobConfig};
use polymarket_client_sdk::clob::types::{Amount, OrderType, Side as ClobSide};

const POLYGON_CHAIN_ID: u64 = 137;
const CLOB_ENDPOINT: &str = "https://clob.polymarket.com";

/// Auto-Seller service
pub struct AutoSeller {
    db: Arc<Database>,
    key_store: KeyStore,
}

impl AutoSeller {
    pub fn new(db: Arc<Database>, key_store: KeyStore) -> Self {
        Self { db, key_store }
    }

    /// Run the auto-seller, processing sell signals
    pub async fn run(&self, mut signal_rx: mpsc::Receiver<SellSignal>) {
        info!("Auto-seller started");

        while let Some(signal) = signal_rx.recv().await {
            if let Err(e) = self.process_signal(&signal).await {
                error!(
                    "Failed to process sell signal for position {}: {}",
                    signal.position_id, e
                );
            }
        }

        info!("Auto-seller shutting down");
    }

    /// Process a sell signal
    async fn process_signal(&self, signal: &SellSignal) -> Result<()> {
        info!(
            "[Auto-Sell] Processing {} for position {} at price {}",
            signal.trigger.action_name(),
            signal.position_id,
            signal.current_price
        );

        // Get the position to check if it's paper or live
        let position = self
            .db
            .get_position_by_id_internal(signal.position_id)
            .await?
            .context("Position not found")?;

        // Calculate PnL
        let pnl = (signal.current_price - position.entry_price) * signal.size;

        // Execute live sell
        self.execute_sell(signal, &position, pnl).await?;

        Ok(())
    }

    /// Execute sell order via CLOB API
    async fn execute_sell(&self, signal: &SellSignal, position: &crate::types::Position, pnl: Decimal) -> Result<()> {
        info!(
            "[Auto-Sell] Executing sell for position {} at {} (PnL: ${:.2})",
            signal.position_id, signal.current_price, pnl
        );

        // Get the decrypted key from the key store
        let private_key = self.key_store.get_key(&signal.wallet_address).await;

        match private_key {
            Some(key) => {
                // Get token ID from position
                let token_id = position.token_id.as_ref()
                    .context("Position missing token_id for sell")?;

                info!(
                    "[Auto-Sell] Submitting sell order to CLOB for position {}",
                    signal.position_id
                );

                // Calculate shares from size and entry price
                let shares = signal.size / position.entry_price;

                // Create signer from private key
                let signer: PrivateKeySigner = key.parse()
                    .context("Failed to parse private key")?;
                let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

                // Create CLOB client and authenticate
                let clob_config = ClobConfig::builder().use_server_time(true).build();
                let client = ClobClient::new(CLOB_ENDPOINT, clob_config)
                    .context("Failed to create CLOB client")?
                    .authentication_builder(&signer)
                    .authenticate()
                    .await
                    .context("Failed to authenticate with CLOB")?;

                // Convert token_id to U256
                let token_id_u256 = U256::from_str_radix(token_id, 10)
                    .context("Failed to parse token ID")?;

                // Create sell order
                let order = client
                    .market_order()
                    .token_id(token_id_u256)
                    .amount(Amount::shares(shares).context("Failed to create shares amount")?)
                    .side(ClobSide::Sell)
                    .order_type(OrderType::FOK)
                    .build()
                    .await
                    .context("Failed to build sell order")?;

                // Sign and submit
                let signed_order = client
                    .sign(&signer, order)
                    .await
                    .context("Failed to sign order")?;

                let response = client
                    .post_order(signed_order)
                    .await
                    .context("Failed to submit sell order")?;

                let order_id = format!("{:?}", response);
                info!("[Auto-Sell] Order submitted: {}", order_id);

                // Close the position
                self.db
                    .close_position(signal.position_id, signal.current_price, Some(&order_id))
                    .await?;

                // Log the auto-trade
                let log = AutoTradeLog {
                    id: None,
                    wallet_address: signal.wallet_address.clone(),
                    position_id: Some(signal.position_id),
                    action: signal.trigger.action_name(),
                    market_question: Some(signal.market_question.clone()),
                    side: Some("Sell".to_string()),
                    entry_price: Some(position.entry_price),
                    exit_price: Some(signal.current_price),
                    size: Some(signal.size),
                    pnl: Some(pnl),
                    trigger_reason: Some(signal.trigger.reason()),
                    created_at: Utc::now(),
                };
                self.db.log_auto_trade(&log).await?;

                info!(
                    "[Auto-Sell] Position {} closed: {} at {} (PnL: ${:.2})",
                    signal.position_id,
                    signal.trigger.action_name(),
                    signal.current_price,
                    pnl
                );
            }
            None => {
                warn!(
                    "[Auto-Sell] No key in KeyStore for wallet {}. Auto-trading may not be enabled.",
                    signal.wallet_address
                );
            }
        }

        Ok(())
    }
}
