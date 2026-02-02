//! Open Orders API endpoints
//! Fetches and manages orders from Polymarket CLOB

use crate::api::server::AppState;
use crate::services::{EndpointClass, derive_safe_wallet};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Open order from CLOB
#[derive(Debug, Serialize, Deserialize)]
pub struct OpenOrder {
    pub id: String,
    pub market: String,
    pub asset_id: String,
    pub side: String,
    pub original_size: String,
    pub size_matched: String,
    pub price: String,
    pub status: String,
    pub created_at: Option<String>,
    pub expiration: Option<String>,
    pub order_type: String,
    /// Market question (looked up from our DB or opportunities)
    pub market_question: Option<String>,
}

/// Response for open orders
#[derive(Debug, Serialize)]
pub struct OpenOrdersResponse {
    pub orders: Vec<OpenOrder>,
    pub total: usize,
}

/// Response for cancel order
#[derive(Debug, Serialize)]
pub struct CancelOrderResponse {
    pub success: bool,
    pub message: Option<String>,
}

/// Get open orders for the authenticated wallet
pub async fn get_open_orders(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<OpenOrdersResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    let (api_key, api_secret, api_passphrase) = match credentials {
        Some(creds) => creds,
        None => {
            // No API credentials - return empty list instead of error
            return Ok(Json(OpenOrdersResponse {
                orders: vec![],
                total: 0,
            }));
        }
    };

    // Derive proxy wallet address for external wallets
    let proxy_address = derive_safe_wallet(&session.wallet_address)
        .unwrap_or_else(|_| session.wallet_address.clone());

    info!("Fetching orders for EOA: {} -> Proxy: {}", session.wallet_address, proxy_address);

    // Rate limit general API requests
    if state.rate_limiter.acquire(EndpointClass::General).await {
        state.metrics.inc_api_rate_limited();
    }
    state.metrics.inc_api_calls();

    // Fetch open orders from CLOB using proxy address
    let orders = match fetch_open_orders_from_clob(
        &session.wallet_address, // EOA for auth headers
        &proxy_address,          // Proxy for maker filter
        &api_key,
        &api_secret,
        &api_passphrase,
    )
    .await {
        Ok(orders) => orders,
        Err(e) => {
            // Log error but return empty list instead of failing
            state.metrics.inc_api_errors();
            info!("Failed to fetch orders from CLOB: {}. Returning empty list.", e);
            vec![]
        }
    };

    let total = orders.len();

    // Try to enrich orders with market questions from our opportunities
    let opportunities = state.opportunities.read().await;
    let enriched_orders: Vec<OpenOrder> = orders
        .into_iter()
        .map(|mut order| {
            // Try to find matching opportunity by token_id
            if let Some(opp) = opportunities.iter().find(|o| {
                o.token_id.as_ref() == Some(&order.asset_id)
            }) {
                order.market_question = Some(opp.question.clone());
            }
            order
        })
        .collect();

    Ok(Json(OpenOrdersResponse {
        orders: enriched_orders,
        total,
    }))
}

/// Cancel an order by ID
pub async fn cancel_order(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Path(order_id): Path<String>,
) -> Result<Json<CancelOrderResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Get API credentials
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
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No API credentials found".to_string(),
                }),
            )
        })?;

    let (api_key, api_secret, api_passphrase) = credentials;

    info!("Cancelling order {} for wallet {}", order_id, session.wallet_address);

    // Rate limit cancel requests
    if state.rate_limiter.acquire(EndpointClass::DeleteOrder).await {
        state.metrics.inc_api_rate_limited();
    }
    state.metrics.inc_api_calls();

    // Cancel the order via CLOB API (use EOA for auth)
    cancel_order_on_clob(
        &order_id,
        &session.wallet_address, // EOA for auth
        &api_key,
        &api_secret,
        &api_passphrase,
    )
    .await
    .map_err(|e| {
        state.metrics.inc_api_errors();
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("Failed to cancel order: {}", e),
            }),
        )
    })?;

    state.metrics.inc_orders_cancelled();
    Ok(Json(CancelOrderResponse {
        success: true,
        message: Some(format!("Order {} cancelled", order_id)),
    }))
}

