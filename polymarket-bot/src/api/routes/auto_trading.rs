//! Auto-trading API endpoints

use crate::api::server::AppState;
use crate::services::auto_trader::{AutoTradeLog, AutoTradingSettings, AutoTradingStats, UpdateSettingsRequest};
use axum::{
    extract::{Query, State},
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

/// Error response
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// Success response
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: Option<String>,
}

/// Settings response
#[derive(Debug, Serialize)]
pub struct SettingsResponse {
    pub settings: AutoTradingSettingsDto,
}

/// Settings DTO for frontend
#[derive(Debug, Serialize, Deserialize)]
pub struct AutoTradingSettingsDto {
    pub enabled: bool,
    pub auto_buy_enabled: bool,
    pub max_position_size: String,
    pub max_total_exposure: String,
    pub min_edge: f64,
    pub strategies: Vec<String>,
    pub take_profit_enabled: bool,
    pub take_profit_percent: f64,
    pub stop_loss_enabled: bool,
    pub stop_loss_percent: f64,
    pub trailing_stop_enabled: bool,
    pub trailing_stop_percent: f64,
    pub time_exit_enabled: bool,
    pub time_exit_hours: f64,
    pub max_positions: i32,
    pub cooldown_minutes: i32,
    pub max_daily_loss: String,
}

impl From<AutoTradingSettings> for AutoTradingSettingsDto {
    fn from(s: AutoTradingSettings) -> Self {
        Self {
            enabled: s.enabled,
            auto_buy_enabled: s.auto_buy_enabled,
            max_position_size: s.max_position_size.to_string(),
            max_total_exposure: s.max_total_exposure.to_string(),
            min_edge: s.min_edge,
            strategies: s.strategies,
            take_profit_enabled: s.take_profit_enabled,
            take_profit_percent: s.take_profit_percent,
            stop_loss_enabled: s.stop_loss_enabled,
            stop_loss_percent: s.stop_loss_percent,
            trailing_stop_enabled: s.trailing_stop_enabled,
            trailing_stop_percent: s.trailing_stop_percent,
            time_exit_enabled: s.time_exit_enabled,
            time_exit_hours: s.time_exit_hours,
            max_positions: s.max_positions,
            cooldown_minutes: s.cooldown_minutes,
            max_daily_loss: s.max_daily_loss.to_string(),
        }
    }
}

/// Stats response
#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub stats: AutoTradingStatsDto,
}

/// Stats DTO for frontend
#[derive(Debug, Serialize)]
pub struct AutoTradingStatsDto {
    pub total_trades: i64,
    pub win_count: i64,
    pub loss_count: i64,
    pub win_rate: f64,
    pub total_pnl: String,
    pub take_profit_count: i64,
    pub take_profit_pnl: String,
    pub stop_loss_count: i64,
    pub stop_loss_pnl: String,
    pub trailing_stop_count: i64,
    pub trailing_stop_pnl: String,
    pub time_exit_count: i64,
    pub time_exit_pnl: String,
    pub auto_buy_count: i64,
    pub best_trade_pnl: String,
    pub worst_trade_pnl: String,
    pub avg_hold_hours: f64,
}

impl From<AutoTradingStats> for AutoTradingStatsDto {
    fn from(s: AutoTradingStats) -> Self {
        Self {
            total_trades: s.total_trades,
            win_count: s.win_count,
            loss_count: s.loss_count,
            win_rate: s.win_rate,
            total_pnl: s.total_pnl.to_string(),
            take_profit_count: s.take_profit_count,
            take_profit_pnl: s.take_profit_pnl.to_string(),
            stop_loss_count: s.stop_loss_count,
            stop_loss_pnl: s.stop_loss_pnl.to_string(),
            trailing_stop_count: s.trailing_stop_count,
            trailing_stop_pnl: s.trailing_stop_pnl.to_string(),
            time_exit_count: s.time_exit_count,
            time_exit_pnl: s.time_exit_pnl.to_string(),
            auto_buy_count: s.auto_buy_count,
            best_trade_pnl: s.best_trade_pnl.to_string(),
            worst_trade_pnl: s.worst_trade_pnl.to_string(),
            avg_hold_hours: s.avg_hold_hours,
        }
    }
}

/// History response
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub history: Vec<AutoTradeLogDto>,
    pub total: usize,
}

/// Log entry DTO for frontend
#[derive(Debug, Serialize)]
pub struct AutoTradeLogDto {
    pub id: i64,
    pub position_id: Option<i64>,
    pub action: String,
    pub market_question: Option<String>,
    pub side: Option<String>,
    pub entry_price: Option<String>,
    pub exit_price: Option<String>,
    pub size: Option<String>,
    pub pnl: Option<String>,
    pub trigger_reason: Option<String>,
    pub created_at: String,
}

impl From<AutoTradeLog> for AutoTradeLogDto {
    fn from(log: AutoTradeLog) -> Self {
        Self {
            id: log.id.unwrap_or(0),
            position_id: log.position_id,
            action: log.action,
            market_question: log.market_question,
            side: log.side,
            entry_price: log.entry_price.map(|p| p.to_string()),
            exit_price: log.exit_price.map(|p| p.to_string()),
            size: log.size.map(|s| s.to_string()),
            pnl: log.pnl.map(|p| p.to_string()),
            trigger_reason: log.trigger_reason,
            created_at: log.created_at.to_rfc3339(),
        }
    }
}

/// History query params
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Get auto-trading settings for authenticated wallet
pub async fn get_settings(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<SettingsResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    let settings = state
        .db
        .get_auto_trading_settings(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(SettingsResponse {
        settings: settings.into(),
    }))
}

