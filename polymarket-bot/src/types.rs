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
    /// Full market description containing resolution rules
    pub description: Option<String>,
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
    /// Whether this is a neg-risk market (uses different exchange contract)
    pub neg_risk: bool,
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
    Dispute,
    MillionairesClub,
    MintMaker,
}

impl fmt::Display for StrategyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrategyType::ResolutionSniper => write!(f, "Sniper"),
            StrategyType::Dispute => write!(f, "Dispute"),
            StrategyType::MillionairesClub => write!(f, "MC"),
            StrategyType::MintMaker => write!(f, "MintMaker"),
        }
    }
}

// ==================== MINT MAKER TYPES ====================

/// Crypto assets supported by Mint Maker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum CryptoAsset {
    BTC,
    ETH,
    SOL,
    XRP,
}

impl fmt::Display for CryptoAsset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CryptoAsset::BTC => write!(f, "BTC"),
            CryptoAsset::ETH => write!(f, "ETH"),
            CryptoAsset::SOL => write!(f, "SOL"),
            CryptoAsset::XRP => write!(f, "XRP"),
        }
    }
}

impl CryptoAsset {
    /// Keywords to match this asset in market questions
    pub fn keywords(&self) -> &[&str] {
        match self {
            CryptoAsset::BTC => &["btc", "bitcoin"],
            CryptoAsset::ETH => &["eth", "ethereum"],
            CryptoAsset::SOL => &["sol", "solana"],
            CryptoAsset::XRP => &["xrp", "ripple"],
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "BTC" => Some(CryptoAsset::BTC),
            "ETH" => Some(CryptoAsset::ETH),
            "SOL" => Some(CryptoAsset::SOL),
            "XRP" => Some(CryptoAsset::XRP),
            _ => None,
        }
    }

    /// All supported assets
    pub fn all() -> &'static [CryptoAsset] {
        &[CryptoAsset::BTC, CryptoAsset::ETH, CryptoAsset::SOL, CryptoAsset::XRP]
    }

    /// Lowercase slug for event URL construction
    pub fn slug(&self) -> &'static str {
        match self {
            CryptoAsset::BTC => "btc",
            CryptoAsset::ETH => "eth",
            CryptoAsset::SOL => "sol",
            CryptoAsset::XRP => "xrp",
        }
    }
}

/// A 15-min crypto Up/Down market eligible for mint making
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintMakerMarket {
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub slug: String,
    pub asset: CryptoAsset,
    pub yes_token_id: String,
    pub no_token_id: String,
    pub yes_price: Decimal,
    pub no_price: Decimal,
    pub minutes_to_close: f64,
    pub neg_risk: bool,
}

/// Status of a mint maker order pair
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MintMakerPairStatus {
    /// Both orders placed, waiting for fills
    Pending,
    /// One side filled, other still open
    HalfFilled,
    /// Both sides filled, ready to merge
    Matched,
    /// Merge in progress
    Merging,
    /// Successfully merged back to USDC
    Merged,
    /// Cancelled (stale, manual, or error)
    Cancelled,
}

impl fmt::Display for MintMakerPairStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MintMakerPairStatus::Pending => write!(f, "Pending"),
            MintMakerPairStatus::HalfFilled => write!(f, "HalfFilled"),
            MintMakerPairStatus::Matched => write!(f, "Matched"),
            MintMakerPairStatus::Merging => write!(f, "Merging"),
            MintMakerPairStatus::Merged => write!(f, "Merged"),
            MintMakerPairStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// A tracked order pair for mint making
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintMakerPair {
    pub id: i64,
    pub wallet_address: String,
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub asset: String,
    pub yes_order_id: String,
    pub no_order_id: String,
    pub yes_bid_price: Decimal,
    pub no_bid_price: Decimal,
    pub yes_fill_price: Option<Decimal>,
    pub no_fill_price: Option<Decimal>,
    pub pair_cost: Option<Decimal>,
    pub profit: Option<Decimal>,
    pub size: Decimal,
    pub status: MintMakerPairStatus,
    pub merge_tx_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Aggregate stats for mint maker
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MintMakerStats {
    pub total_pairs: i64,
    pub merged_pairs: i64,
    pub cancelled_pairs: i64,
    pub total_profit: Decimal,
    pub total_cost: Decimal,
    pub avg_spread: f64,
    pub fill_rate: f64,
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
    /// Full market description containing resolution rules
    pub description: Option<String>,
    pub recommendation: String,
    /// Token ID for CLOB trading (YES or NO token depending on side)
    pub token_id: Option<String>,
    /// Whether this is a neg-risk market
    #[serde(default)]
    pub neg_risk: bool,
    /// Whether the opportunity currently meets all filter criteria.
    /// Updated in real-time as prices change. False = temporarily outside thresholds.
    #[serde(default = "default_meets_criteria")]
    pub meets_criteria: bool,
    /// Top holders on each side of the market
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holders: Option<MarketHolders>,
}

fn default_meets_criteria() -> bool {
    true
}

/// A market holder (top holder on YES or NO side)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketHolder {
    pub address: String,
    pub amount: f64,
    pub name: String,
    pub outcome_index: u8,
}

