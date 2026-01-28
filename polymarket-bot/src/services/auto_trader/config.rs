//! Auto-trading configuration and settings

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Auto-trading settings for a wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTradingSettings {
    pub wallet_address: String,

    /// Master switch - enables/disables all auto-trading
    pub enabled: bool,

    // === Auto-Buy Settings ===
    /// Enable automatic buying of opportunities
    pub auto_buy_enabled: bool,
    /// Maximum USDC per trade
    pub max_position_size: Decimal,
    /// Maximum total USDC exposure across all positions
    pub max_total_exposure: Decimal,
    /// Minimum edge required to buy (e.g., 0.05 = 5%)
    pub min_edge: f64,
    /// Which strategies to auto-buy: ["sniper", "no_bias"]
    pub strategies: Vec<String>,

    // === Take Profit ===
    /// Enable take-profit auto-sell
    pub take_profit_enabled: bool,
    /// Take profit percentage (e.g., 0.20 = 20%)
    pub take_profit_percent: f64,

    // === Stop Loss ===
    /// Enable stop-loss auto-sell
    pub stop_loss_enabled: bool,
    /// Stop loss percentage (e.g., 0.10 = 10%)
    pub stop_loss_percent: f64,

    // === Trailing Stop ===
    /// Enable trailing stop
    pub trailing_stop_enabled: bool,
    /// Trailing stop percentage below peak (e.g., 0.10 = 10%)
    pub trailing_stop_percent: f64,

    // === Time Exit ===
    /// Enable time-based exit
    pub time_exit_enabled: bool,
    /// Hours to hold before time exit
    pub time_exit_hours: f64,

    // === Risk Management ===
    /// Maximum concurrent positions
    pub max_positions: i32,
    /// Cooldown minutes between buys in same market
    pub cooldown_minutes: i32,
    /// Maximum daily loss before pausing auto-trading
    pub max_daily_loss: Decimal,
}

impl Default for AutoTradingSettings {
    fn default() -> Self {
        Self {
            wallet_address: String::new(),
            enabled: false,

            // Auto-buy OFF by default (user must opt-in)
            auto_buy_enabled: false,
            max_position_size: Decimal::from(50),
            max_total_exposure: Decimal::from(500),
            min_edge: 0.05,
            strategies: vec!["sniper".to_string()],

            // Take profit ON by default
            take_profit_enabled: true,
            take_profit_percent: 0.20,

            // Stop loss ON by default
            stop_loss_enabled: true,
            stop_loss_percent: 0.10,

            // Trailing stop OFF by default
            trailing_stop_enabled: false,
            trailing_stop_percent: 0.10,

            // Time exit OFF by default
            time_exit_enabled: false,
            time_exit_hours: 24.0,

            // Risk management
            max_positions: 10,
            cooldown_minutes: 5,
            max_daily_loss: Decimal::from(100),
        }
    }
}

impl AutoTradingSettings {
    /// Create settings with a specific wallet address
    pub fn for_wallet(wallet_address: &str) -> Self {
        Self {
            wallet_address: wallet_address.to_string(),
            ..Default::default()
        }
    }

    /// Check if any auto-sell feature is enabled
    pub fn has_auto_sell_enabled(&self) -> bool {
        self.take_profit_enabled
            || self.stop_loss_enabled
            || self.trailing_stop_enabled
            || self.time_exit_enabled
    }
}

/// Request to update auto-trading settings (partial update)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettingsRequest {
    pub enabled: Option<bool>,
    pub auto_buy_enabled: Option<bool>,
    pub max_position_size: Option<String>,
    pub max_total_exposure: Option<String>,
    pub min_edge: Option<f64>,
    pub strategies: Option<Vec<String>>,
    pub take_profit_enabled: Option<bool>,
    pub take_profit_percent: Option<f64>,
    pub stop_loss_enabled: Option<bool>,
    pub stop_loss_percent: Option<f64>,
    pub trailing_stop_enabled: Option<bool>,
    pub trailing_stop_percent: Option<f64>,
    pub time_exit_enabled: Option<bool>,
    pub time_exit_hours: Option<f64>,
    pub max_positions: Option<i32>,
    pub cooldown_minutes: Option<i32>,
    pub max_daily_loss: Option<String>,
}
