//! Mint Maker API route handlers

use crate::api::server::AppState;
use crate::db::MintMakerSettingsRow;
use crate::services::mint_maker::order_manager;
use crate::services::safe_activation::{self, BuilderCredentials};
use crate::wallet::decrypt_private_key;
use alloy::signers::{local::PrivateKeySigner, Signer};
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use polymarket_client_sdk::auth::ExposeSecret;
use polymarket_client_sdk::clob::{Client as ClobClient, Config as ClobConfig};
use polymarket_client_sdk::clob::types::{
    AssetType, SignatureType,
    request::UpdateBalanceAllowanceRequest,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tracing::{info, warn};

const POLYGON_CHAIN_ID: u64 = 137;
const CLOB_ENDPOINT: &str = "https://clob.polymarket.com";

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Helper to validate session and return wallet address
async fn validate_session(
    state: &AppState,
    token: &str,
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    let session = state.db.get_session(token).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Invalid or expired session".to_string() })))?;
    Ok(session.wallet_address)
}

// ==================== GET /api/mint-maker/settings ====================

#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    pub settings: MintMakerSettingsRow,
}

pub async fn get_settings(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<SettingsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;
    let settings = state.db.get_mint_maker_settings(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;
    Ok(Json(SettingsResponse { settings }))
}

// ==================== PUT /api/mint-maker/settings ====================

#[derive(Debug, Deserialize)]
pub struct UpdateSettingsRequest {
    pub preset: Option<String>,
    pub bid_offset_cents: Option<i32>,
    pub max_pair_cost: Option<f64>,
    pub min_spread_profit: Option<f64>,
    pub max_pairs_per_market: Option<i32>,
    pub max_total_pairs: Option<i32>,
    pub stale_order_seconds: Option<i64>,
    pub assets: Option<Vec<String>>,
    pub min_minutes_to_close: Option<f64>,
    pub max_minutes_to_close: Option<f64>,
    pub auto_place: Option<bool>,
    pub auto_place_size: Option<String>,
    pub auto_max_markets: Option<i32>,
    pub auto_redeem: Option<bool>,
}

pub async fn update_settings(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;
    let mut settings = state.db.get_mint_maker_settings(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;

    // Apply preset if specified
    if let Some(preset) = &req.preset {
        apply_preset(&mut settings, preset);
    }

    // Override individual fields
    if let Some(v) = req.bid_offset_cents { settings.bid_offset_cents = v; }
    if let Some(v) = req.max_pair_cost { settings.max_pair_cost = v; }
    if let Some(v) = req.min_spread_profit { settings.min_spread_profit = v; }
    if let Some(v) = req.max_pairs_per_market { settings.max_pairs_per_market = v; }
    if let Some(v) = req.max_total_pairs { settings.max_total_pairs = v; }
    if let Some(v) = req.stale_order_seconds { settings.stale_order_seconds = v; }
    if let Some(v) = req.assets { settings.assets = v; }
    if let Some(v) = req.min_minutes_to_close { settings.min_minutes_to_close = v; }
    if let Some(v) = req.max_minutes_to_close { settings.max_minutes_to_close = v; }
    if let Some(v) = req.auto_place { settings.auto_place = v; }
    if let Some(ref v) = req.auto_place_size { settings.auto_place_size = v.clone(); }
    if let Some(v) = req.auto_max_markets { settings.auto_max_markets = v; }
    if let Some(v) = req.auto_redeem { settings.auto_redeem = v; }
    if let Some(p) = &req.preset { settings.preset = p.clone(); }

    state.db.upsert_mint_maker_settings(&settings).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;

    Ok(Json(serde_json::json!({ "success": true })))
}

/// Apply a preset configuration
fn apply_preset(settings: &mut MintMakerSettingsRow, preset: &str) {
    match preset {
        "conservative" => {
            settings.bid_offset_cents = 3;
            settings.max_pair_cost = 0.96;
            settings.min_spread_profit = 0.02;
            settings.max_pairs_per_market = 3;
            settings.max_total_pairs = 10;
            settings.stale_order_seconds = 90;
            settings.assets = vec!["BTC".to_string()];
            settings.auto_place_size = "2".to_string();
        }
        "balanced" => {
            settings.bid_offset_cents = 2;
            settings.max_pair_cost = 0.98;
            settings.min_spread_profit = 0.01;
            settings.max_pairs_per_market = 5;
            settings.max_total_pairs = 20;
            settings.stale_order_seconds = 120;
            settings.assets = vec!["BTC".to_string(), "ETH".to_string(), "SOL".to_string(), "XRP".to_string()];
            settings.auto_place_size = "2".to_string();
        }
        "aggressive" => {
            settings.bid_offset_cents = 1;
            settings.max_pair_cost = 0.99;
            settings.min_spread_profit = 0.005;
            settings.max_pairs_per_market = 10;
            settings.max_total_pairs = 50;
            settings.stale_order_seconds = 180;
            settings.assets = vec!["BTC".to_string(), "ETH".to_string(), "SOL".to_string(), "XRP".to_string()];
            settings.auto_place_size = "5".to_string();
        }
        _ => {}
    }
}

// ==================== POST /api/mint-maker/enable ====================

#[derive(Debug, Deserialize)]
pub struct EnableRequest {
    pub password: String,
}

pub async fn enable(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<EnableRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;

    // Verify password by attempting to decrypt key
    let encrypted_key = state.db.get_encrypted_key(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "No stored key".to_string() })))?;

    let private_key = decrypt_private_key(&encrypted_key, &req.password)
        .map_err(|_| (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Invalid password".to_string() })))?;

    // Store decrypted key in key store for auto operations
    state.key_store.store_key(&wallet, private_key.clone()).await;

    let signer: PrivateKeySigner = private_key.parse()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Invalid key: {}", e) })))?;
    let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

    // Step 1: Ensure Safe proxy is deployed and has on-chain approvals
    // Generated wallets use Gnosis Safe proxies — the Safe must be deployed
    // and have ERC-20/ERC-1155 approvals for the exchange contracts.
    let builder_creds = match (
        state.config.builder_api_key.as_ref(),
        state.config.builder_secret.as_ref(),
        state.config.builder_passphrase.as_ref(),
    ) {
        (Some(key), Some(secret), Some(pass)) => Some(BuilderCredentials {
            api_key: key.clone(),
            secret: secret.clone(),
            passphrase: pass.clone(),
        }),
        _ => None,
    };

    if let Some(bcreds) = &builder_creds {
        match safe_activation::ensure_safe_activated(&signer, bcreds).await {
            Ok(safe_addr) => info!("MintMaker: Safe activated at {}", safe_addr),
            Err(e) => {
                warn!("MintMaker: Safe activation error: {}", e);
                // Don't block enable — the Safe may already be activated
                // and the check might have failed due to a network issue
            }
        }
    } else {
        warn!("MintMaker: No builder credentials configured, skipping Safe activation");
    }

    // Step 2: Derive CLOB API credentials using unauthenticated client + L1 signer
    let clob_config = ClobConfig::builder()
        .use_server_time(true)
        .build();

    let unauth_client = ClobClient::new(CLOB_ENDPOINT, clob_config)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("CLOB client error: {}", e) })))?;

    let creds = unauth_client.create_or_derive_api_key(&signer, None).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("API key derivation error: {}", e) })))?;

    let api_key = creds.key().to_string();
    let api_secret = creds.secret().expose_secret().to_string();
    let api_passphrase = creds.passphrase().expose_secret().to_string();

    info!("MintMaker: Derived API credentials for {}", wallet);

    // Store credentials in DB for future use (cancel orders, check fills, etc.)
    if let Err(e) = state.db.store_api_credentials(&wallet, &api_key, &api_secret, &api_passphrase).await {
        warn!("MintMaker: Failed to store API credentials: {}", e);
    }

    // Step 3: Tell CLOB to refresh its cached view of on-chain balance & allowances.
    // Must use an authenticated client with GnosisSafe signature type so the CLOB
    // checks the Safe proxy address (where USDC lives), not the bare EOA.
    // The SDK uses GET /balance-allowance/update with query params (not POST).
    {
        let clob_config2 = ClobConfig::builder()
            .use_server_time(true)
            .build();
        let authed_client = ClobClient::new(CLOB_ENDPOINT, clob_config2)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("CLOB client error: {}", e) })))?
            .authentication_builder(&signer)
            .signature_type(SignatureType::GnosisSafe)
            .credentials(creds)
            .authenticate()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("CLOB auth error: {}", e) })))?;

        // Refresh COLLATERAL (USDC) allowance cache
        let collateral_req = UpdateBalanceAllowanceRequest::builder()
            .asset_type(AssetType::Collateral)
            .build();
        match authed_client.update_balance_allowance(collateral_req).await {
            Ok(()) => info!("MintMaker: COLLATERAL allowance refreshed for {}", wallet),
            Err(e) => warn!("MintMaker: Failed to refresh COLLATERAL allowance: {}", e),
        }

        // Refresh CONDITIONAL token allowance cache
        let conditional_req = UpdateBalanceAllowanceRequest::builder()
            .asset_type(AssetType::Conditional)
            .build();
        match authed_client.update_balance_allowance(conditional_req).await {
            Ok(()) => info!("MintMaker: CONDITIONAL allowance refreshed for {}", wallet),
            Err(e) => warn!("MintMaker: Failed to refresh CONDITIONAL allowance: {}", e),
        }
    }

    // Enable in settings
    let mut settings = state.db.get_mint_maker_settings(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;
    settings.enabled = true;
    state.db.upsert_mint_maker_settings(&settings).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;

    info!("MintMaker enabled for wallet {}", wallet);
    Ok(Json(serde_json::json!({ "success": true, "message": "Mint Maker enabled" })))
}