/// Fetch open orders from Polymarket CLOB API
/// - eoa_address: The EOA address for L2 auth headers
/// - proxy_address: The proxy wallet address to filter orders by maker
async fn fetch_open_orders_from_clob(
    eoa_address: &str,
    proxy_address: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<Vec<OpenOrder>, String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    // Correct endpoint: GET /data/orders
    // Optional query params: id, market, asset_id
    // L2 auth headers authenticate the user and filter to their orders
    let _ = proxy_address; // Auth headers determine which user's orders to return
    let path = "/data/orders";
    let method = "GET";
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    // Create L2 signature - for GET requests, signature is just timestamp+method+path (no body)
    let sig_payload = format!("{}{}{}", timestamp, method, path);

    // Decode the base64url-encoded secret
    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(api_secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(api_secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(api_secret))
        .map_err(|e| format!("Failed to decode API secret: {}", e))?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes)
        .map_err(|e| format!("Invalid API secret: {}", e))?;
    mac.update(sig_payload.as_bytes());

    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    let url = format!("https://clob.polymarket.com{}", path);
    info!("Fetching orders from: {}", url);

    let response = client
        .get(&url)
        .header("Content-Type", "application/json")
        .header("POLY_ADDRESS", eoa_address)
        .header("POLY_SIGNATURE", &signature)
        .header("POLY_TIMESTAMP", &timestamp)
        .header("POLY_API_KEY", api_key)
        .header("POLY_PASSPHRASE", api_passphrase)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("CLOB API error {}: {}", status, body));
    }

    // Parse the response - might be a direct array or wrapped in an object
    let response_text = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    info!("CLOB orders response (first 500 chars): {}", &response_text[..response_text.len().min(500)]);

    let orders_data: Vec<serde_json::Value> = if response_text.starts_with('[') {
        // Direct array
        serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse array response: {}", e))?
    } else {
        // Might be wrapped in an object like { "orders": [...] } or { "data": [...] }
        let obj: serde_json::Value = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        if let Some(orders) = obj.get("orders").and_then(|v| v.as_array()) {
            orders.clone()
        } else if let Some(data) = obj.get("data").and_then(|v| v.as_array()) {
            data.clone()
        } else if let Some(arr) = obj.as_array() {
            arr.clone()
        } else {
            info!("Unexpected response format, returning empty");
            vec![]
        }
    };

    // Convert to our OpenOrder struct, filtering for open/live orders only
    // Polymarket OpenOrder schema:
    // id, status, owner, maker_address, market, asset_id, side, original_size,
    // size_matched, price, associate_trades, outcome, created_at (number), expiration, order_type
    let orders: Vec<OpenOrder> = orders_data
        .into_iter()
        .filter_map(|order| {
            let status = order.get("status")?.as_str()?.to_string();
            // Only include live/open orders (not filled, cancelled, etc.)
            if status != "LIVE" && status != "OPEN" && status != "live" && status != "open" {
                return None;
            }

            Some(OpenOrder {
                id: order.get("id")?.as_str()?.to_string(),
                market: order.get("market").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                asset_id: order.get("asset_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                side: order.get("side").and_then(|v| v.as_str()).unwrap_or("BUY").to_string(),
                original_size: order.get("original_size").and_then(|v| v.as_str()).unwrap_or("0").to_string(),
                size_matched: order.get("size_matched").and_then(|v| v.as_str()).unwrap_or("0").to_string(),
                price: order.get("price").and_then(|v| v.as_str()).unwrap_or("0").to_string(),
                status,
                // created_at is a number (timestamp) in Polymarket API
                created_at: order.get("created_at")
                    .and_then(|v| v.as_i64())
                    .map(|ts| ts.to_string())
                    .or_else(|| order.get("created_at").and_then(|v| v.as_str()).map(|s| s.to_string())),
                expiration: order.get("expiration").and_then(|v| v.as_str()).map(|s| s.to_string()),
                order_type: order.get("order_type").and_then(|v| v.as_str()).unwrap_or("GTC").to_string(),
                market_question: None,
            })
        })
        .collect();

    info!("Fetched {} open orders from CLOB", orders.len());
    Ok(orders)
}

/// Cancel an order on Polymarket CLOB
async fn cancel_order_on_clob(
    order_id: &str,
    wallet_address: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<(), String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    let path = "/order";
    let method = "DELETE";
    let body = serde_json::json!({ "orderID": order_id }).to_string();
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    // Create L2 signature (includes body for DELETE with payload)
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

    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    let url = format!("https://clob.polymarket.com{}", path);
    let response = client
        .delete(&url)
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

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("CLOB API error {}: {}", status, body));
    }

    info!("Order {} cancelled successfully", order_id);
    Ok(())
}

