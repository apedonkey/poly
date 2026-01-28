//! Trade API endpoints

use crate::api::server::AppState;
use crate::types::{Side, StrategyType};
use crate::wallet::decrypt_private_key;
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use base64::Engine;
use tracing::{info, warn};
use alloy::primitives::{keccak256, Address, B256};

/// Safe Proxy Factory address on Polygon
const SAFE_FACTORY: &str = "0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b";
/// Init code hash for Safe proxy
const SAFE_INIT_CODE_HASH: &str = "0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf";

/// Derive the Polymarket Safe proxy wallet address from an EOA address
/// Uses CREATE2: address = keccak256(0xff ++ factory ++ salt ++ init_code_hash)[12:]
fn derive_safe_wallet(eoa_address: &str) -> Result<String, String> {
    // Parse addresses
    let eoa: Address = eoa_address.parse()
        .map_err(|e| format!("Invalid EOA address: {}", e))?;
    let factory: Address = SAFE_FACTORY.parse()
        .map_err(|e| format!("Invalid factory address: {}", e))?;
    let init_code_hash: B256 = SAFE_INIT_CODE_HASH.parse()
        .map_err(|e| format!("Invalid init code hash: {}", e))?;

    // Salt = keccak256(abi.encode(address)) - address padded to 32 bytes (left-padded with zeros)
    let mut padded = [0u8; 32];
    padded[12..32].copy_from_slice(eoa.as_slice());
    let salt = keccak256(&padded);

    // CREATE2: keccak256(0xff ++ factory ++ salt ++ init_code_hash)
    let mut data = Vec::with_capacity(1 + 20 + 32 + 32);
    data.push(0xff);
    data.extend_from_slice(factory.as_slice());
    data.extend_from_slice(salt.as_slice());
    data.extend_from_slice(init_code_hash.as_slice());

    let hash = keccak256(&data);
    // Take last 20 bytes as address
    let proxy_address = Address::from_slice(&hash[12..]);

    Ok(format!("{:?}", proxy_address))
}

/// Execute trade request
#[derive(Debug, Deserialize)]
pub struct ExecuteTradeRequest {
    pub market_id: String,
    pub side: String, // "Yes" or "No"
    pub size_usdc: String,
    /// Password to decrypt private key for live trading
    pub password: String,
}

/// Paper trade request (no password needed)
#[derive(Debug, Deserialize)]
pub struct PaperTradeRequest {
    pub market_id: String,
    pub side: String,
    pub size_usdc: String,
}

/// Trade response
#[derive(Debug, Serialize)]
pub struct TradeResponse {
    pub success: bool,
    pub position_id: Option<i64>,
    pub message: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Execute a live trade
pub async fn execute_trade(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<ExecuteTradeRequest>,
) -> Result<Json<TradeResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate session
    let session = state
        .db
        .get_session(auth.token())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or expired session".to_string(),
                }),
            )
        })?;

    // Get encrypted key
    let encrypted_key = state
        .db
        .get_encrypted_key(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Wallet has no stored encrypted key. Use paper trading or import key client-side.".to_string(),
                }),
            )
        })?;

    // Decrypt private key
    let _private_key = decrypt_private_key(&encrypted_key, &req.password).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid password".to_string(),
            }),
        )
    })?;

    // Parse side
    let side = match req.side.to_lowercase().as_str() {
        "yes" => Side::Yes,
        "no" => Side::No,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Side must be 'Yes' or 'No'".to_string(),
                }),
            ))
        }
    };

    // Parse size
    let size = Decimal::from_str(&req.size_usdc).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid size_usdc".to_string(),
            }),
        )
    })?;

    // Find the opportunity to get the entry price
    let opportunities = state.opportunities.read().await;
    let opportunity = opportunities
        .iter()
        .find(|o| o.market_id == req.market_id && o.side == side);

    let entry_price = match opportunity {
        Some(opp) => opp.entry_price,
        None => {
            // If no matching opportunity, this might be a manual trade
            // For now, reject it
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No matching opportunity found for this market/side".to_string(),
                }),
            ));
        }
    };

    // Calculate end_date from opportunity's time_to_close_hours
    let end_date = opportunity
        .and_then(|o| o.time_to_close_hours)
        .map(|hours| Utc::now() + Duration::seconds((hours * 3600.0) as i64));

    // Get token_id from opportunity
    let token_id = opportunity.and_then(|o| o.token_id.clone());

    // Get slug from opportunity
    let slug = opportunity.map(|o| o.slug.clone());

    // TODO: Actually execute the trade using Executor with the decrypted private key
    // For now, just record as paper trade
    let position_id = state
        .db
        .create_position_for_wallet(
            &session.wallet_address,
            &req.market_id,
            &opportunity.map(|o| o.question.clone()).unwrap_or_default(),
            slug.as_deref(),
            side,
            entry_price,
            size,
            StrategyType::ResolutionSniper, // TODO: Get from opportunity
            false, // Live trade
            end_date,
            token_id.as_deref(),
            None, // No order_id for this flow yet
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(TradeResponse {
        success: true,
        position_id: Some(position_id),
        message: "Trade executed (paper mode - live trading coming soon)".to_string(),
    }))
}

