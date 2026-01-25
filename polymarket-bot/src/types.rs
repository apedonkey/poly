//! Core types for the Polymarket trading bot

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents a market tracked by the bot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedMarket {
    pub id: String,
    pub condition_id: String,
    pub question: String,
    pub slug: String,
    pub resolution_source: Option<String>,
    pub end_date: Option<DateTime<Utc>>,
    pub yes_price: Decimal,
    pub no_price: Decimal,
    pub volume: Decimal,
    pub liquidity: Decimal,
    pub category: Option<String>,
    pub active: bool,
    pub closed: bool,
    pub yes_token_id: Option<String>,
    pub no_token_id: Option<String>,
    pub hours_until_close: Option<f64>,
}

impl TrackedMarket {
    /// Get the favorite side and its price
    pub fn favorite(&self) -> (Side, Decimal) {
        if self.yes_price > self.no_price {
            (Side::Yes, self.yes_price)
        } else {
            (Side::No, self.no_price)
        }
    }

    /// Check if this is a fast-resolving market based on category/source
    pub fn is_fast_resolution(&self) -> bool {
        let fast_categories = ["Sports", "Crypto", "NBA", "NFL", "MLB", "Soccer"];
        let fast_sources = ["chainlink", "espn", "official", "ap news", "reuters"];

        if let Some(cat) = &self.category {
            if fast_categories.iter().any(|c| cat.to_lowercase().contains(&c.to_lowercase())) {
                return true;
            }
        }

        if let Some(src) = &self.resolution_source {
            let src_lower = src.to_lowercase();
            if fast_sources.iter().any(|s| src_lower.contains(s)) {
                return true;
            }
        }

        false
    }
}

/// Trading side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Yes,
    No,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::Yes => write!(f, "YES"),
            Side::No => write!(f, "NO"),
        }
    }
}

/// Strategy type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrategyType {
    ResolutionSniper,
    NoBias,
}

impl fmt::Display for StrategyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrategyType::ResolutionSniper => write!(f, "Sniper"),
            StrategyType::NoBias => write!(f, "NO Bias"),
        }
    }
}

/// A trading opportunity identified by a strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub slug: String,
    pub strategy: StrategyType,
    pub side: Side,
    pub entry_price: Decimal,
    pub expected_return: f64,
    pub confidence: f64,
    pub edge: f64,
    pub time_to_close_hours: Option<f64>,
    pub liquidity: Decimal,
    pub volume: Decimal,
    pub category: Option<String>,
    pub resolution_source: Option<String>,
    pub recommendation: String,
    /// Token ID for CLOB trading (YES or NO token depending on side)
    pub token_id: Option<String>,
}

impl Opportunity {
    /// Calculate expected value in percentage
    pub fn ev_percent(&self) -> f64 {
        self.edge * 100.0
    }

    /// Calculate potential return in percentage
    pub fn return_percent(&self) -> f64 {
        self.expected_return * 100.0
    }

    /// Get price in cents for display
    pub fn price_cents(&self) -> i32 {
        let price_f64: f64 = self.entry_price.try_into().unwrap_or(0.0);
        (price_f64 * 100.0).round() as i32
    }

    /// Get Polymarket URL for this opportunity
    pub fn url(&self) -> String {
        // Use condition_id for reliable market URLs
        if !self.condition_id.is_empty() {
            format!("https://polymarket.com/event/{}", self.condition_id)
        } else if !self.slug.is_empty() {
            format!("https://polymarket.com/event/{}", self.slug)
        } else {
            format!("https://polymarket.com/markets")
        }
    }

    /// Format time to close for display
    pub fn time_display(&self) -> String {
        match self.time_to_close_hours {
            Some(h) if h < 1.0 => format!("{:.0}m", h * 60.0),
            Some(h) if h < 24.0 => format!("{:.1}h", h),
            Some(h) if h < 24.0 * 7.0 => format!("{:.1}d", h / 24.0),
            Some(h) => format!("{:.1}w", h / (24.0 * 7.0)),
            None => "?".to_string(),
        }
    }

    /// Get a shortened question for display (handles UTF-8 properly)
    pub fn short_question(&self, max_len: usize) -> String {
        let chars: Vec<char> = self.question.chars().collect();
        if chars.len() <= max_len {
            self.question.clone()
        } else {
            let truncated: String = chars[..max_len.saturating_sub(3)].iter().collect();
            format!("{}...", truncated)
        }
    }
}

/// Order status for tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Filled,
    PartiallyFilled,
    Cancelled,
    Rejected,
}

/// A tracked position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: i64,
    pub market_id: String,
    pub question: String,
    pub side: Side,
    pub entry_price: Decimal,
    pub size: Decimal,
    pub strategy: StrategyType,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub exit_price: Option<Decimal>,
    pub pnl: Option<Decimal>,
    pub status: PositionStatus,
    pub is_paper: bool,
    /// When the market ends/closes
    pub end_date: Option<DateTime<Utc>>,
}

impl Position {
    /// Get hours remaining until market closes
    pub fn hours_until_close(&self) -> Option<f64> {
        self.end_date.map(|end| {
            let now = Utc::now();
            let duration = end.signed_duration_since(now);
            duration.num_minutes() as f64 / 60.0
        })
    }

    /// Format time remaining for display
    pub fn time_remaining_display(&self) -> String {
        match self.hours_until_close() {
            Some(h) if h <= 0.0 => "Ended".to_string(),
            Some(h) if h < 1.0 => format!("{:.0}m", h * 60.0),
            Some(h) if h < 24.0 => format!("{:.1}h", h),
            Some(h) if h < 24.0 * 7.0 => format!("{:.1}d", h / 24.0),
            Some(h) => format!("{:.1}w", h / (24.0 * 7.0)),
            None => "Unknown".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionStatus {
    Open,
    PendingResolution,
    Resolved,
    Closed,
}

/// Statistics for bot performance
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BotStats {
    pub total_trades: i64,
    pub winning_trades: i64,
    pub losing_trades: i64,
    pub total_pnl: Decimal,
    pub sniper_trades: i64,
    pub sniper_wins: i64,
    pub no_bias_trades: i64,
    pub no_bias_wins: i64,
    pub avg_hold_time_hours: f64,
}

impl BotStats {
    pub fn win_rate(&self) -> f64 {
        if self.total_trades == 0 {
            0.0
        } else {
            (self.winning_trades as f64 / self.total_trades as f64) * 100.0
        }
    }

    pub fn sniper_win_rate(&self) -> f64 {
        if self.sniper_trades == 0 {
            0.0
        } else {
            (self.sniper_wins as f64 / self.sniper_trades as f64) * 100.0
        }
    }

    pub fn no_bias_win_rate(&self) -> f64 {
        if self.no_bias_trades == 0 {
            0.0
        } else {
            (self.no_bias_wins as f64 / self.no_bias_trades as f64) * 100.0
        }
    }
}