/// Cancel all open orders for the authenticated wallet
pub async fn cancel_all_orders(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<CancelOrderResponse>, (StatusCode, Json<ErrorResponse>)> {
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
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No API credentials found".to_string(),
                }),
            )
        })?;

    let (api_key, api_secret, api_passphrase) = credentials;

    info!("Cancelling ALL orders for wallet {}", session.wallet_address);

    if state.rate_limiter.acquire(EndpointClass::DeleteOrder).await {
        state.metrics.inc_api_rate_limited();
    }
    state.metrics.inc_api_calls();

    cancel_all_on_clob(
        &session.wallet_address,
        &api_key,
        &api_secret,
        &api_passphrase,
    )
    .await
    .map_err(|e| {
        state.metrics.inc_api_errors();
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("Failed to cancel all orders: {}", e),
            }),
        )
    })?;

    Ok(Json(CancelOrderResponse {
        success: true,
        message: Some("All orders cancelled".to_string()),
    }))
}

/// Cancel all orders for a specific market
pub async fn cancel_market_orders(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Path(market_id): Path<String>,
) -> Result<Json<CancelOrderResponse>, (StatusCode, Json<ErrorResponse>)> {
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
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "No API credentials found".to_string(),
                }),
            )
        })?;

    let (api_key, api_secret, api_passphrase) = credentials;

    info!("Cancelling orders for market {} wallet {}", market_id, session.wallet_address);

    if state.rate_limiter.acquire(EndpointClass::DeleteOrder).await {
        state.metrics.inc_api_rate_limited();
    }
    state.metrics.inc_api_calls();

    cancel_market_on_clob(
        &market_id,
        &session.wallet_address,
        &api_key,
        &api_secret,
        &api_passphrase,
    )
    .await
    .map_err(|e| {
        state.metrics.inc_api_errors();
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("Failed to cancel market orders: {}", e),
            }),
        )
    })?;

    Ok(Json(CancelOrderResponse {
        success: true,
        message: Some(format!("All orders for market {} cancelled", market_id)),
    }))
}

/// Cancel all orders on Polymarket CLOB
async fn cancel_all_on_clob(
    wallet_address: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<(), String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    let path = "/cancel-all";
    let method = "DELETE";
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    let sig_payload = format!("{}{}{}", timestamp, method, path);

    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(api_secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(api_secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(api_secret))
        .map_err(|e| format!("Failed to decode API secret: {}", e))?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes)
        .map_err(|e| format!("Invalid API secret: {}", e))?;
    mac.update(sig_payload.as_bytes());

    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    let url = format!("https://clob.polymarket.com{}", path);
    let response = client
        .delete(&url)
        .header("Content-Type", "application/json")
        .header("POLY_ADDRESS", wallet_address)
        .header("POLY_SIGNATURE", &signature)
        .header("POLY_TIMESTAMP", &timestamp)
        .header("POLY_API_KEY", api_key)
        .header("POLY_PASSPHRASE", api_passphrase)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("CLOB API error {}: {}", status, body));
    }

    info!("All orders cancelled successfully for {}", wallet_address);
    Ok(())
}

/// Cancel all orders for a market on Polymarket CLOB
async fn cancel_market_on_clob(
    market_id: &str,
    wallet_address: &str,
    api_key: &str,
    api_secret: &str,
    api_passphrase: &str,
) -> Result<(), String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| format!("Failed to create client: {}", e))?;

    let path = "/cancel-market-orders";
    let method = "DELETE";
    let body = serde_json::json!({ "market": market_id });
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    let sig_payload = format!("{}{}{}{}", timestamp, method, path, body_str);

    let secret_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(api_secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(api_secret))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(api_secret))
        .map_err(|e| format!("Failed to decode API secret: {}", e))?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes)
        .map_err(|e| format!("Invalid API secret: {}", e))?;
    mac.update(sig_payload.as_bytes());

    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    let url = format!("https://clob.polymarket.com{}", path);
    let response = client
        .delete(&url)
        .header("Content-Type", "application/json")
        .header("POLY_ADDRESS", wallet_address)
        .header("POLY_SIGNATURE", &signature)
        .header("POLY_TIMESTAMP", &timestamp)
        .header("POLY_API_KEY", api_key)
        .header("POLY_PASSPHRASE", api_passphrase)
        .body(body_str)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("CLOB API error {}: {}", status, body));
    }

    info!("Market orders cancelled for {} by {}", market_id, wallet_address);
    Ok(())
}

// ==================== ORDER LIFECYCLE ====================

/// Get order lifecycle history for a wallet
pub async fn get_order_lifecycle(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<Vec<crate::types::Order>>, (StatusCode, Json<ErrorResponse>)> {
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

    let orders = state
        .db
        .get_orders_for_wallet(&session.wallet_address, None)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(orders))
}