/// Update auto-trading settings
pub async fn update_settings(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<SettingsResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Get current settings
    let mut settings = state
        .db
        .get_auto_trading_settings(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    // Apply updates
    if let Some(enabled) = req.enabled {
        settings.enabled = enabled;
    }
    if let Some(auto_buy_enabled) = req.auto_buy_enabled {
        settings.auto_buy_enabled = auto_buy_enabled;
    }
    if let Some(max_position_size) = req.max_position_size {
        settings.max_position_size = Decimal::from_str(&max_position_size).unwrap_or(settings.max_position_size);
    }
    if let Some(max_total_exposure) = req.max_total_exposure {
        settings.max_total_exposure = Decimal::from_str(&max_total_exposure).unwrap_or(settings.max_total_exposure);
    }
    if let Some(min_edge) = req.min_edge {
        settings.min_edge = min_edge;
    }
    if let Some(strategies) = req.strategies {
        settings.strategies = strategies;
    }
    if let Some(take_profit_enabled) = req.take_profit_enabled {
        settings.take_profit_enabled = take_profit_enabled;
    }
    if let Some(take_profit_percent) = req.take_profit_percent {
        settings.take_profit_percent = take_profit_percent;
    }
    if let Some(stop_loss_enabled) = req.stop_loss_enabled {
        settings.stop_loss_enabled = stop_loss_enabled;
    }
    if let Some(stop_loss_percent) = req.stop_loss_percent {
        settings.stop_loss_percent = stop_loss_percent;
    }
    if let Some(trailing_stop_enabled) = req.trailing_stop_enabled {
        settings.trailing_stop_enabled = trailing_stop_enabled;
    }
    if let Some(trailing_stop_percent) = req.trailing_stop_percent {
        settings.trailing_stop_percent = trailing_stop_percent;
    }
    if let Some(time_exit_enabled) = req.time_exit_enabled {
        settings.time_exit_enabled = time_exit_enabled;
    }
    if let Some(time_exit_hours) = req.time_exit_hours {
        settings.time_exit_hours = time_exit_hours;
    }
    if let Some(max_positions) = req.max_positions {
        settings.max_positions = max_positions;
    }
    if let Some(cooldown_minutes) = req.cooldown_minutes {
        settings.cooldown_minutes = cooldown_minutes;
    }
    if let Some(max_daily_loss) = req.max_daily_loss {
        settings.max_daily_loss = Decimal::from_str(&max_daily_loss).unwrap_or(settings.max_daily_loss);
    }

    // Save updated settings
    state
        .db
        .update_auto_trading_settings(&settings)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(SettingsResponse {
        settings: settings.into(),
    }))
}

/// Request to enable auto-trading with password
#[derive(Debug, Deserialize)]
pub struct EnableAutoTradingRequest {
    pub password: String,
}

/// Enable auto-trading with password (stores decrypted key in memory for auto-signing)
pub async fn enable(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(req): Json<EnableAutoTradingRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Get encrypted key for this wallet
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
                    error: "No encrypted key found. Auto-trading requires a generated or imported wallet.".to_string(),
                }),
            )
        })?;

    // Decrypt the key with the provided password
    let private_key = crate::wallet::decrypt_private_key(&encrypted_key, &req.password)
        .map_err(|_| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid password".to_string(),
                }),
            )
        })?;

    // Store the decrypted key in memory for auto-trading
    state.key_store.store_key(&session.wallet_address, private_key).await;

    // Enable auto-trading in database
    state
        .db
        .enable_auto_trading(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(SuccessResponse {
        success: true,
        message: Some("Auto-trading enabled. Your wallet key is stored in memory for automated trading.".to_string()),
    }))
}

/// Disable auto-trading
pub async fn disable(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<SuccessResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    // Remove the decrypted key from memory
    state.key_store.remove_key(&session.wallet_address).await;

    // Disable auto-trading in database
    state
        .db
        .disable_auto_trading(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(SuccessResponse {
        success: true,
        message: Some("Auto-trading disabled. Your wallet key has been removed from memory.".to_string()),
    }))
}

/// Get auto-trading history
pub async fn get_history(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    let history = state
        .db
        .get_auto_trade_history(&session.wallet_address, limit, offset)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    let total = history.len();
    let history_dto: Vec<AutoTradeLogDto> = history.into_iter().map(|log| log.into()).collect();

    Ok(Json(HistoryResponse {
        history: history_dto,
        total,
    }))
}

/// Get auto-trading stats
pub async fn get_stats(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<ErrorResponse>)> {
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
        .get_auto_trading_stats(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    Ok(Json(StatsResponse {
        stats: stats.into(),
    }))
}

/// Status response with current state
#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub enabled: bool,
    pub auto_buy_enabled: bool,
    pub open_positions: i32,
    pub total_exposure: String,
    pub daily_pnl: String,
}

/// Get current auto-trading status
pub async fn get_status(
    State(state): State<AppState>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<Json<StatusResponse>, (StatusCode, Json<ErrorResponse>)> {
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

    let settings = state
        .db
        .get_auto_trading_settings(&session.wallet_address)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Database error: {}", e),
                }),
            )
        })?;

    let open_positions = state
        .db
        .count_open_positions(&session.wallet_address)
        .await
        .unwrap_or(0);

    let total_exposure = state
        .db
        .get_total_exposure(&session.wallet_address)
        .await
        .unwrap_or_default();

    let daily_pnl = state
        .db
        .get_daily_auto_pnl(&session.wallet_address)
        .await
        .unwrap_or_default();

    Ok(Json(StatusResponse {
        enabled: settings.enabled,
        auto_buy_enabled: settings.auto_buy_enabled,
        open_positions,
        total_exposure: total_exposure.to_string(),
        daily_pnl: daily_pnl.to_string(),
    }))
}
