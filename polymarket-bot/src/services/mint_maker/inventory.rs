//! Inventory management for Mint Maker - merges matched pairs back to USDC

use crate::db::Database;
use crate::services::CtfService;
use anyhow::Result;
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

/// Merge a matched pair (both sides filled) back into USDC via CTF relay
pub async fn merge_matched_pair(
    db: &Arc<Database>,
    pair_id: i64,
    condition_id: &str,
    size: &str,
    private_key: &str,
    builder_api_key: &str,
    builder_secret: &str,
    builder_passphrase: &str,
    yes_token_id: Option<&str>,
    no_token_id: Option<&str>,
    neg_risk: bool,
) -> Result<Option<String>> {
    // Mark as merging
    db.update_mint_maker_pair_status(pair_id, "Merging").await?;

    let amount = Decimal::from_str(size).unwrap_or(Decimal::ZERO);
    if amount <= Decimal::ZERO {
        warn!("MintMaker: Invalid merge amount for pair {}: {}", pair_id, size);
        db.update_mint_maker_pair_status(pair_id, "Cancelled").await?;
        return Ok(None);
    }

    let ctf = CtfService::new();
    let result = ctf.merge(
        condition_id,
        amount,
        private_key,
        builder_api_key,
        builder_secret,
        builder_passphrase,
        yes_token_id,
        no_token_id,
        neg_risk,
    ).await?;

    if result.success {
        let tx_id = result.transaction_id.unwrap_or_else(|| "unknown".to_string());
        info!("MintMaker: Pair {} merged successfully, tx: {}", pair_id, tx_id);
        db.mark_mint_maker_pair_merged(pair_id, &tx_id).await?;
        Ok(Some(tx_id))
    } else {
        let err = result.error.unwrap_or_else(|| "unknown error".to_string());
        warn!("MintMaker: Merge failed for pair {}: {}", pair_id, err);
        // Revert to Matched so we can retry
        db.update_mint_maker_pair_status(pair_id, "Matched").await?;
        // Propagate as Err so the caller can detect 429 rate limits
        Err(anyhow::anyhow!("{}", err))
    }
}

/// Cancel the unfilled side of a half-filled pair
pub async fn cancel_half_filled(
    db: &Arc<Database>,
    pair_id: i64,
    unfilled_order_id: &str,
    wallet_address: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<()> {
    // Cancel the unfilled order
    if let Err(e) = super::order_manager::cancel_order(
        wallet_address,
        unfilled_order_id,
        api_key,
        api_secret,
        api_passphrase,
    ).await {
        warn!("MintMaker: Failed to cancel unfilled order {} for pair {}: {}", unfilled_order_id, pair_id, e);
    }

    // Mark pair as cancelled (the filled side token remains in wallet)
    db.update_mint_maker_pair_status(pair_id, "Cancelled").await?;
    info!("MintMaker: Half-filled pair {} cancelled, unfilled order {} cancelled", pair_id, unfilled_order_id);

    Ok(())
}
