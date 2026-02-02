//! Market data proxy endpoints

use crate::api::server::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Query params for price history
#[derive(Debug, Deserialize)]
pub struct PriceHistoryQuery {
    /// Market/token ID
    pub market: String,
    /// Time interval (e.g., "1h", "1d")
    pub interval: Option<String>,
    /// Start timestamp (fidelity)
    pub fidelity: Option<u64>,
}

/// Price history data point
#[derive(Debug, Serialize, Deserialize)]
pub struct PriceHistoryPoint {
    pub t: i64,
    pub p: f64,
}

/// Get price history from Polymarket CLOB
pub async fn get_price_history(
    State(_state): State<AppState>,
    Query(params): Query<PriceHistoryQuery>,
) -> Result<Json<Vec<PriceHistoryPoint>>, (StatusCode, Json<serde_json::Value>)> {
    let interval = params.interval.as_deref().unwrap_or("1h");
    let fidelity = params.fidelity.unwrap_or(60); // default 60 min

    info!("Fetching price history for market={} interval={}", params.market, interval);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to create client: {}", e) })),
            )
        })?;

    let url = format!(
        "https://clob.polymarket.com/prices-history?market={}&interval={}&fidelity={}",
        params.market, interval, fidelity
    );

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("Failed to fetch price history: {}", e) })),
            )
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("CLOB API error {}: {}", status, body) })),
        ));
    }

    let history: serde_json::Value = response
        .json()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("Failed to parse response: {}", e) })),
            )
        })?;

    // The response is typically an object with "history" array
    let points = if let Some(arr) = history.get("history").and_then(|v| v.as_array()) {
        arr.iter()
            .filter_map(|v| {
                let t = v.get("t").and_then(|t| t.as_i64())?;
                let p = v.get("p").and_then(|p| p.as_f64())?;
                Some(PriceHistoryPoint { t, p })
            })
            .collect()
    } else if let Some(arr) = history.as_array() {
        arr.iter()
            .filter_map(|v| {
                let t = v.get("t").and_then(|t| t.as_i64())?;
                let p = v.get("p").and_then(|p| p.as_f64())?;
                Some(PriceHistoryPoint { t, p })
            })
            .collect()
    } else {
        vec![]
    };

    Ok(Json(points))
}

/// Get tick size for a token
pub async fn get_tick_size(
    State(state): State<AppState>,
    Query(params): Query<TickSizeQuery>,
) -> Json<TickSizeResponse> {
    let info = state
        .tick_size_cache
        .get_tick_size(&params.token_id)
        .await;

    Json(TickSizeResponse {
        token_id: params.token_id,
        tick_size: info.tick_size.to_string(),
    })
}

#[derive(Debug, Deserialize)]
pub struct TickSizeQuery {
    pub token_id: String,
}

#[derive(Debug, Serialize)]
pub struct TickSizeResponse {
    pub token_id: String,
    pub tick_size: String,
}

/// Get bot metrics snapshot
pub async fn get_metrics(
    State(state): State<AppState>,
) -> Json<crate::services::metrics::MetricsSnapshot> {
    // Update rate limiter utilization before taking snapshot
    let (general, post, delete) = state.rate_limiter.utilization().await;
    state.metrics.set_rate_limiter_util(general, post, delete).await;
    Json(state.metrics.snapshot().await)
}
