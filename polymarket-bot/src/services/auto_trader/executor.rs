//! Auto-trading order executor
//!
//! Executes buy and sell orders for auto-trading using stored wallet credentials

use crate::db::Database;
use crate::types::Opportunity;
use crate::wallet::decrypt_private_key;
use anyhow::{anyhow, Context, Result};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;

use alloy::primitives::U256;
use alloy::signers::{local::PrivateKeySigner, Signer};
use polymarket_client_sdk::clob::{Client as ClobClient, Config as ClobConfig};
use polymarket_client_sdk::clob::types::{Amount, OrderType, Side as ClobSide};

const POLYGON_CHAIN_ID: u64 = 137;
const CLOB_ENDPOINT: &str = "https://clob.polymarket.com";

/// Auto-trading executor for CLOB orders
pub struct AutoTradingExecutor {
    db: Arc<Database>,
}

impl AutoTradingExecutor {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Execute a buy order for an opportunity
    pub async fn execute_buy(
        &self,
        wallet_address: &str,
        opportunity: &Opportunity,
        size: Decimal,
        password: &str,
    ) -> Result<BuyResult> {
        let token_id = opportunity
            .token_id
            .as_ref()
            .ok_or_else(|| anyhow!("Token ID required for trading"))?;

        // Get encrypted key and decrypt
        let encrypted_key = self
            .db
            .get_encrypted_key(wallet_address)
            .await?
            .ok_or_else(|| anyhow!("No encrypted key found for wallet"))?;

        let private_key = decrypt_private_key(&encrypted_key, password)
            .context("Failed to decrypt private key")?;

        info!(
            "[Auto-Buy] Executing {} {} at {:.0}c for ${} - {}",
            opportunity.side,
            token_id,
            opportunity.entry_price * Decimal::from(100),
            size,
            opportunity.short_question(40)
        );

        // Create signer
        let signer: PrivateKeySigner = private_key
            .parse()
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

        info!("[Auto-Buy] Order submitted: {:?}", response);

        // Get order ID from response
        let order_id = format!("{:?}", response);

        Ok(BuyResult {
            order_id,
            token_id: token_id.clone(),
            size,
            price: opportunity.entry_price,
        })
    }

    /// Execute a sell order for a position
    pub async fn execute_sell(
        &self,
        wallet_address: &str,
        token_id: &str,
        shares: Decimal,
        password: &str,
    ) -> Result<SellResult> {
        // Get encrypted key and decrypt
        let encrypted_key = self
            .db
            .get_encrypted_key(wallet_address)
            .await?
            .ok_or_else(|| anyhow!("No encrypted key found for wallet"))?;

        let private_key = decrypt_private_key(&encrypted_key, password)
            .context("Failed to decrypt private key")?;

        info!(
            "[Auto-Sell] Executing sell of {} shares for token {}",
            shares, token_id
        );

        // Create signer
        let signer: PrivateKeySigner = private_key
            .parse()
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

        info!("[Auto-Sell] Order submitted: {:?}", response);

        let order_id = format!("{:?}", response);

        Ok(SellResult {
            order_id,
            token_id: token_id.to_string(),
            shares,
        })
    }
}

/// Result of a buy execution
#[derive(Debug)]
pub struct BuyResult {
    pub order_id: String,
    pub token_id: String,
    pub size: Decimal,
    pub price: Decimal,
}

/// Result of a sell execution
#[derive(Debug)]
pub struct SellResult {
    pub order_id: String,
    pub token_id: String,
    pub shares: Decimal,
}