/// Top holders for both sides of a market
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketHolders {
    pub yes_holders: Vec<MarketHolder>,
    pub no_holders: Vec<MarketHolder>,
    pub yes_total_count: usize,
    pub no_total_count: usize,
}

impl Opportunity {
    /// Check if this opportunity belongs in the Sniper section
    /// (ResolutionSniper + NOT crypto + NOT sports + closing within 12h)
    pub fn is_sniper_section(&self) -> bool {
        self.strategy == StrategyType::ResolutionSniper
            && !self.is_crypto()
            && !self.is_sports()
            && self.time_to_close_hours.map(|h| h <= 12.0).unwrap_or(false)
    }

    /// Helper for word boundary matching without regex
    fn contains_word(text: &str, word: &str) -> bool {
        let text = text.to_lowercase();
        let word = word.to_lowercase();

        for (i, _) in text.match_indices(&word) {
            let before_ok = i == 0 || !text.chars().nth(i - 1).unwrap_or(' ').is_alphanumeric();
            let after_ok = i + word.len() >= text.len()
                || !text.chars().nth(i + word.len()).unwrap_or(' ').is_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
        }
        false
    }

    /// Check if opportunity is crypto-related
    pub fn is_crypto(&self) -> bool {
        let q = &self.question;
        let cat = self.category.as_deref().unwrap_or("").to_lowercase();

        if cat == "crypto" || cat == "cryptocurrency" {
            return true;
        }

        // Match crypto keywords with word boundaries
        let crypto_keywords = [
            "bitcoin", "btc", "ethereum", "eth", "solana", "sol", "xrp", "ripple",
            "dogecoin", "doge", "cardano", "ada", "polkadot", "dot", "avalanche",
            "avax", "chainlink", "link", "polygon", "matic", "litecoin", "ltc",
            "crypto", "cryptocurrency",
        ];

        crypto_keywords.iter().any(|kw| Self::contains_word(q, kw))
    }

    /// Check if opportunity is sports-related
    pub fn is_sports(&self) -> bool {
        let q = self.question.to_lowercase();
        let cat = self.category.as_deref().unwrap_or("").to_lowercase();

        if cat == "sports" {
            return true;
        }

        // Sports keywords (simple contains is fine for multi-word phrases)
        let sports_keywords = [
            "spread:", "moneyline", "over/under", "fight", "fighter", "knockout",
            "submission", "rounds", "decision", "unanimous",
            "nba", "nfl", "mlb", "nhl", "mls", "ufc", "bellator", "pga", "atp", "wta",
            "premier league", "la liga", "serie a", "bundesliga", "ligue 1",
            "champions league", "europa league", "super bowl", "world series",
            "stanley cup", "world cup",
        ];

        if sports_keywords.iter().any(|kw| q.contains(kw)) {
            return true;
        }

        // Check for "vs" pattern (Team vs Team)
        if q.contains(" vs ") || q.contains(" vs.") {
            return true;
        }

        // Check for KO/TKO with word boundaries
        if Self::contains_word(&q, "ko") || Self::contains_word(&q, "tko") {
            return true;
        }

        // Check for O/U pattern
        if Self::contains_word(&q, "o/u") {
            return true;
        }

        false
    }
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

