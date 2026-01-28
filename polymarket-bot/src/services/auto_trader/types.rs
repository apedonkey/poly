//! Types for the auto-trading system

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Exit trigger types for auto-selling
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ExitTrigger {
    TakeProfit {
        price: Decimal,
        pnl_percent: Decimal,
    },
    StopLoss {
        price: Decimal,
        pnl_percent: Decimal,
    },
    TrailingStop {
        peak: Decimal,
        price: Decimal,
        drop_percent: Decimal,
    },
    TimeExit {
        hours_held: f64,
        price: Decimal,
    },
}

impl ExitTrigger {
    /// Get the action name for logging
    pub fn action_name(&self) -> String {
        match self {
            ExitTrigger::TakeProfit { .. } => "take_profit".to_string(),
            ExitTrigger::StopLoss { .. } => "stop_loss".to_string(),
            ExitTrigger::TrailingStop { .. } => "trailing_stop".to_string(),
            ExitTrigger::TimeExit { .. } => "time_exit".to_string(),
        }
    }

    /// Get the reason string for logging
    pub fn reason(&self) -> String {
        match self {
            ExitTrigger::TakeProfit { pnl_percent, .. } => {
                format!("Take profit triggered at +{:.1}%", pnl_percent * Decimal::from(100))
            }
            ExitTrigger::StopLoss { pnl_percent, .. } => {
                format!("Stop loss triggered at {:.1}%", pnl_percent * Decimal::from(100))
            }
            ExitTrigger::TrailingStop { drop_percent, peak, .. } => {
                format!(
                    "Trailing stop: {:.1}% drop from peak {}",
                    drop_percent * Decimal::from(100),
                    peak
                )
            }
            ExitTrigger::TimeExit { hours_held, .. } => {
                format!("Time exit after {:.1} hours", hours_held)
            }
        }
    }

    /// Get the exit price
    pub fn price(&self) -> Decimal {
        match self {
            ExitTrigger::TakeProfit { price, .. } => *price,
            ExitTrigger::StopLoss { price, .. } => *price,
            ExitTrigger::TrailingStop { price, .. } => *price,
            ExitTrigger::TimeExit { price, .. } => *price,
        }
    }
}

/// Auto-trade log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTradeLog {
    pub id: Option<i64>,
    pub wallet_address: String,
    pub position_id: Option<i64>,
    pub action: String,
    pub market_question: Option<String>,
    pub side: Option<String>,
    pub entry_price: Option<Decimal>,
    pub exit_price: Option<Decimal>,
    pub size: Option<Decimal>,
    pub pnl: Option<Decimal>,
    pub trigger_reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Auto-trading statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTradingStats {
    pub total_trades: i64,
    pub win_count: i64,
    pub loss_count: i64,
    pub win_rate: f64,
    pub total_pnl: Decimal,
    pub take_profit_count: i64,
    pub take_profit_pnl: Decimal,
    pub stop_loss_count: i64,
    pub stop_loss_pnl: Decimal,
    pub trailing_stop_count: i64,
    pub trailing_stop_pnl: Decimal,
    pub time_exit_count: i64,
    pub time_exit_pnl: Decimal,
    pub auto_buy_count: i64,
    pub best_trade_pnl: Decimal,
    pub worst_trade_pnl: Decimal,
    pub avg_hold_hours: f64,
}

impl Default for AutoTradingStats {
    fn default() -> Self {
        Self {
            total_trades: 0,
            win_count: 0,
            loss_count: 0,
            win_rate: 0.0,
            total_pnl: Decimal::ZERO,
            take_profit_count: 0,
            take_profit_pnl: Decimal::ZERO,
            stop_loss_count: 0,
            stop_loss_pnl: Decimal::ZERO,
            trailing_stop_count: 0,
            trailing_stop_pnl: Decimal::ZERO,
            time_exit_count: 0,
            time_exit_pnl: Decimal::ZERO,
            auto_buy_count: 0,
            best_trade_pnl: Decimal::ZERO,
            worst_trade_pnl: Decimal::ZERO,
            avg_hold_hours: 0.0,
        }
    }
}

/// Position peak price for trailing stop
#[derive(Debug, Clone)]
pub struct PositionPeak {
    pub position_id: i64,
    pub peak_price: Decimal,
    pub peak_at: DateTime<Utc>,
}