/// Execute a paper trade (no real money)
pub async fn paper_trade(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<PaperTradeRequest>,
) -> Result<Json<TradeResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate session
    let session = state
        .db
        .get_session(auth.token())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or expired session".to_string(),
                }),
            )
        })?;

    // Parse side
    let side = match req.side.to_lowercase().as_str() {
        "yes" => Side::Yes,
        "no" => Side::No,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Side must be 'Yes' or 'No'".to_string(),
                }),
            ))
        }
    };

    // Parse size
    let size = Decimal::from_str(&req.size_usdc).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid size_usdc".to_string(),
            }),
        )
    })?;

    // Find the opportunity
    let opportunities = state.opportunities.read().await;
    let opportunity = opportunities
        .iter()
        .find(|o| o.market_id == req.market_id && o.side == side);

    let (entry_price, question, slug, strategy, time_to_close, token_id) = match opportunity {
        Some(opp) => (opp.entry_price, opp.question.clone(), opp.slug.clone(), opp.strategy, opp.time_to_close_hours, opp.token_id.clone()),
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No matching opportunity found for this market/side".to_string(),
                }),
            ));
        }
    };

    // Calculate end_date from time_to_close_hours
    let end_date = time_to_close
        .map(|hours| Utc::now() + Duration::seconds((hours * 3600.0) as i64));

    // Record the paper trade
    let position_id = state
        .db
        .create_position_for_wallet(
            &session.wallet_address,
            &req.market_id,
            &question,
            Some(&slug),
            side,
            entry_price,
            size,
            strategy,
            true, // Paper trade
            end_date,
            token_id.as_deref(),
            None, // Paper trades don't have order_id
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(TradeResponse {
        success: true,
        position_id: Some(position_id),
        message: "Paper trade recorded".to_string(),
    }))
}

/// Signed order from frontend (for external wallet live trading)
#[derive(Debug, Deserialize)]
pub struct SignedOrder {
    pub salt: String,
    pub maker: String,
    pub signer: String,
    pub taker: String,
    #[serde(rename = "tokenId")]
    pub token_id: String,
    #[serde(rename = "makerAmount")]
    pub maker_amount: String,
    #[serde(rename = "takerAmount")]
    pub taker_amount: String,
    pub expiration: String,
    pub nonce: String,
    #[serde(rename = "feeRateBps")]
    pub fee_rate_bps: String,
    pub side: u8,
    #[serde(rename = "signatureType")]
    pub signature_type: u8,
    pub signature: String,
}

/// Request for executing a pre-signed order
#[derive(Debug, Deserialize)]
pub struct SignedTradeRequest {
    pub market_id: String,
    pub question: String,
    pub slug: Option<String>,
    pub side: String,
    pub size_usdc: String,
    pub entry_price: String,
    pub token_id: String,
    pub signed_order: SignedOrder,
    /// ISO8601 timestamp when market ends
    pub end_date: Option<String>,
}

/// Response for signed trade
#[derive(Debug, Serialize)]
pub struct SignedTradeResponse {
    pub success: bool,
    pub position_id: Option<i64>,
    pub order_id: Option<String>,
    pub message: String,
}