// ==================== POST /api/mint-maker/disable ====================

pub async fn disable(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;

    let mut settings = state.db.get_mint_maker_settings(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;
    settings.enabled = false;
    state.db.upsert_mint_maker_settings(&settings).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;

    info!("MintMaker disabled for wallet {}", wallet);
    Ok(Json(serde_json::json!({ "success": true })))
}

// ==================== GET /api/mint-maker/pairs ====================

pub async fn get_pairs(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;
    let pairs = state.db.get_mint_maker_recent_pairs(&wallet, 50).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;
    Ok(Json(serde_json::json!({ "pairs": pairs })))
}

// ==================== GET /api/mint-maker/stats ====================

pub async fn get_stats(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;
    let (total, merged, cancelled, profit, cost, avg_spread) = state.db.get_mint_maker_stats(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;
    let fill_rate = if total > 0 { merged as f64 / total as f64 * 100.0 } else { 0.0 };

    Ok(Json(serde_json::json!({
        "stats": {
            "total_pairs": total,
            "merged_pairs": merged,
            "cancelled_pairs": cancelled,
            "total_profit": format!("{:.4}", profit),
            "total_cost": format!("{:.4}", cost),
            "avg_spread": format!("{:.4}", avg_spread),
            "fill_rate": format!("{:.2}", fill_rate)
        }
    })))
}

// ==================== GET /api/mint-maker/log ====================

pub async fn get_log(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;
    let log = state.db.get_mint_maker_log(&wallet, 50).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;
    Ok(Json(serde_json::json!({ "log": log })))
}

// ==================== POST /api/mint-maker/place ====================

#[derive(Debug, Deserialize)]
pub struct PlacePairRequest {
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub asset: String,
    pub yes_token_id: String,
    pub no_token_id: String,
    pub yes_price: String,
    pub no_price: String,
    pub size: String,
    pub password: String,
    pub slug: Option<String>,
    #[serde(default)]
    pub neg_risk: bool,
}

pub async fn place_pair(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<PlacePairRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;

    // Check capacity
    let settings = state.db.get_mint_maker_settings(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;

    let market_pairs = state.db.count_mint_maker_open_pairs_for_market(&wallet, &req.market_id).await.unwrap_or(0);
    if market_pairs >= settings.max_pairs_per_market as i64 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: format!("Max pairs per market reached ({}/{})", market_pairs, settings.max_pairs_per_market),
        })));
    }

    let total_pairs = state.db.count_mint_maker_total_open_pairs(&wallet).await.unwrap_or(0);
    if total_pairs >= settings.max_total_pairs as i64 {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: format!("Max total pairs reached ({}/{})", total_pairs, settings.max_total_pairs),
        })));
    }

    // Decrypt private key
    let encrypted_key = state.db.get_encrypted_key(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "No stored key".to_string() })))?;

    let private_key = decrypt_private_key(&encrypted_key, &req.password)
        .map_err(|_| (StatusCode::UNAUTHORIZED, Json(ErrorResponse { error: "Invalid password".to_string() })))?;

    // Parse prices and USD amount per side
    let yes_price = Decimal::from_str(&req.yes_price)
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Invalid yes_price".to_string() })))?;
    let no_price = Decimal::from_str(&req.no_price)
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Invalid no_price".to_string() })))?;
    let usd_per_side = Decimal::from_str(&req.size)
        .map_err(|_| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Invalid size (USD per side)".to_string() })))?;

    if usd_per_side <= Decimal::ZERO {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Size must be positive".to_string() })));
    }

    // Validate pair cost (per-pair: yes_bid + no_bid must be < $1.00)
    let pair_cost_per_share = yes_price + no_price;
    if pair_cost_per_share >= Decimal::ONE {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: format!("Pair cost {} >= $1.00, not profitable", pair_cost_per_share),
        })));
    }

    let total_cost = usd_per_side * Decimal::from(2);

    // Ensure Safe has CLOB approval and cache is refreshed before placing orders
    {
        let signer: PrivateKeySigner = private_key.parse()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("Invalid key: {}", e) })))?;
        let signer = signer.with_chain_id(Some(POLYGON_CHAIN_ID));

        if let (Some(key), Some(secret), Some(pass)) = (
            state.config.builder_api_key.as_ref(),
            state.config.builder_secret.as_ref(),
            state.config.builder_passphrase.as_ref(),
        ) {
            let bcreds = BuilderCredentials {
                api_key: key.clone(),
                secret: secret.clone(),
                passphrase: pass.clone(),
            };
            match safe_activation::ensure_safe_activated(&signer, &bcreds).await {
                Ok(safe_addr) => {
                    info!("MintMaker place_pair: Safe ready at {}", safe_addr);

                    // Check balance
                    let http_client = reqwest::Client::new();
                    match order_manager::fetch_safe_usdc_balance(&http_client, &safe_addr).await {
                        Ok(balance) => {
                            if total_cost > balance {
                                return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
                                    error: format!("Insufficient balance: ${} available, ${} needed", balance, total_cost),
                                })));
                            }
                        }
                        Err(e) => warn!("MintMaker: Could not check balance: {}", e),
                    }
                }
                Err(e) => warn!("MintMaker place_pair: Safe activation failed: {}", e),
            }
        }

        // Refresh CLOB's cached view of on-chain balance & allowances.
        if let Err(e) = order_manager::refresh_clob_allowance_cache(&private_key).await {
            warn!("MintMaker place_pair: CLOB cache refresh failed: {}", e);
        }

        // Ensure API credentials exist (needed for cancel operations)
        if let Err(e) = order_manager::ensure_clob_api_credentials(&private_key, &state.db, &wallet).await {
            warn!("MintMaker place_pair: Failed to ensure API credentials: {}", e);
        }
    }

    // Calculate shares from USD / price
    let yes_shares = if yes_price > Decimal::ZERO { (usd_per_side / yes_price).floor() } else { Decimal::ZERO };
    let no_shares = if no_price > Decimal::ZERO { (usd_per_side / no_price).floor() } else { Decimal::ZERO };
    // CLOB requires minimum 5 shares per order
    let min_shares = Decimal::from(5);
    if yes_shares < min_shares || no_shares < min_shares {
        let needed_per_side = (min_shares * std::cmp::max(yes_price, no_price)).ceil();
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse {
            error: format!(
                "Below CLOB minimum: YES:{} NO:{} shares (min 5). Need at least ${}/side at these prices.",
                yes_shares, no_shares, needed_per_side
            ),
        })));
    }
    let merge_size = std::cmp::min(yes_shares, no_shares);

    info!("MintMaker: Placing GTC pair for {} - YES@{}x{} NO@{}x{} (${}/side)",
        req.market_id, yes_price, yes_shares, no_price, no_shares, usd_per_side);

    // Place YES as GTC at scanner price (aggressive limit for fast fill + 0% maker fee)
    let yes_order_id = order_manager::place_gtc_bid(&private_key, &req.yes_token_id, yes_price, yes_shares).await
        .map_err(|e| {
            warn!("MintMaker: YES GTC failed for {}: {:?}", req.market_id, e);
            (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("Failed to place YES bid: {}", e) }))
        })?;

    // Place NO as GTC at scanner price.
    // If this fails, try to cancel YES; if cancel fails, record orphan.
    let no_order_id = match order_manager::place_gtc_bid(&private_key, &req.no_token_id, no_price, no_shares).await {
        Ok(id) => id,
        Err(e) => {
            warn!("MintMaker: NO GTC failed, cancelling YES order {}: {}", yes_order_id, e);
            let cancel_ok = if let Ok(Some((ak, as_, ap))) = state.db.get_api_credentials(&wallet).await {
                order_manager::cancel_order(&wallet, &yes_order_id, &ak, &as_, &ap).await.is_ok()
            } else { false };

            if !cancel_ok {
                warn!("MintMaker: YES cancel failed — recording orphaned order {}", yes_order_id);
                let shares_str = yes_shares.to_string();
                if let Ok(pair_id) = state.db.create_mint_maker_pair(
                    &wallet, &req.market_id, &req.condition_id, &req.question, &req.asset,
                    &yes_order_id, "",
                    &yes_price.to_string(), "0",
                    &shares_str,
                    Some(&shares_str), None,
                    req.slug.as_deref(),
                    Some(&req.yes_token_id),
                    Some(&req.no_token_id),
                    req.neg_risk,
                ).await {
                    let _ = state.db.update_mint_maker_pair_status(pair_id, "Orphaned").await;
                }
                let _ = state.db.log_mint_maker_action(
                    &wallet, "orphaned", Some(&req.market_id), Some(&req.question), Some(&req.asset),
                    Some(&yes_price.to_string()), None,
                    None, None,
                    Some(&shares_str),
                    Some(&format!("YES GTC {} — NO failed, cancel failed — orphaned", yes_order_id)),
                ).await;
            }
            return Err((StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("Failed to place NO bid: {}", e) })));
        }
    };

    // Both GTC orders placed. They're aggressive limits at scanner price —
    // should fill quickly on fresh markets, and qualify as maker (0% fee) if they rest.

    // Record pair in DB
    let pair_id = state.db.create_mint_maker_pair(
        &wallet, &req.market_id, &req.condition_id, &req.question, &req.asset,
        &yes_order_id, &no_order_id, &yes_price.to_string(), &no_price.to_string(),
        &merge_size.to_string(),
        Some(&yes_shares.to_string()), Some(&no_shares.to_string()),
        req.slug.as_deref(),
        Some(&req.yes_token_id),
        Some(&req.no_token_id),
        req.neg_risk,
    ).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;

    // Log action
    let per_pair_profit = Decimal::ONE - pair_cost_per_share;
    let total_profit = per_pair_profit * merge_size;
    let _ = state.db.log_mint_maker_action(
        &wallet, "place_pair", Some(&req.market_id), Some(&req.question), Some(&req.asset),
        Some(&yes_price.to_string()), Some(&no_price.to_string()),
        Some(&pair_cost_per_share.to_string()), Some(&per_pair_profit.to_string()),
        Some(&merge_size.to_string()),
        Some(&format!("GTC@market yes={} no={} ${}/side", yes_order_id, no_order_id, usd_per_side)),
    ).await;

    info!("MintMaker: Pair {} placed (GTC@market) - YES:{}x{} NO:{}x{} cost=${}, profit=${}",
        pair_id, yes_order_id, yes_shares, no_order_id, no_shares, total_cost, total_profit);

    Ok(Json(serde_json::json!({
        "success": true,
        "pair_id": pair_id,
        "yes_order_id": yes_order_id,
        "no_order_id": no_order_id,
        "yes_shares": yes_shares.to_string(),
        "no_shares": no_shares.to_string(),
        "pair_cost": total_cost.to_string(),
        "expected_profit": total_profit.to_string()
    })))
}

