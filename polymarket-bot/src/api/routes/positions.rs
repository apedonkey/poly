//! Position API endpoints

use crate::api::server::AppState;
use crate::types::{BotStats, Position};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use axum_extra::{
    headers::{authorization::Bearer, Authorization},
    TypedHeader,
};
use serde::{Deserialize, Serialize};

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