/// Execute a pre-signed order from an external wallet
pub async fn execute_signed_trade(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<SignedTradeRequest>,
) -> Result<Json<SignedTradeResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate session
    let session = state
        .db
        .get_session(auth.token())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or expired session".to_string(),
                }),
            )
        })?;

    // Verify the signer matches the session wallet
    if req.signed_order.signer.to_lowercase() != session.wallet_address.to_lowercase() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Signed order signer does not match session wallet".to_string(),
            }),
        ));
    }

    // Parse side
    let side = match req.side.to_lowercase().as_str() {
        "yes" => Side::Yes,
        "no" => Side::No,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Side must be 'Yes' or 'No'".to_string(),
                }),
            ))
        }
    };

    // Parse amounts
    let size = Decimal::from_str(&req.size_usdc).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid size_usdc".to_string(),
            }),
        )
    })?;

    let entry_price = Decimal::from_str(&req.entry_price).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid entry_price".to_string(),
            }),
        )
    })?;

    info!(
        "NEWCODE Submitting signed order for wallet {} - market: {}, side: {:?}, size: {}",
        session.wallet_address, req.market_id, side, size
    );

    // Get API credentials for this wallet
    let credentials = state
        .db
        .get_api_credentials(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    // Derive the proxy wallet address from EOA (Polymarket uses Safe proxies for browser wallets)
    let proxy_address = match derive_safe_wallet(&session.wallet_address) {
        Ok(addr) => addr,
        Err(e) => {
            warn!("Failed to derive proxy wallet: {}", e);
            session.wallet_address.clone() // Fallback to EOA
        }
    };
    println!(">>> EOA: {} -> Proxy: {}", session.wallet_address, proxy_address);
    warn!("EOA: {} -> Proxy: {}", session.wallet_address, proxy_address);

    let order_result = match credentials {
        Some((api_key, api_secret, api_passphrase)) => {
            info!("Using API credentials: key={}..., secret_len={}, pass_len={}",
                  &api_key[..8.min(api_key.len())], api_secret.len(), api_passphrase.len());
            submit_signed_order_to_clob(&req.signed_order, &session.wallet_address, &proxy_address, &api_key, &api_secret, &api_passphrase).await
        }
        None => {
            warn!("No API credentials found for wallet {}", session.wallet_address);
            Err("No API credentials found. Please authenticate with Polymarket first.".to_string())
        }
    };

    let (order_id, message) = match order_result {
        Ok(id) => {
            info!("Order submitted successfully: {}", id);
            (Some(id), "Order submitted to Polymarket CLOB".to_string())
        }
        Err(e) => {
            warn!("CLOB submission failed: {}. Recording position anyway.", e);
            (None, format!("Order recorded (CLOB submission: {})", e))
        }
    };

    // Parse end_date from request
    let end_date: Option<DateTime<Utc>> = req.end_date
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));

    // Record the position regardless of CLOB result
    let position_id = state
        .db
        .create_position_for_wallet(
            &session.wallet_address,
            &req.market_id,
            &req.question,
            req.slug.as_deref(),
            side,
            entry_price,
            size,
            StrategyType::ResolutionSniper, // Default to sniper for now
            false, // Live trade
            end_date,
            Some(&req.token_id),
            order_id.as_deref(),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(SignedTradeResponse {
        success: true,
        position_id: Some(position_id),
        order_id,
        message,
    }))
}

/// Request for recording a position (after browser-side CLOB submission)
#[derive(Debug, Deserialize)]
pub struct RecordPositionRequest {
    pub market_id: String,
    pub question: String,
    pub slug: Option<String>,
    pub side: String,
    pub size_usdc: String,
    pub entry_price: String,
    pub token_id: String,
    pub order_id: Option<String>,
    /// ISO8601 timestamp when market ends
    pub end_date: Option<String>,
}

/// Record a position after browser submitted to CLOB
pub async fn record_position(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<RecordPositionRequest>,
) -> Result<Json<TradeResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate session
    let session = state
        .db
        .get_session(auth.token())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or expired session".to_string(),
                }),
            )
        })?;

    // Parse side
    let side = match req.side.to_lowercase().as_str() {
        "yes" => Side::Yes,
        "no" => Side::No,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Side must be 'Yes' or 'No'".to_string(),
                }),
            ))
        }
    };

    // Parse amounts
    let size = Decimal::from_str(&req.size_usdc).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid size_usdc".to_string(),
            }),
        )
    })?;

    let entry_price = Decimal::from_str(&req.entry_price).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid entry_price".to_string(),
            }),
        )
    })?;

    info!(
        "Recording position for wallet {} - market: {}, side: {:?}, size: {}, order_id: {:?}",
        session.wallet_address, req.market_id, side, size, req.order_id
    );

    // Parse end_date from request
    let end_date: Option<DateTime<Utc>> = req.end_date
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc));

    // Record the position
    let position_id = state
        .db
        .create_position_for_wallet(
            &session.wallet_address,
            &req.market_id,
            &req.question,
            req.slug.as_deref(),
            side,
            entry_price,
            size,
            StrategyType::ResolutionSniper,
            false, // Live trade
            end_date,
            Some(&req.token_id),
            req.order_id.as_deref(),
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(TradeResponse {
        success: true,
        position_id: Some(position_id),
        message: format!("Position recorded (order_id: {:?})", req.order_id),
    }))
}

