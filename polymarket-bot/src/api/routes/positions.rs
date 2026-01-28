//! Position API endpoints

use crate::api::server::AppState;
use crate::types::{BotStats, Position};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use base64::Engine;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Query parameters for listing positions
#[derive(Debug, Deserialize)]
pub struct ListPositionsQuery {
    /// Filter by status: "open", "closed", or "all" (default)
    pub status: Option<String>,
}

/// Positions response
#[derive(Debug, Serialize)]
pub struct PositionsResponse {
    pub positions: Vec<Position>,
    pub total: usize,
}

/// Stats response
#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub stats: BotStats,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// List positions for authenticated wallet
pub async fn list_positions(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Query(query): Query<ListPositionsQuery>,
) -> Result<Json<PositionsResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Get positions based on filter
    let positions = match query.status.as_deref() {
        Some("open") => state
            .db
            .get_open_positions_for_wallet(&session.wallet_address)
            .await,
        _ => state
            .db
            .get_positions_for_wallet(&session.wallet_address)
            .await,
    }
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Database error: {}", e),
            }),
        )
    })?;

    let total = positions.len();

    Ok(Json(PositionsResponse { positions, total }))
}

/// Get stats for authenticated wallet
pub async fn get_stats(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    let stats = state
        .db
        .get_stats_for_wallet(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(StatsResponse { stats }))
}

/// Request to update position token_id (for backfilling)
#[derive(Debug, Deserialize)]
pub struct UpdateTokenIdRequest {
    pub token_id: String,
}

/// Request to update position entry_price (for corrections)
#[derive(Debug, Deserialize)]
pub struct UpdateEntryPriceRequest {
    pub entry_price: String,
}

/// Update entry_price for a position (fix incorrect entry prices)
pub async fn update_entry_price(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Path(position_id): Path<i64>,
    Json(req): Json<UpdateEntryPriceRequest>,
) -> Result<Json<UpdateTokenIdResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Validate entry_price format
    let _price = Decimal::from_str(&req.entry_price).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid entry_price format".to_string(),
            }),
        )
    })?;

    // Update the entry_price
    state
        .db
        .update_position_entry_price(&session.wallet_address, position_id, &req.entry_price)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(UpdateTokenIdResponse { success: true }))
}

/// Response for update token_id
#[derive(Debug, Serialize)]
pub struct UpdateTokenIdResponse {
    pub success: bool,
}

/// Update token_id for a position (backfill for existing positions)
pub async fn update_token_id(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Path(position_id): Path<i64>,
    Json(req): Json<UpdateTokenIdRequest>,
) -> Result<Json<UpdateTokenIdResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Update the token_id
    state
        .db
        .update_position_token_id(&session.wallet_address, position_id, &req.token_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(UpdateTokenIdResponse { success: true }))
}

/// Request to close a position (full or partial)
#[derive(Debug, Deserialize)]
pub struct ClosePositionRequest {
    pub exit_price: String,
    pub order_id: Option<String>,
    /// If provided, sell only this many shares (partial sell). If None, sells all remaining.
    pub sell_shares: Option<String>,
}

/// Response for close position
#[derive(Debug, Serialize)]
pub struct ClosePositionResponse {
    pub success: bool,
    /// PnL from this specific sell (may be partial)
    pub pnl: Option<String>,
    /// Shares remaining after this sell (None = fully closed, backward compat)
    pub remaining_shares: Option<String>,
    /// Whether the position is now fully closed
    pub is_fully_closed: bool,
    /// Total realized PnL from all partial sells (for partial positions)
    pub total_realized_pnl: Option<String>,
}

/// Response for redeem position
#[derive(Debug, Serialize)]
pub struct RedeemPositionResponse {
    pub success: bool,
    pub transaction_id: Option<String>,
    pub message: Option<String>,
}

