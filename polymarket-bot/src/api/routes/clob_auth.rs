//! CLOB Authentication endpoints for external wallet trading

use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::api::server::AppState;

/// Polymarket CLOB API endpoint
const CLOB_API: &str = "https://clob.polymarket.com";

/// Request to derive API credentials from a signed EIP-712 message
#[derive(Debug, Deserialize)]
pub struct DeriveApiKeyRequest {
    pub address: String,
    pub signature: String,
    pub timestamp: String,
    pub nonce: i64,
}

/// API credentials response from Polymarket
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ApiCredentials {
    pub api_key: String,
    pub secret: String,
    pub passphrase: String,
}

/// Our API response
#[derive(Debug, Serialize)]
pub struct ApiCredentialsResponse {
    pub api_key: String,
    pub api_secret: String,
    pub api_passphrase: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Get server timestamp from Polymarket (for synchronization)
pub async fn get_server_time() -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
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
        .get(format!("{}/time", CLOB_API))
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("Failed to get server time: {}", e),
                }),
            )
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("CLOB API error {}: {}", status, body),
            }),
        ));
    }

    let time: serde_json::Value = response.json().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to parse time: {}", e),
            }),
        )
    })?;

    Ok(Json(time))
}

/// Derive API credentials using L1 authentication headers
/// The frontend signs an EIP-712 message, we forward it to Polymarket
pub async fn derive_api_key(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<DeriveApiKeyRequest>,
) -> Result<Json<ApiCredentialsResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Verify the address matches the session wallet
    if req.address.to_lowercase() != session.wallet_address.to_lowercase() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "Address does not match session wallet".to_string(),
            }),
        ));
    }

    info!("Deriving API key for wallet {}", req.address);
    debug!("Timestamp: {}, Nonce: {}", req.timestamp, req.nonce);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to create client: {}", e),
                }),
            )
        })?;

    // Try to create API key first, fall back to derive if it already exists
    let response = client
        .post(format!("{}/auth/api-key", CLOB_API))
        .header("POLY_ADDRESS", &req.address)
        .header("POLY_SIGNATURE", &req.signature)
        .header("POLY_TIMESTAMP", &req.timestamp)
        .header("POLY_NONCE", req.nonce.to_string())
        .send()
        .await
        .map_err(|e| {
            warn!("Failed to call create-api-key: {}", e);
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorResponse {
                    error: format!("Failed to create API key: {}", e),
                }),
            )
        })?;

    // If create fails, try derive (for existing keys)
    let response = if !response.status().is_success() {
        info!("Create API key failed, trying derive...");
        client
            .get(format!("{}/auth/derive-api-key", CLOB_API))
            .header("POLY_ADDRESS", &req.address)
            .header("POLY_SIGNATURE", &req.signature)
            .header("POLY_TIMESTAMP", &req.timestamp)
            .header("POLY_NONCE", req.nonce.to_string())
            .send()
            .await
            .map_err(|e| {
                warn!("Failed to call derive-api-key: {}", e);
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("Failed to derive API key: {}", e),
                    }),
                )
            })?
    } else {
        response
    };

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        warn!("derive-api-key failed: {} - {}", status, body);
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("Polymarket auth failed: {} - {}", status, body),
            }),
        ));
    }

    let credentials: ApiCredentials = response.json().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Failed to parse credentials: {}", e),
            }),
        )
    })?;

    // Store credentials in database for this wallet
    state
        .db
        .store_api_credentials(
            &session.wallet_address,
            &credentials.api_key,
            &credentials.secret,
            &credentials.passphrase,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Failed to store credentials: {}", e),
                }),
            )
        })?;

    info!("API credentials stored for wallet {}", session.wallet_address);

    Ok(Json(ApiCredentialsResponse {
        api_key: credentials.api_key,
        api_secret: credentials.secret,
        api_passphrase: credentials.passphrase,
    }))
}
