//! Auto-Buyer - automatically buys opportunities based on user settings
//!
//! Listens to new opportunities and executes buys when:
//! - Auto-buy is enabled for the wallet
//! - Opportunity matches configured strategies (sniper, no_bias)
//! - Position limits and exposure limits are not exceeded
//! - Minimum edge threshold is met

use super::key_store::KeyStore;
use super::types::AutoTradeLog;
use crate::db::Database;
use crate::types::Opportunity;
use anyhow::{Context, Result};
use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};

use alloy::primitives::U256;
use alloy::signers::{local::PrivateKeySigner, Signer};
use polymarket_client_sdk::clob::{Client as ClobClient, Config as ClobConfig};
use polymarket_client_sdk::clob::types::{Amount, OrderType, Side as ClobSide};

const POLYGON_CHAIN_ID: u64 = 137;
const CLOB_ENDPOINT: &str = "https://clob.polymarket.com";

/// Auto-Buyer service
pub struct AutoBuyer {
    db: Arc<Database>,
    key_store: KeyStore,
    /// Shared list of current opportunities
    opportunities: Arc<RwLock<Vec<Opportunity>>>,
}

impl AutoBuyer {
    pub fn new(db: Arc<Database>, key_store: KeyStore, opportunities: Arc<RwLock<Vec<Opportunity>>>) -> Self {
        Self { db, key_store, opportunities }
    }