/// Redeem a resolved winning position (claim USDC)
pub async fn redeem_position(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Path(position_id): Path<i64>,
) -> Result<Json<RedeemPositionResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Get the position
    let position = state
        .db
        .get_position_by_id(&session.wallet_address, position_id)
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
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Position not found".to_string(),
                }),
            )
        })?;

    // Verify position is resolved with positive PnL
    if position.status != crate::types::PositionStatus::Resolved {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Position is not resolved yet".to_string(),
            }),
        ));
    }

    let pnl = position.pnl.unwrap_or(Decimal::ZERO);
    if pnl <= Decimal::ZERO {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Position did not win - nothing to claim".to_string(),
            }),
        ));
    }

    if position.is_paper {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Cannot redeem paper trades".to_string(),
            }),
        ));
    }

    // Get builder credentials
    let api_key = state.config.builder_api_key.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Builder API key not configured".to_string(),
            }),
        )
    })?;
    let secret = state.config.builder_secret.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Builder secret not configured".to_string(),
            }),
        )
    })?;
    let passphrase = state.config.builder_passphrase.as_ref().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "Builder passphrase not configured".to_string(),
            }),
        )
    })?;

    // CTF contract address on Polygon
    const CTF_CONTRACT: &str = "0x4d97dcd97ec945f40cf65f87097ace5ea0476045";
    const USDC_ADDRESS: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";

    // Determine index set based on winning side
    // For binary markets: YES = index 0 (indexSet = 1), NO = index 1 (indexSet = 2)
    let index_set = match position.side {
        crate::types::Side::Yes => 1u64,
        crate::types::Side::No => 2u64,
    };

    // Build the redeemPositions call data using ABI encoding
    // Function signature: redeemPositions(address,bytes32,bytes32,uint256[])
    // Function selector: 0x01b7037c (verified from 4byte.directory)
    let condition_id = &position.market_id;

    // ABI encode the redeemPositions call
    // 1. Function selector (4 bytes): 0x01b7037c
    // 2. collateralToken (address, 32 bytes padded)
    // 3. parentCollectionId (bytes32, 32 bytes) - all zeros for Polymarket
    // 4. conditionId (bytes32, 32 bytes)
    // 5. offset to indexSets array (32 bytes) - points to position 128 (0x80)
    // 6. length of indexSets array (32 bytes)
    // 7. indexSets[0] (32 bytes)

    let mut call_data = String::from("0x01b7037c");

    // collateralToken address (pad to 32 bytes, remove 0x prefix)
    let usdc_padded = format!("{:0>64}", USDC_ADDRESS.trim_start_matches("0x"));
    call_data.push_str(&usdc_padded);

    // parentCollectionId - all zeros for Polymarket
    call_data.push_str(&"0".repeat(64));

    // conditionId - the market's condition_id (should already be bytes32 hex)
    // Remove 0x prefix if present and pad to 64 chars
    let condition_hex = condition_id.trim_start_matches("0x");
    let condition_padded = format!("{:0>64}", condition_hex);
    call_data.push_str(&condition_padded);

    // Offset to dynamic array (4 * 32 = 128 = 0x80)
    call_data.push_str(&format!("{:0>64}", "80"));

    // Array length (1 element)
    call_data.push_str(&format!("{:0>64}", "1"));

    // indexSet value
    call_data.push_str(&format!("{:0>64x}", index_set));

    // Construct the transaction payload for the relay
    // The relay expects an array of transactions
    let redeem_payload = serde_json::json!({
        "transactions": [{
            "to": CTF_CONTRACT,
            "data": call_data,
            "value": "0"
        }],
        "description": format!("Redeem position {}", position_id)
    });

    // Submit via relay
    let relay_url = "https://relayer-v2.polymarket.com";
    let path = "/submit";
    let method = "POST";
    let body_str = serde_json::to_string(&redeem_payload).unwrap_or_default();

    // Create HMAC signature
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();
    let sig_payload = format!("{}{}{}{}", timestamp, method, path, body_str);

    let secret_bytes = base64::engine::general_purpose::STANDARD
        .decode(secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(secret))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(secret))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to decode builder secret: {}", e),
                }),
            )
        })?;

    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Invalid builder secret: {}", e),
            }),
        )
    })?;
    mac.update(sig_payload.as_bytes());
    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    // Make the relay request
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}{}", relay_url, path))
        .header("POLY_BUILDER_TIMESTAMP", &timestamp)
        .header("POLY_BUILDER_SIGNATURE", &signature)
        .header("POLY_BUILDER_API_KEY", api_key)
        .header("POLY_BUILDER_PASSPHRASE", passphrase)
        .header("Content-Type", "application/json")
        .body(body_str)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("Relay request failed: {}", e),
                }),
            )
        })?;

    let status = response.status();
    let response_text = response.text().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("Failed to read relay response: {}", e),
            }),
        )
    })?;

    if !status.is_success() {
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(ErrorResponse {
                error: format!("Relay error ({}): {}", status, response_text),
            }),
        ));
    }

    // Parse response to get transaction ID
    let response_json: serde_json::Value = serde_json::from_str(&response_text).unwrap_or_default();
    let tx_id = response_json.get("transactionId")
        .or_else(|| response_json.get("id"))
        .and_then(|v| v.as_str())
        .map(String::from);

    Ok(Json(RedeemPositionResponse {
        success: true,
        transaction_id: tx_id,
        message: Some("Redeem transaction submitted".to_string()),
    }))
}

/// Close a position (mark as sold) - supports full or partial sells
pub async fn close_position(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Path(position_id): Path<i64>,
    Json(req): Json<ClosePositionRequest>,
) -> Result<Json<ClosePositionResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Parse exit price
    let exit_price = Decimal::from_str(&req.exit_price).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid exit_price".to_string(),
            }),
        )
    })?;

    // Check if this is a partial sell
    if let Some(sell_shares_str) = &req.sell_shares {
        // Partial sell - use the new partial close method
        let sell_shares = Decimal::from_str(sell_shares_str).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid sell_shares".to_string(),
                }),
            )
        })?;

        let result = state
            .db
            .partial_close_position_for_wallet(&session.wallet_address, position_id, sell_shares, exit_price)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?;

        Ok(Json(ClosePositionResponse {
            success: true,
            pnl: Some(result.pnl_this_sell.to_string()),
            remaining_shares: Some(result.remaining_shares.to_string()),
            is_fully_closed: result.is_fully_closed,
            total_realized_pnl: Some(result.total_realized_pnl.to_string()),
        }))
    } else {
        // Full sell (legacy behavior) - close all remaining shares
        let pnl = state
            .db
            .close_position_for_wallet(&session.wallet_address, position_id, exit_price)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: format!("Database error: {}", e),
                    }),
                )
            })?;

        Ok(Json(ClosePositionResponse {
            success: true,
            pnl: Some(pnl.to_string()),
            remaining_shares: Some("0".to_string()),
            is_fully_closed: true,
            total_realized_pnl: Some(pnl.to_string()),
        }))
    }
}
