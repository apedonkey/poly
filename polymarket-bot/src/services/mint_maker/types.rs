//! Types for Mint Maker status broadcasts

use crate::db::{MintMakerLogEntry, MintMakerPairRow, MintMakerSettingsRow};
use serde::{Deserialize, Serialize};

/// Broadcast to frontend via WebSocket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintMakerStatusUpdate {
    pub enabled: bool,
    pub active_markets: Vec<MintMakerMarketStatus>,
    pub stats: MintMakerStatsSnapshot,
    pub open_pairs: Vec<MintMakerPairRow>,
    pub recent_log: Vec<MintMakerLogEntry>,
    pub settings: Option<MintMakerSettingsRow>,
}

/// Per-market status for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintMakerMarketStatus {
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub asset: String,
    pub yes_token_id: String,
    pub no_token_id: String,
    pub yes_price: String,
    pub no_price: String,
    pub yes_bid: Option<String>,
    pub no_bid: Option<String>,
    pub spread_profit: Option<String>,
    pub slug: String,
    pub minutes_left: f64,
    pub open_pairs: i64,
}

/// Stats snapshot for frontend
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MintMakerStatsSnapshot {
    pub total_pairs: i64,
    pub merged_pairs: i64,
    pub cancelled_pairs: i64,
    pub total_profit: String,
    pub total_cost: String,
    pub avg_spread: String,
    pub fill_rate: String,
}
