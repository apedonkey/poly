//! Opportunity API endpoints

use crate::api::server::AppState;
use crate::types::Opportunity;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Query parameters for listing opportunities
#[derive(Debug, Deserialize)]
pub struct ListOpportunitiesQuery {
    /// Filter by strategy: "sniper", "nobias", or "all" (default)
    pub strategy: Option<String>,
    /// Maximum number to return
    pub limit: Option<usize>,
}

/// Opportunities response
#[derive(Debug, Serialize)]
pub struct OpportunitiesResponse {
    pub opportunities: Vec<Opportunity>,
    pub total: usize,
    pub last_scan: Option<DateTime<Utc>>,
}

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// List current opportunities
pub async fn list_opportunities(
    State(state): State<AppState>,
    Query(query): Query<ListOpportunitiesQuery>,
) -> Result<Json<OpportunitiesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let opportunities = state.opportunities.read().await;

    let filtered: Vec<Opportunity> = opportunities
        .iter()
        .filter(|opp| {
            match query.strategy.as_deref() {
                Some("sniper") => matches!(opp.strategy, crate::types::StrategyType::ResolutionSniper),
                Some("nobias") => matches!(opp.strategy, crate::types::StrategyType::NoBias),
                _ => true, // "all" or no filter
            }
        })
        .take(query.limit.unwrap_or(50))
        .cloned()
        .collect();

    let total = filtered.len();

    Ok(Json(OpportunitiesResponse {
        opportunities: filtered,
        total,
        last_scan: Some(Utc::now()), // TODO: Track actual last scan time
    }))
}
