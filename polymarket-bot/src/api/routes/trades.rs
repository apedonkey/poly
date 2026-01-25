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
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use base64::Engine;
use tracing::{info, warn};

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

    // TODO: Actually execute the trade using Executor with the decrypted private key
    // For now, just record as paper trade
    let position_id = state
        .db
        .create_position_for_wallet(
            &session.wallet_address,
            &req.market_id,
            &opportunity.map(|o| o.question.clone()).unwrap_or_default(),
            side,
            entry_price,
            size,
            StrategyType::ResolutionSniper, // TODO: Get from opportunity
            false, // Live trade
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

    let (entry_price, question, strategy) = match opportunity {
        Some(opp) => (opp.entry_price, opp.question.clone(), opp.strategy),
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No matching opportunity found for this market/side".to_string(),
                }),
            ));
        }
    };

    // Record the paper trade
    let position_id = state
        .db
        .create_position_for_wallet(
            &session.wallet_address,
            &req.market_id,
            &question,
            side,
            entry_price,
            size,
            strategy,
            true, // Paper trade
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
    pub side: String,
    pub size_usdc: String,
    pub entry_price: String,
    pub token_id: String,
    pub signed_order: SignedOrder,
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
        "Submitting signed order for wallet {} - market: {}, side: {:?}, size: {}",
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

    let order_result = match credentials {
        Some((api_key, api_secret, api_passphrase)) => {
            submit_signed_order_to_clob(&req.signed_order, &session.wallet_address, &api_key, &api_secret, &api_passphrase).await
        }
        None => {
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

    // Record the position regardless of CLOB result
    let position_id = state
        .db
        .create_position_for_wallet(
            &session.wallet_address,
            &req.market_id,
            &req.question,
            side,
            entry_price,
            size,
            StrategyType::ResolutionSniper, // Default to sniper for now
            false, // Live trade
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
    pub side: String,
    pub size_usdc: String,
    pub entry_price: String,
    pub token_id: String,
    pub order_id: Option<String>,
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

    // Record the position
    let position_id = state
        .db
        .create_position_for_wallet(
            &session.wallet_address,
            &req.market_id,
            &req.question,
            side,
            entry_price,
            size,
            StrategyType::ResolutionSniper,
            false, // Live trade
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
async fn submit_signed_order_to_clob(
    order: &SignedOrder,
    wallet_address: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<String, String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    // Build the order payload per Polymarket docs
    // Side should be "BUY" or "SELL" string, not number
    let side_str = if order.side == 0 { "BUY" } else { "SELL" };

    let payload = serde_json::json!({
        "order": {
            "salt": order.salt,
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
            "signatureType": order.signature_type,
            "signature": order.signature
        },
        "owner": wallet_address,
        "orderType": "FOK"
    });

    let body = serde_json::to_string(&payload).map_err(|e| format!("Serialize error: {}", e))?;
    let path = "/order";
    let method = "POST";
    let timestamp = chrono::Utc::now().timestamp().to_string();

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

    // Encode signature as base64url
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

    let response = client
        .post("https://clob.polymarket.com/order")
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
