//! Builder relay signing and proxy endpoints
//! Handles HMAC signing for Polymarket's builder relay service

use crate::api::server::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use tracing::{info, error};

type HmacSha256 = Hmac<Sha256>;

const RELAY_URL: &str = "https://relayer-v2.polymarket.com";

/// Request for builder signature
#[derive(Debug, Deserialize)]
pub struct BuilderSignRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: Option<String>,
}

/// Response with builder signature headers
#[derive(Debug, Serialize)]
pub struct BuilderSignResponse {
    pub timestamp: String,
    pub signature: String,
    pub api_key: String,
    pub passphrase: String,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Generate HMAC signature for builder relay requests
/// This endpoint is called by the frontend before making relay API calls
pub async fn sign_builder_request(
    State(state): State<AppState>,
    Json(req): Json<BuilderSignRequest>,
) -> Result<Json<BuilderSignResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Get builder credentials from config
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

    // Get current timestamp in milliseconds (matches JavaScript Date.now())
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    // Build signature payload: timestamp + method + path + body
    let body_str = req.body.as_deref().unwrap_or("");
    let sig_payload = format!("{}{}{}{}", timestamp, req.method, req.path, body_str);

    info!("Builder sign request: {} {}", req.method, req.path);

    // Decode the base64-encoded secret
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

    // Create HMAC signature
    let mut mac = HmacSha256::new_from_slice(&secret_bytes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Invalid builder secret: {}", e),
            }),
        )
    })?;
    mac.update(sig_payload.as_bytes());

    // Use URL-safe base64 encoding (matches Polymarket's SDK)
    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    Ok(Json(BuilderSignResponse {
        timestamp,
        signature,
        api_key: api_key.clone(),
        passphrase: passphrase.clone(),
    }))
}

/// Request for relay proxy (generic)
#[derive(Debug, Deserialize)]
pub struct RelayProxyRequest {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: Option<Value>,
}

/// Generic relay body - accepts any JSON
#[derive(Debug, Deserialize, Serialize)]
pub struct RelayBody(Value);

/// Helper to create builder auth headers
fn create_builder_headers(
    _api_key: &str,
    secret: &str,
    _passphrase: &str,
    method: &str,
    path: &str,
    body: &str,
) -> Result<(String, String), String> {
    // Use milliseconds for timestamp (Date.now() equivalent)
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();
    let sig_payload = format!("{}{}{}{}", timestamp, method, path, body);

    info!("HMAC signature payload: timestamp={}, method={}, path={}, body_len={}",
          timestamp, method, path, body.len());
    info!("HMAC signature payload preview: {}{}{}{}...", timestamp, method, path, &body[..body.len().min(100)]);

    let secret_bytes = base64::engine::general_purpose::STANDARD
        .decode(secret)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(secret))
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(secret))
        .map_err(|e| format!("Failed to decode secret: {}", e))?;

    let mut mac = HmacSha256::new_from_slice(&secret_bytes)
        .map_err(|e| format!("Invalid secret: {}", e))?;
    mac.update(sig_payload.as_bytes());
    // Use URL-safe base64 encoding (matches Polymarket's SDK)
    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    Ok((timestamp, signature))
}

/// Proxy requests to the relay service with builder auth
/// This allows the frontend to make relay calls through our backend
pub async fn relay_proxy(
    State(state): State<AppState>,
    Json(req): Json<RelayProxyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    // Get builder credentials from config
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

    // Get current timestamp in milliseconds (matches JavaScript Date.now())
    let timestamp = chrono::Utc::now().timestamp_millis().to_string();

    // Build signature payload: timestamp + method + path + body
    let body_str = req.body.as_ref()
        .map(|b| serde_json::to_string(b).unwrap_or_default())
        .unwrap_or_default();
    let sig_payload = format!("{}{}{}{}", timestamp, req.method, req.path, body_str);

    info!("Relay proxy: {} {}", req.method, req.path);

    // Decode the base64-encoded secret
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

    // Create HMAC signature
    let mut mac = HmacSha256::new_from_slice(&secret_bytes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("Invalid builder secret: {}", e),
            }),
        )
    })?;
    mac.update(sig_payload.as_bytes());
    // Use URL-safe base64 encoding (matches Polymarket's SDK)
    let signature = base64::engine::general_purpose::URL_SAFE.encode(mac.finalize().into_bytes());

    // Build the full URL
    let url = format!("{}{}", RELAY_URL, req.path);

    // Create HTTP client and make request
    let client = reqwest::Client::new();

    let mut request_builder = match req.method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Unsupported method: {}", req.method),
                }),
            ));
        }
    };

    // Add builder auth headers
    request_builder = request_builder
        .header("POLY_BUILDER_TIMESTAMP", &timestamp)
        .header("POLY_BUILDER_SIGNATURE", &signature)
        .header("POLY_BUILDER_API_KEY", api_key)
        .header("POLY_BUILDER_PASSPHRASE", passphrase)
        .header("Content-Type", "application/json");

    // Add body if present
    if let Some(body) = &req.body {
        request_builder = request_builder.json(body);
    }

    // Send request
    let response = request_builder.send().await.map_err(|e| {
        error!("Relay request failed: {}", e);
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("Relay request failed: {}", e),
            }),
        )
    })?;

    let status = response.status();
    let response_text = response.text().await.map_err(|e| {
        error!("Failed to read relay response: {}", e);
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("Failed to read relay response: {}", e),
            }),
        )
    })?;

    info!("Relay response status: {}, body: {}", status, &response_text[..response_text.len().min(200)]);

    if !status.is_success() {
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(ErrorResponse {
                error: format!("Relay error ({}): {}", status, response_text),
            }),
        ));
    }

    // Parse response as JSON
    let response_json: Value = serde_json::from_str(&response_text).map_err(|e| {
        error!("Failed to parse relay response: {}", e);
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("Failed to parse relay response: {}", e),
            }),
        )
    })?;

    Ok(Json(response_json))
}

