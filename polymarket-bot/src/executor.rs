//! Order execution for Polymarket trades

use crate::config::Config;
use crate::db::Database;
use crate::types::{Opportunity, Side};
use anyhow::{anyhow, Context, Result};
use rust_decimal::Decimal;
use tracing::{debug, info};

// Polymarket SDK imports
use alloy::primitives::U256;
use alloy::signers::{local::PrivateKeySigner, Signer};
use polymarket_client_sdk::clob::{
    Client as ClobClient, Config as ClobConfig,
};
use polymarket_client_sdk::clob::types::{
    Amount, OrderType, Side as ClobSide,
};

/// Polygon chain ID for signing
const POLYGON_CHAIN_ID: u64 = 137;

/// CLOB API endpoint
const CLOB_ENDPOINT: &str = "https://clob.polymarket.com";

/// Order executor handles placing trades (paper or real)
pub struct Executor {
    config: Config,
    db: Database,
}

impl Executor {
    pub fn new(config: Config, db: Database) -> Self {
        Self { config, db }
    }

    /// Execute an opportunity (paper trade or real)
    pub async fn execute(&self, opportunity: &Opportunity) -> Result<ExecutionResult> {
        // Check if we already have a position in this market
        if let Some(existing) = self.db.get_position_by_market(&opportunity.market_id).await? {
            return Ok(ExecutionResult::Skipped {
                reason: format!("Already have position in market: {}", existing.market_id),
            });
        }

        // Calculate position size
        let size = self.calculate_position_size(opportunity)?;

        if size.is_zero() {
            return Ok(ExecutionResult::Skipped {
                reason: "Position size too small".to_string(),
            });
        }

        if self.config.paper_trading {
            self.paper_execute(opportunity, size).await
        } else {
            self.live_execute(opportunity, size).await
        }
    }

    /// Paper trade execution (simulation)
    async fn paper_execute(
        &self,
        opportunity: &Opportunity,
        size: Decimal,
    ) -> Result<ExecutionResult> {
        info!(
            "[PAPER] Executing {} {} at {} for ${} - {}",
            opportunity.side,
            opportunity.market_id,
            opportunity.entry_price,
            size,
            opportunity.short_question(50)
        );

        // Record the position in database
        let position_id = self
            .db
            .create_position(
                &opportunity.market_id,
                &opportunity.question,
                opportunity.side,
                opportunity.entry_price,
                size,
                opportunity.strategy,
                true, // Paper trade
            )
            .await
            .context("Failed to create position record")?;

        Ok(ExecutionResult::Executed {
            position_id,
            side: opportunity.side,
            price: opportunity.entry_price,
            size,
            paper: true,
        })
    }

    /// Live trade execution via CLOB API
    async fn live_execute(
        &self,
        opportunity: &Opportunity,
        size: Decimal,
    ) -> Result<ExecutionResult> {
        // Get token ID for the order
        let token_id = opportunity.token_id.as_ref()
            .ok_or_else(|| anyhow!("Token ID required for live trading"))?;

        let private_key = self.config.private_key.as_ref()
            .ok_or_else(|| anyhow!("Private key required for live trading"))?;

        info!(
            "[LIVE] Executing {} {} at {} for ${} - {}",
            opportunity.side,
            opportunity.market_id,
            opportunity.entry_price,
            size,
            opportunity.short_question(50)
        );

        // Create signer from private key
        let signer: PrivateKeySigner = private_key.parse()
            .context("Failed to parse private key")?;
        let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

        // Create CLOB config
        let clob_config = ClobConfig::builder()
            .use_server_time(true)
            .build();

        // Create and authenticate client
        debug!("Authenticating with CLOB API...");
        let client = ClobClient::new(CLOB_ENDPOINT, clob_config)
            .context("Failed to create CLOB client")?
            .authentication_builder(&signer)
            .authenticate()
            .await
            .context("Failed to authenticate with CLOB")?;

        info!("CLOB client authenticated successfully");

        // We're always buying (either YES or NO tokens)
        let clob_side = ClobSide::Buy;

        // Convert token_id string to U256
        let token_id_u256 = U256::from_str_radix(token_id, 10)
            .context("Failed to parse token ID as U256")?;

        // Create market order (Fill-or-Kill for immediate execution)
        debug!("Creating market order: token={}, amount=${}, side={:?}", token_id, size, clob_side);

        let order = client
            .market_order()
            .token_id(token_id_u256)
            .amount(Amount::usdc(size).context("Failed to create USDC amount")?)
            .side(clob_side)
            .order_type(OrderType::FOK) // Fill-or-Kill
            .build()
            .await
            .context("Failed to build market order")?;

        // Sign the order
        let signed_order = client
            .sign(&signer, order)
            .await
            .context("Failed to sign order")?;

        // Submit the order
        let response = client
            .post_order(signed_order)
            .await
            .context("Failed to submit order")?;

        info!("[LIVE] Order submitted: {:?}", response);

        // Record the position in database
        let position_id = self
            .db
            .create_position(
                &opportunity.market_id,
                &opportunity.question,
                opportunity.side,
                opportunity.entry_price,
                size,
                opportunity.strategy,
                false, // Live trade
            )
            .await
            .context("Failed to create position record")?;

        Ok(ExecutionResult::Executed {
            position_id,
            side: opportunity.side,
            price: opportunity.entry_price,
            size,
            paper: false,
        })
    }

    /// Calculate position size based on config and risk parameters
    fn calculate_position_size(&self, opportunity: &Opportunity) -> Result<Decimal> {
        // Start with max position size
        let mut size = self.config.max_position_size;

        // Scale by edge (higher edge = larger position)
        let edge_factor = Decimal::try_from(opportunity.edge.min(0.30) / 0.30)?;
        size = size * edge_factor;

        // Don't exceed max position size
        size = size.min(self.config.max_position_size);

        // Round to 2 decimal places
        size = size.round_dp(2);

        Ok(size)
    }

    /// Check current exposure against limits
    pub async fn check_exposure(&self) -> Result<ExposureStatus> {
        let positions = self.db.get_open_positions().await?;

        let total_exposure: Decimal = positions.iter().map(|p| p.size).sum();

        let available = self.config.max_total_exposure - total_exposure;

        Ok(ExposureStatus {
            current: total_exposure,
            max: self.config.max_total_exposure,
            available,
            position_count: positions.len(),
        })
    }
}

/// Result of an execution attempt
#[derive(Debug)]
pub enum ExecutionResult {
    Executed {
        position_id: i64,
        side: Side,
        price: Decimal,
        size: Decimal,
        paper: bool,
    },
    Skipped {
        reason: String,
    },
}

/// Current exposure status
#[derive(Debug)]
pub struct ExposureStatus {
    pub current: Decimal,
    pub max: Decimal,
    pub available: Decimal,
    pub position_count: usize,
}

impl std::fmt::Display for ExposureStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Exposure: ${:.2} / ${:.2} ({} positions, ${:.2} available)",
            self.current, self.max, self.position_count, self.available
        )
    }
}