    /// Run the auto-buyer, listening for opportunity updates
    pub async fn run(&self, mut opportunity_rx: broadcast::Receiver<Vec<Opportunity>>) {
        info!("Auto-buyer started");

        loop {
            match opportunity_rx.recv().await {
                Ok(opportunities) => {
                    if let Err(e) = self.process_opportunities(&opportunities).await {
                        warn!("Error processing opportunities: {}", e);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!("Auto-buyer lagged {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("Opportunity channel closed, shutting down auto-buyer");
                    break;
                }
            }
        }
    }

    /// Process new opportunities and execute auto-buys where appropriate
    async fn process_opportunities(&self, opportunities: &[Opportunity]) -> Result<()> {
        // Get all wallets with auto-buy enabled
        let wallets = self.db.get_auto_buy_enabled_wallets().await?;

        if wallets.is_empty() {
            return Ok(());
        }

        for wallet_address in wallets {
            if let Err(e) = self.check_opportunities_for_wallet(&wallet_address, opportunities).await {
                warn!("Error checking opportunities for wallet {}: {}", wallet_address, e);
            }
        }

        Ok(())
    }

    /// Check opportunities for a specific wallet and execute buys
    async fn check_opportunities_for_wallet(
        &self,
        wallet_address: &str,
        opportunities: &[Opportunity],
    ) -> Result<()> {
        let settings = self.db.get_auto_trading_settings(wallet_address).await?;

        if !settings.enabled || !settings.auto_buy_enabled {
            return Ok(());
        }

        // Check position limits
        let open_count = self.db.count_open_positions(wallet_address).await?;
        if open_count >= settings.max_positions {
            debug!(
                "Wallet {} at max positions ({}/{})",
                wallet_address, open_count, settings.max_positions
            );
            return Ok(());
        }

        // Check exposure limits
        let current_exposure = self.db.get_total_exposure(wallet_address).await?;
        let max_exposure = settings.max_total_exposure;
        if current_exposure >= max_exposure {
            debug!(
                "Wallet {} at max exposure ({}/{})",
                wallet_address, current_exposure, max_exposure
            );
            return Ok(());
        }

        // Check daily loss limit
        let daily_pnl = self.db.get_daily_auto_pnl(wallet_address).await?;
        if daily_pnl <= -settings.max_daily_loss {
            debug!(
                "Wallet {} hit daily loss limit (${} vs -${})",
                wallet_address, daily_pnl, settings.max_daily_loss
            );
            return Ok(());
        }

        // Find matching opportunities
        for opp in opportunities {
            // Check if already has position in this market
            if self.db.has_open_position(wallet_address, &opp.market_id).await? {
                continue;
            }

            // Check if strategy is enabled
            let strategy_enabled = match opp.strategy {
                crate::types::StrategyType::ResolutionSniper => settings.strategies.contains(&"sniper".to_string()),
                crate::types::StrategyType::NoBias => settings.strategies.contains(&"no_bias".to_string()),
            };

            if !strategy_enabled {
                continue;
            }

            // Check minimum edge
            if opp.edge < settings.min_edge as f64 {
                continue;
            }

            // Check meets_criteria (opportunity is still valid)
            if !opp.meets_criteria {
                continue;
            }

            // Calculate position size (respecting limits)
            let available_exposure = max_exposure - current_exposure;
            let position_size = settings.max_position_size.min(available_exposure);

            if position_size <= Decimal::ZERO {
                continue;
            }

            // Get token_id for trading
            let token_id = match &opp.token_id {
                Some(t) => t.clone(),
                None => continue, // Can't trade without token_id
            };

            // Get the decrypted key from the key store
            let private_key = match self.key_store.get_key(wallet_address).await {
                Some(k) => k,
                None => {
                    debug!("No key in KeyStore for wallet {}", wallet_address);
                    continue;
                }
            };

            info!(
                "[Auto-Buy] {} {} {} at {:.0}c (edge: {:.1}%)",
                wallet_address,
                opp.side,
                opp.short_question(40),
                opp.entry_price * Decimal::from(100),
                opp.edge * 100.0
            );

            // Execute live buy order
            let order_id = match self.execute_buy(&private_key, &token_id, position_size).await {
                Ok(id) => Some(id),
                Err(e) => {
                    warn!("[Auto-Buy] Failed to execute buy: {}", e);
                    continue;
                }
            };

            // Create the position
            let position_id = self
                .db
                .create_position_for_wallet(
                    wallet_address,
                    &opp.market_id,
                    &opp.question,
                    Some(&opp.slug),
                    opp.side,
                    opp.entry_price,
                    position_size,
                    opp.strategy,
                    false, // Live trade
                    None,  // end_date
                    Some(&token_id),
                    order_id.as_deref(),
                )
                .await?;

            // Log the auto-buy
            let log = AutoTradeLog {
                id: None,
                wallet_address: wallet_address.to_string(),
                position_id: Some(position_id),
                action: "auto_buy".to_string(),
                market_question: Some(opp.question.clone()),
                side: Some(format!("{:?}", opp.side)),
                entry_price: Some(opp.entry_price),
                exit_price: None,
                size: Some(position_size),
                pnl: None,
                trigger_reason: Some(format!(
                    "Edge {:.1}% >= {:.1}% threshold",
                    opp.edge * 100.0,
                    settings.min_edge * 100.0
                )),
                created_at: Utc::now(),
            };
            self.db.log_auto_trade(&log).await?;

            info!(
                "[Auto-Buy] Created position {} for ${} on {}",
                position_id, position_size, opp.short_question(30)
            );

            // Only buy one opportunity per pass to avoid rapid-fire buys
            break;
        }

        Ok(())
    }

    /// Execute a buy order via CLOB API
    async fn execute_buy(&self, private_key: &str, token_id: &str, size: Decimal) -> Result<String> {
        // Create signer from private key
        let signer: PrivateKeySigner = private_key.parse()
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

        // Create buy order
        let order = client
            .market_order()
            .token_id(token_id_u256)
            .amount(Amount::usdc(size).context("Failed to create USDC amount")?)
            .side(ClobSide::Buy)
            .order_type(OrderType::FOK)
            .build()
            .await
            .context("Failed to build order")?;

        // Sign and submit
        let signed_order = client
            .sign(&signer, order)
            .await
            .context("Failed to sign order")?;

        let response = client
            .post_order(signed_order)
            .await
            .context("Failed to submit order")?;

        let order_id = format!("{:?}", response);
        info!("[Auto-Buy] Order submitted: {}", order_id);

        Ok(order_id)
    }
}