/// Submit a pre-signed order to the Polymarket CLOB API with authentication
/// NOTE: This often gets blocked by Cloudflare - browser-side submission is preferred
/// - eoa_address: The EOA that signed/owns the API key (used in POLY_ADDRESS header)
/// - proxy_address: The Polymarket proxy wallet (used as order owner)
async fn submit_signed_order_to_clob(
    order: &SignedOrder,
    eoa_address: &str,
    proxy_address: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<String, String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    info!("CLOB submission: eoa={}, proxy={}, api_key_len={}, secret_len={}",
          eoa_address, proxy_address, api_key.len(), api_secret.len());
    info!("Order: maker={}, signer={}, sigType={}, makerAmt={}, takerAmt={}, tokenId={}",
          order.maker, order.signer, order.signature_type, order.maker_amount, order.taker_amount, order.token_id);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    // Build the order payload per official Polymarket TypeScript SDK
    // Side is "BUY" or "SELL" string
    let side_str = if order.side == 0 { "BUY" } else { "SELL" };

    // SignatureType as string per TS SDK
    let sig_type_str = order.signature_type.to_string();

    // Salt as number (TS SDK uses parseInt)
    let salt_num: u64 = order.salt.parse().unwrap_or(0);

    let payload = serde_json::json!({
        "order": {
            "salt": salt_num,
            "maker": order.maker,
            "signer": order.signer,
            "taker": order.taker,
            "tokenId": order.token_id,
            "makerAmount": order.maker_amount,
            "takerAmount": order.taker_amount,
            "expiration": order.expiration,
            "nonce": order.nonce,
            "feeRateBps": order.fee_rate_bps,
            "side": side_str,
            "signatureType": sig_type_str,
            "signature": order.signature
        },
        "owner": api_key,
        "orderType": "GTC"
    });

    let body = serde_json::to_string(&payload).map_err(|e| format!("Serialize error: {}", e))?;
    info!("Request body: {}", body);
    let path = "/order";
    let method = "POST";
    // Polymarket uses milliseconds for timestamp
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    // Create L2 signature: timestamp + method + path + body
    // The secret is base64url encoded, need to decode first
    let sig_payload = format!("{}{}{}{}", timestamp, method, path, body);

    // Decode the base64url-encoded secret
    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(api_secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(api_secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(api_secret))
        .map_err(|e| format!("Failed to decode API secret: {}", e))?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes)
        .map_err(|e| format!("Invalid API secret: {}", e))?;
    mac.update(sig_payload.as_bytes());

    // Encode signature as base64url (with padding to match Python client)
    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    let response = client
        .post("https://clob.polymarket.com/order")
        .header("Content-Type", "application/json")
        .header("POLY_ADDRESS", eoa_address)
        .header("POLY_SIGNATURE", &signature)
        .header("POLY_TIMESTAMP", &timestamp)
        .header("POLY_API_KEY", api_key)
        .header("POLY_PASSPHRASE", api_passphrase)
        .body(body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if response.status().is_success() {
        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;

        if let Some(order_id) = result.get("orderId").and_then(|v| v.as_str()) {
            Ok(order_id.to_string())
        } else if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            Ok("submitted".to_string())
        } else {
            Err(format!("Unexpected response: {:?}", result))
        }
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!("CLOB API error {}: {}", status, body))
    }
}

/// Request for submitting an SDK-created signed order
/// The order comes from the official Polymarket SDK (created in browser)
#[derive(Debug, Deserialize)]
pub struct SubmitSdkOrderRequest {
    /// The signed order from the SDK (already properly formatted)
    pub signed_order: serde_json::Value,
    /// API credentials
    pub api_key: String,
    pub api_secret: String,
    pub api_passphrase: String,
    /// Order type (FOK, GTC, GTD)
    pub order_type: String,
}