// ==================== POST /api/mint-maker/cancel-pair ====================

#[derive(Debug, Deserialize)]
pub struct CancelPairRequest {
    pub pair_id: i64,
}

pub async fn cancel_pair(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<CancelPairRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let wallet = validate_session(&state, auth.token()).await?;

    // Get the pair
    let pairs = state.db.get_mint_maker_open_pairs(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;

    let pair = pairs.iter().find(|p| p.id == req.pair_id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(ErrorResponse { error: "Pair not found".to_string() })))?;

    // Get API credentials for cancellation
    let creds = state.db.get_api_credentials(&wallet).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?
        .ok_or_else(|| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "No API credentials".to_string() })))?;

    let (api_key, api_secret, api_passphrase) = creds;

    // Cancel both orders
    let _ = order_manager::cancel_order(&wallet, &pair.yes_order_id, &api_key, &api_secret, &api_passphrase).await;
    let _ = order_manager::cancel_order(&wallet, &pair.no_order_id, &api_key, &api_secret, &api_passphrase).await;

    // Update status
    state.db.update_mint_maker_pair_status(req.pair_id, "Cancelled").await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: format!("DB error: {}", e) })))?;

    let _ = state.db.log_mint_maker_action(
        &wallet, "cancel_pair", Some(&pair.market_id), Some(&pair.question), Some(&pair.asset),
        None, None, None, None, Some(&pair.size), Some("Manual cancel"),
    ).await;

    info!("MintMaker: Pair {} cancelled by user", req.pair_id);

    Ok(Json(serde_json::json!({ "success": true })))
}