/// Proxy POST /submit to relay (handles raw body for correct HMAC)
pub async fn relay_submit(
    State(state): State<AppState>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    // Get builder credentials
    let api_key = state.config.builder_api_key.as_ref().ok_or_else(|| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Builder API key not configured".to_string() }))
    })?;
    let secret = state.config.builder_secret.as_ref().ok_or_else(|| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Builder secret not configured".to_string() }))
    })?;
    let passphrase = state.config.builder_passphrase.as_ref().ok_or_else(|| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Builder passphrase not configured".to_string() }))
    })?;

    // Convert body to string - this is the exact string we'll sign and send
    let body_str = String::from_utf8_lossy(&body).to_string();
    let path = "/submit";
    let method = "POST";

    info!("Relay submit - body length: {}", body_str.len());
    info!("Relay submit - body: {}", &body_str[..body_str.len().min(500)]);

    let (timestamp, signature) = create_builder_headers(api_key, secret, passphrase, method, path, &body_str)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))?;

    let url = format!("{}{}", RELAY_URL, path);
    info!("Relay proxy: {} {} -> {}", method, path, url);

    let client = reqwest::Client::new();
    let response = client.post(&url)
        .header("POLY_BUILDER_TIMESTAMP", &timestamp)
        .header("POLY_BUILDER_SIGNATURE", &signature)
        .header("POLY_BUILDER_API_KEY", api_key)
        .header("POLY_BUILDER_PASSPHRASE", passphrase)
        .header("Content-Type", "application/json")
        .body(body_str.clone())
        .send()
        .await
        .map_err(|e| {
            error!("Relay request failed: {}", e);
            (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("Relay request failed: {}", e) }))
        })?;

    let status = response.status();
    let response_text = response.text().await.map_err(|e| {
        error!("Failed to read relay response: {}", e);
        (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("Failed to read response: {}", e) }))
    })?;

    info!("Relay submit response: {} - {}", status, &response_text[..response_text.len().min(500)]);

    if !status.is_success() {
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(ErrorResponse { error: format!("Relay error ({}): {}", status, response_text) }),
        ));
    }

    let response_json: Value = serde_json::from_str(&response_text).unwrap_or(Value::Null);
    Ok(Json(response_json))
}

/// Proxy GET /nonce to relay
pub async fn relay_nonce(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    let query = params.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    let path = if query.is_empty() { "/nonce".to_string() } else { format!("/nonce?{}", query) };
    proxy_to_relay(&state, "GET", &path, None).await
}

/// Proxy GET /relay (relay address) to relay
pub async fn relay_address(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    proxy_to_relay(&state, "GET", "/relay", None).await
}

/// Proxy GET /deployed to relay
pub async fn relay_deployed(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    let query = params.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    let path = if query.is_empty() { "/deployed".to_string() } else { format!("/deployed?{}", query) };
    proxy_to_relay(&state, "GET", &path, None).await
}

/// Proxy GET /transaction?id=... to relay
pub async fn relay_transaction(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    let tx_id = params.get("id").ok_or_else(|| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "Missing id parameter".to_string() }))
    })?;
    let path = format!("/transaction/{}", tx_id);
    proxy_to_relay(&state, "GET", &path, None).await
}

/// Common helper for proxying requests to the relay
async fn proxy_to_relay(
    state: &AppState,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    // Get builder credentials
    let api_key = state.config.builder_api_key.as_ref().ok_or_else(|| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Builder API key not configured".to_string() }))
    })?;
    let secret = state.config.builder_secret.as_ref().ok_or_else(|| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Builder secret not configured".to_string() }))
    })?;
    let passphrase = state.config.builder_passphrase.as_ref().ok_or_else(|| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "Builder passphrase not configured".to_string() }))
    })?;

    let body_str = body.map(|b| serde_json::to_string(b).unwrap_or_default()).unwrap_or_default();

    let (timestamp, signature) = create_builder_headers(api_key, secret, passphrase, method, path, &body_str)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })))?;

    let url = format!("{}{}", RELAY_URL, path);
    info!("Relay proxy: {} {} -> {}", method, path, url);

    let client = reqwest::Client::new();
    let mut request_builder = match method {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        _ => return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: format!("Unsupported method: {}", method) }))),
    };

    request_builder = request_builder
        .header("POLY_BUILDER_TIMESTAMP", &timestamp)
        .header("POLY_BUILDER_SIGNATURE", &signature)
        .header("POLY_BUILDER_API_KEY", api_key)
        .header("POLY_BUILDER_PASSPHRASE", passphrase)
        .header("Content-Type", "application/json");

    if let Some(b) = body {
        request_builder = request_builder.json(b);
    }

    let response = request_builder.send().await.map_err(|e| {
        error!("Relay request failed: {}", e);
        (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("Relay request failed: {}", e) }))
    })?;

    let status = response.status();
    let response_text = response.text().await.map_err(|e| {
        error!("Failed to read relay response: {}", e);
        (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: format!("Failed to read response: {}", e) }))
    })?;

    info!("Relay response: {} - {}", status, &response_text[..response_text.len().min(500)]);

    if !status.is_success() {
        return Err((
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            Json(ErrorResponse { error: format!("Relay error ({}): {}", status, response_text) }),
        ));
    }

    let response_json: Value = serde_json::from_str(&response_text).unwrap_or(Value::Null);
    Ok(Json(response_json))
}