    /// Recalculate opportunity metrics after a price change.
    /// Updates `meets_criteria` field - returns the new value.
    /// Opportunity is kept in list either way so it can reactivate if price moves back.
    pub fn recalculate_with_price(&mut self, new_price: Decimal) -> bool {
        let price_f64: f64 = new_price.try_into().unwrap_or(0.0);

        // Avoid division by zero
        if price_f64 <= 0.0 || price_f64 >= 1.0 {
            self.meets_criteria = false;
            return false;
        }

        self.entry_price = new_price;

        self.meets_criteria = match self.strategy {
            StrategyType::ResolutionSniper => {
                self.recalculate_sniper(price_f64)
            }
            StrategyType::Dispute | StrategyType::MillionairesClub | StrategyType::MintMaker => {
                // Disputes, MC, and MintMaker don't use sniper price filtering
                true
            }
        };

        self.meets_criteria
    }

    /// Recalculate sniper opportunity metrics
    fn recalculate_sniper(&mut self, price: f64) -> bool {
        // Sniper config defaults
        const MIN_FAVORITE_PRICE: f64 = 0.70;
        const MAX_FAVORITE_PRICE: f64 = 0.90;
        const MIN_EV: f64 = 0.05;

        // Check price range filter
        if price < MIN_FAVORITE_PRICE || price > MAX_FAVORITE_PRICE {
            return false;
        }

        // Get accuracy based on hours (uses same interpolation as strategy)
        let hours = self.time_to_close_hours.unwrap_or(12.0);
        let accuracy = Self::accuracy_at_hours(hours);

        // Recalculate expected return: (1 - price) / price
        self.expected_return = (1.0 - price) / price;

        // Recalculate EV: (win_prob × profit) - (lose_prob × loss)
        self.edge = (accuracy * (1.0 - price)) - ((1.0 - accuracy) * price);

        // Check EV threshold
        if self.edge < MIN_EV {
            return false;
        }

        // Update confidence (may have changed if hours changed, but typically static during session)
        self.confidence = accuracy;

        // Update recommendation string
        let no_bias_bonus = matches!(self.side, Side::No);
        self.recommendation = format!(
            "BUY {} at {:.0}c | {:.1}% return | {:.1}% EV | {:.1}h left{}",
            self.side,
            price * 100.0,
            self.expected_return * 100.0,
            self.edge * 100.0,
            hours,
            if no_bias_bonus { " [NO BIAS+]" } else { "" }
        );

        true
    }

    /// Get historical accuracy at given hours before close (same as sniper strategy)
    fn accuracy_at_hours(hours: f64) -> f64 {
        if hours <= 4.0 {
            0.953
        } else if hours <= 12.0 {
            // Linear interpolation between 4h (95.3%) and 12h (90.6%)
            let t = (hours - 4.0) / 8.0;
            0.953 - (t * (0.953 - 0.906))
        } else if hours <= 24.0 {
            // Linear interpolation between 12h (90.6%) and 24h (89.4%)
            let t = (hours - 12.0) / 12.0;
            0.906 - (t * (0.906 - 0.894))
        } else {
            0.893 // Baseline for > 24 hours
        }
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

/// Order status for tracking (legacy)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Filled,
    PartiallyFilled,
    Cancelled,
    Rejected,
}

/// Order lifecycle status matching Polymarket CLOB states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderLifecycleStatus {
    /// Order submitted but not yet confirmed
    Pending,
    /// Order is live on the orderbook
    Live,
    /// Order matched (waiting for on-chain confirmation)
    Matched,
    /// Order transaction submitted to blockchain
    Mined,
    /// Order fully confirmed on-chain
    Confirmed,
    /// Order failed to execute
    Failed,
    /// Order cancelled by user
    Cancelled,
}