/// Response for SDK order submission
#[derive(Debug, Serialize)]
pub struct SubmitOrderResponse {
    pub success: bool,
    pub order_id: Option<String>,
    pub error: Option<String>,
}

/// Submit an SDK-created order to the CLOB
/// This endpoint handles L2 authentication since crypto.subtle isn't available over HTTP
pub async fn submit_sdk_order(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<SubmitSdkOrderRequest>,
) -> Result<Json<SubmitOrderResponse>, (StatusCode, Json<ErrorResponse>)> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    // Validate session
    let session = state
        .db
        .get_session(auth.token())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or expired session".to_string(),
                }),
            )
        })?;

    info!("Submitting SDK order for wallet {}", session.wallet_address);
    info!("Order: {:?}", req.signed_order);

    // Convert SDK order format to API format
    // The SDK returns raw EIP-712 format, but API expects specific conversions
    let order = &req.signed_order;

    // Convert side: 0 -> "BUY", 1 -> "SELL"
    let side_str = match order.get("side").and_then(|v| v.as_i64()) {
        Some(0) => "BUY",
        Some(1) => "SELL",
        _ => "BUY", // Default
    };

    // Build the converted order payload
    let payload = serde_json::json!({
        "order": {
            "salt": order.get("salt").and_then(|v| v.as_str()).unwrap_or("0").parse::<u64>().unwrap_or(0),
            "maker": order.get("maker").and_then(|v| v.as_str()).unwrap_or(""),
            "signer": order.get("signer").and_then(|v| v.as_str()).unwrap_or(""),
            "taker": order.get("taker").and_then(|v| v.as_str()).unwrap_or("0x0000000000000000000000000000000000000000"),
            "tokenId": order.get("tokenId").and_then(|v| v.as_str()).unwrap_or(""),
            "makerAmount": order.get("makerAmount").and_then(|v| v.as_str()).unwrap_or("0"),
            "takerAmount": order.get("takerAmount").and_then(|v| v.as_str()).unwrap_or("0"),
            "expiration": order.get("expiration").and_then(|v| v.as_str()).unwrap_or("0"),
            "nonce": order.get("nonce").and_then(|v| v.as_str()).unwrap_or("0"),
            "feeRateBps": order.get("feeRateBps").and_then(|v| v.as_str()).unwrap_or("0"),
            "side": side_str,
            "signatureType": order.get("signatureType").and_then(|v| v.as_i64()).unwrap_or(2),
            "signature": order.get("signature").and_then(|v| v.as_str()).unwrap_or("")
        },
        "owner": req.api_key,
        "orderType": req.order_type
    });

    let body = serde_json::to_string(&payload).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Serialize error: {}", e),
            }),
        )
    })?;

    info!("Request body: {}", body);

    let path = "/order";
    let method = "POST";
    // Polymarket uses milliseconds for timestamp
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    // Create L2 signature
    let sig_payload = format!("{}{}{}{}", timestamp, method, path, body);

    // Decode the base64url-encoded secret
    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&req.api_secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&req.api_secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&req.api_secret))
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Failed to decode API secret: {}", e),
                }),
            )
        })?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("Invalid API secret: {}", e),
            }),
        )
    })?;
    mac.update(sig_payload.as_bytes());

    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to create client: {}", e),
                }),
            )
        })?;

    let response = client
        .post("https://clob.polymarket.com/order")
        .header("Content-Type", "application/json")
        .header("POLY_ADDRESS", &session.wallet_address)
        .header("POLY_SIGNATURE", &signature)
        .header("POLY_TIMESTAMP", &timestamp)
        .header("POLY_API_KEY", &req.api_key)
        .header("POLY_PASSPHRASE", &req.api_passphrase)
        .body(body)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("Network error: {}", e),
                }),
            )
        })?;

    if response.status().is_success() {
        let result: serde_json::Value = response.json().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("Parse error: {}", e),
                }),
            )
        })?;

        info!("CLOB response: {:?}", result);

        let order_id = result
            .get("orderID")
            .or_else(|| result.get("orderId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(Json(SubmitOrderResponse {
            success: true,
            order_id,
            error: None,
        }))
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        warn!("CLOB error {}: {}", status, body);

        Ok(Json(SubmitOrderResponse {
            success: false,
            order_id: None,
            error: Some(format!("CLOB API error {}: {}", status, body)),
        }))
    }
}

