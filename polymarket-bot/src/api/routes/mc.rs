//! Millionaires Club API routes

use crate::api::server::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct McStatusResponse {
    pub status: Option<crate::services::mc_scanner::McStatusUpdate>,
}

/// GET /api/mc/status — current MC status
pub async fn get_status(
    State(state): State<AppState>,
) -> Json<McStatusResponse> {
    let status = state.mc_status.read().await.clone();
    Json(McStatusResponse { status })
}

#[derive(Debug, Serialize)]
pub struct McScoutLogResponse {
    pub logs: Vec<crate::services::mc_scanner::McScoutResult>,
    pub total: i64,
}

/// GET /api/mc/scout-log — paginated scout evaluation history
pub async fn get_scout_log(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Json<McScoutLogResponse> {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    match state.db.mc_get_scout_log(limit, offset).await {
        Ok((logs, total)) => Json(McScoutLogResponse { logs, total }),
        Err(e) => {
            tracing::warn!("Failed to get MC scout log: {}", e);
            Json(McScoutLogResponse { logs: vec![], total: 0 })
        }
    }
}

#[derive(Debug, Serialize)]
pub struct McTradeRow {
    pub id: i64,
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub slug: String,
    pub side: String,
    pub entry_price: String,
    pub exit_price: Option<String>,
    pub size: String,
    pub shares: String,
    pub pnl: Option<String>,
    pub certainty_score: i32,
    pub category: Option<String>,
    pub status: String,
    pub tier_at_entry: i32,
    pub token_id: Option<String>,
    pub end_date: Option<String>,
    pub opened_at: String,
    pub closed_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct McTradesResponse {
    pub trades: Vec<McTradeRow>,
    pub total: i64,
}

/// GET /api/mc/trades — paginated simulated trade history
pub async fn get_trades(
    State(state): State<AppState>,
    Query(params): Query<PaginationParams>,
) -> Json<McTradesResponse> {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    match state.db.mc_get_trades(limit, offset).await {
        Ok((trades, total)) => {
            let rows: Vec<McTradeRow> = trades.into_iter().map(|t| McTradeRow {
                id: t.id,
                market_id: t.market_id,
                condition_id: t.condition_id,
                question: t.question,
                slug: t.slug,
                side: t.side,
                entry_price: t.entry_price,
                exit_price: t.exit_price,
                size: t.size,
                shares: t.shares,
                pnl: t.pnl,
                certainty_score: t.certainty_score,
                category: t.category,
                status: t.status,
                tier_at_entry: t.tier_at_entry,
                token_id: t.token_id,
                end_date: t.end_date,
                opened_at: t.opened_at,
                closed_at: t.closed_at,
            }).collect();
            Json(McTradesResponse { trades: rows, total })
        }
        Err(e) => {
            tracing::warn!("Failed to get MC trades: {}", e);
            Json(McTradesResponse { trades: vec![], total: 0 })
        }
    }
}

#[derive(Debug, Serialize)]
pub struct McTierHistoryEntry {
    pub id: i64,
    pub from_tier: i32,
    pub to_tier: i32,
    pub bankroll: String,
    pub reason: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct McTierHistoryResponse {
    pub history: Vec<McTierHistoryEntry>,
}

/// GET /api/mc/tier-history — all tier transitions
pub async fn get_tier_history(
    State(state): State<AppState>,
) -> Json<McTierHistoryResponse> {
    match state.db.mc_get_tier_history().await {
        Ok(history) => {
            let entries: Vec<McTierHistoryEntry> = history.into_iter().map(|h| McTierHistoryEntry {
                id: h.id,
                from_tier: h.from_tier,
                to_tier: h.to_tier,
                bankroll: h.bankroll,
                reason: h.reason,
                timestamp: h.timestamp,
            }).collect();
            Json(McTierHistoryResponse { history: entries })
        }
        Err(e) => {
            tracing::warn!("Failed to get MC tier history: {}", e);
            Json(McTierHistoryResponse { history: vec![] })
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct McConfigUpdate {
    pub bankroll: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct McConfigResponse {
    pub success: bool,
    pub message: String,
}

/// PUT /api/mc/config — update bankroll or mode
pub async fn update_config(
    State(state): State<AppState>,
    Json(body): Json<McConfigUpdate>,
) -> Json<McConfigResponse> {
    if let Some(ref bankroll) = body.bankroll {
        // Validate bankroll is a valid number
        if bankroll.parse::<f64>().is_err() {
            return Json(McConfigResponse {
                success: false,
                message: "Invalid bankroll value".to_string(),
            });
        }
        if let Err(e) = state.db.mc_update_bankroll(bankroll, bankroll).await {
            return Json(McConfigResponse {
                success: false,
                message: format!("Failed to update bankroll: {}", e),
            });
        }
    }

    if let Some(ref mode) = body.mode {
        if mode != "observation" && mode != "live" {
            return Json(McConfigResponse {
                success: false,
                message: "Mode must be 'observation' or 'live'".to_string(),
            });
        }
        if let Err(e) = state.db.mc_update_mode(mode).await {
            return Json(McConfigResponse {
                success: false,
                message: format!("Failed to update mode: {}", e),
            });
        }
    }

    Json(McConfigResponse {
        success: true,
        message: "Config updated".to_string(),
    })
}