impl fmt::Display for OrderLifecycleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderLifecycleStatus::Pending => write!(f, "Pending"),
            OrderLifecycleStatus::Live => write!(f, "Live"),
            OrderLifecycleStatus::Matched => write!(f, "Matched"),
            OrderLifecycleStatus::Mined => write!(f, "Mined"),
            OrderLifecycleStatus::Confirmed => write!(f, "Confirmed"),
            OrderLifecycleStatus::Failed => write!(f, "Failed"),
            OrderLifecycleStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// A tracked order in the CLOB system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    /// Order ID from CLOB
    pub id: String,
    /// Wallet address that placed this order
    pub wallet_address: String,
    /// Token ID being traded
    pub token_id: String,
    /// Market ID (for linking to positions)
    pub market_id: Option<String>,
    /// Buy or Sell
    pub side: Side,
    /// Order type (FOK, GTC, GTD)
    pub order_type: String,
    /// Order price
    pub price: Decimal,
    /// Original size in USDC
    pub original_size: Decimal,
    /// Amount filled so far
    pub filled_size: Decimal,
    /// Average fill price (from actual fills)
    pub avg_fill_price: Option<Decimal>,
    /// Current lifecycle status
    pub status: OrderLifecycleStatus,
    /// Associated position ID if any
    pub position_id: Option<i64>,
    /// Whether this is a neg-risk market order
    pub neg_risk: bool,
    /// When the order was created
    pub created_at: DateTime<Utc>,
    /// Last status update
    pub updated_at: DateTime<Utc>,
}

/// A tracked position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: i64,
    /// Wallet address that owns this position
    pub wallet_address: String,
    pub market_id: String,
    pub question: String,
    pub slug: Option<String>,
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
    /// Token ID for CLOB trading (needed to sell the position)
    pub token_id: Option<String>,
    /// Order ID from CLOB (for querying fill price)
    pub order_id: Option<String>,
    /// Shares remaining (for partial sells). None means full position (backward compat)
    pub remaining_size: Option<Decimal>,
    /// Cumulative PnL from partial sells
    pub realized_pnl: Option<Decimal>,
    /// Total shares sold so far
    pub total_sold_size: Option<Decimal>,
    /// Weighted average exit price from partial sells
    pub avg_exit_price: Option<Decimal>,
    /// Whether this is a neg-risk market
    pub neg_risk: bool,
    /// Fee paid on this position (taker fee)
    pub fee_paid: Option<Decimal>,
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
}

// ==================== UMA DISPUTE TRACKER TYPES ====================

/// Status of a UMA dispute
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeStatus {
    /// Initial proposal, 2hr challenge window
    Proposed,
    /// First dispute, auto-reset
    Disputed,
    /// Escalated to UMA DVM voting
    DvmVote,
}

impl fmt::Display for DisputeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DisputeStatus::Proposed => write!(f, "Proposed"),
            DisputeStatus::Disputed => write!(f, "Disputed"),
            DisputeStatus::DvmVote => write!(f, "DVM Vote"),
        }
    }
}

/// Alert for an active UMA dispute on a Polymarket market
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisputeAlert {
    /// UMA assertion ID
    pub assertion_id: String,
    pub condition_id: String,
    pub question: String,
    pub slug: String,
    pub dispute_status: DisputeStatus,
    /// "Yes" or "No"
    pub proposed_outcome: String,
    /// When the dispute started
    pub dispute_timestamp: i64,
    /// Estimated when challenge window / DVM vote ends (from actual expirationTime)
    pub estimated_resolution: i64,
    pub current_yes_price: Decimal,
    pub current_no_price: Decimal,
    pub liquidity: Decimal,
    /// Token ID for YES outcome (for trading)
    pub yes_token_id: Option<String>,
    /// Token ID for NO outcome (for trading)
    pub no_token_id: Option<String>,
    /// Edge if proposed outcome wins (e.g., 0.15 = 15% profit)
    pub edge: Option<Decimal>,
    /// Which dispute round this is (1 = first proposal, 2 = re-proposal after first dispute)
    /// UmaCtfAdapter uses a two-round mechanism: first dispute resets, second dispute goes to DVM
    #[serde(default = "default_dispute_round")]
    pub dispute_round: u8,
    /// Proposer bond amount in USDC (typically $750)
    #[serde(default)]
    pub proposer_bond: Option<Decimal>,
    /// Which UmaCtfAdapter version this assertion belongs to (e.g., "v1", "v2", "v3")
    #[serde(default)]
    pub adapter_version: Option<String>,
    /// Liveness period in seconds (challenge window, typically 7200 = 2 hours)
    #[serde(default)]
    pub liveness_seconds: Option<i64>,
    /// Expected value considering 50-50 outcome possibility
    #[serde(default)]
    pub expected_value: Option<Decimal>,
}

fn default_dispute_round() -> u8 {
    1
}