/// Request to enable trading (set USDC allowances)
#[derive(Debug, Deserialize)]
pub struct EnableTradingRequest {
    pub api_key: String,
    pub api_secret: String,
    pub api_passphrase: String,
}

/// Response for enable trading
#[derive(Debug, Serialize)]
pub struct EnableTradingResponse {
    pub success: bool,
    pub error: Option<String>,
}

/// Enable trading by setting USDC allowances via backend (handles L2 auth)
pub async fn enable_trading(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<EnableTradingRequest>,
) -> Result<Json<EnableTradingResponse>, (StatusCode, Json<ErrorResponse>)> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    // Validate session
    let session = state
        .db
        .get_session(auth.token())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid or expired session".to_string(),
                }),
            )
        })?;

    info!("Enabling trading for wallet {}", session.wallet_address);

    // Decode the base64url-encoded secret
    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&req.api_secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(&req.api_secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(&req.api_secret))
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Failed to decode API secret: {}", e),
                }),
            )
        })?;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to create client: {}", e),
                }),
            )
        })?;

    // Enable COLLATERAL (USDC) allowance
    let collateral_result = set_allowance(
        &client,
        &session.wallet_address,
        &req.api_key,
        &req.api_passphrase,
        &secret_bytes,
        "COLLATERAL",
    ).await;

    if let Err(e) = &collateral_result {
        warn!("Failed to set COLLATERAL allowance: {}", e);
    }

    // Enable CONDITIONAL token allowance
    let conditional_result = set_allowance(
        &client,
        &session.wallet_address,
        &req.api_key,
        &req.api_passphrase,
        &secret_bytes,
        "CONDITIONAL",
    ).await;

    if let Err(e) = &conditional_result {
        warn!("Failed to set CONDITIONAL allowance: {}", e);
    }

    // Return success if at least one worked
    match (collateral_result, conditional_result) {
        (Ok(_), Ok(_)) => Ok(Json(EnableTradingResponse {
            success: true,
            error: None,
        })),
        (Err(e1), Err(e2)) => Ok(Json(EnableTradingResponse {
            success: false,
            error: Some(format!("COLLATERAL: {}, CONDITIONAL: {}", e1, e2)),
        })),
        (Err(e), Ok(_)) => Ok(Json(EnableTradingResponse {
            success: true,
            error: Some(format!("COLLATERAL failed: {}", e)),
        })),
        (Ok(_), Err(e)) => Ok(Json(EnableTradingResponse {
            success: true,
            error: Some(format!("CONDITIONAL failed: {}", e)),
        })),
    }
}

/// Helper to set a specific allowance type
async fn set_allowance(
    client: &reqwest::Client,
    wallet_address: &str,
    api_key: &str,
    api_passphrase: &str,
    secret_bytes: &[u8],
    asset_type: &str,
) -> Result<(), String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let payload = serde_json::json!({
        "asset_type": asset_type
    });

    let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;

    let path = "/balance-allowance";
    let method = "POST";
    // Polymarket uses milliseconds for timestamp
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    // Create L2 signature
    let sig_payload = format!("{}{}{}{}", timestamp, method, path, body);

    let mut mac = HmacSha256::new_from_slice(secret_bytes)
        .map_err(|e| format!("Invalid secret: {}", e))?;
    mac.update(sig_payload.as_bytes());

    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    info!("Setting {} allowance for {}", asset_type, wallet_address);

    let response = client
        .post("https://clob.polymarket.com/balance-allowance")
        .header("Content-Type", "application/json")
        .header("POLY_ADDRESS", wallet_address)
        .header("POLY_SIGNATURE", &signature)
        .header("POLY_TIMESTAMP", &timestamp)
        .header("POLY_API_KEY", api_key)
        .header("POLY_PASSPHRASE", api_passphrase)
        .body(body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if response.status().is_success() {
        let result: serde_json::Value = response.json().await.unwrap_or_default();
        info!("Allowance response for {}: {:?}", asset_type, result);
        Ok(())
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(format!("API error {}: {}", status, body))
    }
}
