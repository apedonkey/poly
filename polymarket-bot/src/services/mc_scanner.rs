//! Millionaires Club observation scanner
//!
//! Evaluates markets priced 93–97c for resolution certainty,
//! checks orderbook depth, simulates trades, and tracks tiered bankroll
//! progression. Observation mode only — no real money deployed.

use crate::db::Database;
use crate::types::{DisputeAlert, TrackedMarket, Side};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn, debug};

// ==================== TYPES ====================

/// Operating mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum McMode {
    Observation,
    Live,
}

impl std::fmt::Display for McMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McMode::Observation => write!(f, "observation"),
            McMode::Live => write!(f, "live"),
        }
    }
}

/// Pause state for risk management
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McPauseState {
    Active,
    DrawdownReduced,
    DrawdownPaused,
    DisputePause,
    WeeklyLossPause,
}

impl std::fmt::Display for McPauseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McPauseState::Active => write!(f, "active"),
            McPauseState::DrawdownReduced => write!(f, "drawdown_reduced"),
            McPauseState::DrawdownPaused => write!(f, "drawdown_paused"),
            McPauseState::DisputePause => write!(f, "dispute_pause"),
            McPauseState::WeeklyLossPause => write!(f, "weekly_loss_pause"),
        }
    }
}

/// Result of evaluating a single market
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McScoutResult {
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub slug: String,
    pub side: String,
    pub price: String,
    pub volume: String,
    pub category: Option<String>,
    pub end_date: Option<String>,
    pub passed: bool,
    pub certainty_score: i32,
    pub reasons: Vec<String>,
    pub slippage_pct: Option<f64>,
    pub would_trade: bool,
    pub token_id: Option<String>,
    pub scanned_at: String,
}

/// Full status update for frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McStatusUpdate {
    pub mode: String,
    pub tier: i32,
    pub bankroll: String,
    pub bet_size: String,
    pub win_rate: f64,
    pub total_pnl: String,
    pub total_trades: i64,
    pub open_trades: i64,
    pub drawdown_pct: f64,
    pub peak_bankroll: String,
    pub pause_state: String,
    pub pause_until: Option<String>,
    pub recent_scouts: Vec<McScoutResult>,
    pub max_positions: i32,
}

/// Tier definition
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct TierDef {
    tier: i32,
    bankroll: f64,
    bet_size: f64,
    promote_at: f64,   // 120% of bankroll
    demote_at: f64,    // 80% of bankroll
    max_positions: i32,
}

/// Static tier definitions
fn get_tiers() -> Vec<TierDef> {
    vec![
        TierDef { tier: 1, bankroll: 40.0,    bet_size: 5.0,    promote_at: 48.0,    demote_at: 32.0,    max_positions: 6  },
        TierDef { tier: 2, bankroll: 100.0,   bet_size: 12.0,   promote_at: 120.0,   demote_at: 80.0,    max_positions: 7  },
        TierDef { tier: 3, bankroll: 300.0,   bet_size: 35.0,   promote_at: 360.0,   demote_at: 240.0,   max_positions: 8  },
        TierDef { tier: 4, bankroll: 1000.0,  bet_size: 100.0,  promote_at: 1200.0,  demote_at: 800.0,   max_positions: 9  },
        TierDef { tier: 5, bankroll: 3000.0,  bet_size: 250.0,  promote_at: 3600.0,  demote_at: 2400.0,  max_positions: 10 },
        TierDef { tier: 6, bankroll: 7000.0,  bet_size: 500.0,  promote_at: 8400.0,  demote_at: 5600.0,  max_positions: 11 },
        TierDef { tier: 7, bankroll: 10000.0, bet_size: 750.0,  promote_at: 12000.0, demote_at: 8000.0,  max_positions: 12 },
    ]
}

fn get_tier_def(tier: i32) -> TierDef {
    let tiers = get_tiers();
    tiers.into_iter().find(|t| t.tier == tier).unwrap_or(TierDef {
        tier: 1, bankroll: 40.0, bet_size: 5.0, promote_at: 48.0, demote_at: 32.0, max_positions: 6,
    })
}

// ==================== ORDERBOOK TYPES ====================

#[derive(Debug, Deserialize)]
struct OrderbookResponse {
    #[serde(default)]
    asks: Vec<OrderbookLevel>,
}

#[derive(Debug, Deserialize)]
struct OrderbookLevel {
    price: String,
    size: String,
}

// ==================== MC SCANNER ====================

pub struct McScanner {
    db: Arc<Database>,
    client: reqwest::Client,
}

impl McScanner {
    pub async fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            client: reqwest::Client::new(),
        }
    }

    /// Main loop: receives raw markets, evaluates them, broadcasts status
    pub async fn run(
        &mut self,
        mut markets_rx: broadcast::Receiver<Vec<TrackedMarket>>,
        disputes: Arc<RwLock<Vec<DisputeAlert>>>,
        mc_tx: broadcast::Sender<McStatusUpdate>,
    ) {
        info!("Starting Millionaires Club observation scanner...");

        // Ensure config exists
        if let Err(e) = self.db.mc_get_config().await {
            warn!("MC config init error (will create default): {}", e);
        }

        loop {
            match markets_rx.recv().await {
                Ok(markets) => {
                    let dispute_list = disputes.read().await.clone();
                    if let Err(e) = self.process_markets(&markets, &dispute_list, &mc_tx).await {
                        warn!("MC scanner cycle error: {}", e);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    debug!("MC scanner lagged by {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    info!("MC scanner channel closed, shutting down");
                    break;
                }
            }
        }
    }

    async fn process_markets(
        &self,
        markets: &[TrackedMarket],
        disputes: &[DisputeAlert],
        mc_tx: &broadcast::Sender<McStatusUpdate>,
    ) -> Result<()> {
        let config = self.db.mc_get_config().await?;
        let tier_def = get_tier_def(config.tier);

        // Check if paused
        if config.pause_state != "active" {
            if let Some(pause_until) = &config.pause_until {
                if let Ok(until) = DateTime::parse_from_rfc3339(pause_until) {
                    if Utc::now() > until.with_timezone(&Utc) {
                        // Unpause
                        self.db.mc_update_pause_state("active", None).await?;
                        info!("MC scanner pause expired, resuming");
                    } else {
                        // Still paused — broadcast status but skip evaluation
                        self.broadcast_status(mc_tx, &[], &config, &tier_def).await?;
                        return Ok(());
                    }
                }
            }
        }

        // Check simulated resolutions first
        self.check_simulated_resolutions().await?;

        // Update risk state (drawdown checks, tier promotion/demotion)
        self.update_risk_state().await?;

        // Re-read config after risk updates
        let config = self.db.mc_get_config().await?;
        let tier_def = get_tier_def(config.tier);

        let mut scout_results = Vec::new();
        let mut evaluated = 0;

        for market in markets {
            if !market.active || market.closed {
                continue;
            }

            let result = self.evaluate_market(market, disputes, &config, &tier_def).await;
            match result {
                Ok(Some(scout)) => {
                    // Log to database
                    let _ = self.db.mc_insert_scout_log(&scout).await;

                    // Simulate trade if it passed
                    if scout.would_trade {
                        if let Err(e) = self.simulate_trade(&scout, &config, &tier_def).await {
                            warn!("MC simulate trade error: {}", e);
                        }
                    }

                    scout_results.push(scout);
                    evaluated += 1;
                }
                Ok(None) => {
                    // Market didn't meet initial price filter, skip
                }
                Err(e) => {
                    debug!("MC evaluate error for {}: {}", market.question, e);
                }
            }
        }

        if evaluated > 0 {
            info!("MC scanner evaluated {} markets, {} passed initial filters", evaluated, scout_results.iter().filter(|s| s.passed).count());
        }

        // Broadcast status
        self.broadcast_status(mc_tx, &scout_results, &config, &tier_def).await?;

        // Cleanup old scout logs (keep last 7 days)
        let _ = self.db.mc_cleanup_old_scout_logs(7).await;

        Ok(())
    }

    /// Evaluate a single market for MC criteria
    async fn evaluate_market(
        &self,
        market: &TrackedMarket,
        disputes: &[DisputeAlert],
        _config: &McConfig,
        tier_def: &TierDef,
    ) -> Result<Option<McScoutResult>> {
        let (fav_side, fav_price) = market.favorite();
        let price_f64 = fav_price.to_f64().unwrap_or(0.0);

        // Price filter: 93c to 97c (above 98c is unprofitable after 2% taker fee)
        if price_f64 < 0.93 || price_f64 > 0.97 {
            return Ok(None);
        }

        let mut reasons = Vec::new();
        let mut passed = true;

        // Volume filter: > $5K
        let volume_f64 = market.volume.to_f64().unwrap_or(0.0);
        if volume_f64 < 5000.0 {
            reasons.push(format!("-FAIL volume ${:.0} < $5K", volume_f64));
            passed = false;
        }

        // Resolution within 12 hours
        let hours = market.hours_until_close.unwrap_or(f64::MAX);
        if hours > 12.0 {
            reasons.push(format!("-FAIL resolution {:.1}h > 12h", hours));
            passed = false;
        }
        if hours <= 0.0 {
            reasons.push("-FAIL market already ended".to_string());
            passed = false;
        }

        // Dispute check
        let has_dispute = disputes.iter().any(|d| d.condition_id == market.condition_id);
        if has_dispute {
            reasons.push("-FAIL active dispute".to_string());
            passed = false;
        }

        // Certainty score
        let (certainty_score, certainty_reasons) = self.resolution_certainty_score(market, disputes);
        reasons.extend(certainty_reasons);

        if certainty_score < 60 {
            reasons.push(format!("-FAIL certainty {} < 60", certainty_score));
            passed = false;
        }

        // Orderbook depth check (only if other checks pass to save API calls)
        let mut slippage_pct = None;
        let token_id = match fav_side {
            Side::Yes => market.yes_token_id.clone(),
            Side::No => market.no_token_id.clone(),
        };

        if passed {
            if let Some(ref tid) = token_id {
                match self.check_orderbook_depth(tid, tier_def.bet_size).await {
                    Ok((has_depth, _avg_fill, slippage)) => {
                        slippage_pct = Some(slippage);
                        if !has_depth {
                            reasons.push(format!("-FAIL insufficient depth for ${:.0} bet", tier_def.bet_size));
                            passed = false;
                        }
                        if slippage > 0.5 {
                            reasons.push(format!("-FAIL slippage {:.2}% > 0.5%", slippage));
                            passed = false;
                        } else {
                            reasons.push(format!("+OK slippage {:.2}%", slippage));
                        }
                    }
                    Err(e) => {
                        reasons.push(format!("-WARN orderbook check failed: {}", e));
                        // Don't fail on orderbook errors, just note it
                    }
                }
            } else {
                reasons.push("-WARN no token_id available".to_string());
            }
        }

        // Category correlation check (max 2 open per category)
        let mut would_trade = passed;
        if passed {
            if let Some(ref cat) = market.category {
                let cat_count = self.db.mc_get_category_trade_count(cat).await.unwrap_or(0);
                if cat_count >= 2 {
                    reasons.push(format!("-SKIP category '{}' already has {} open trades", cat, cat_count));
                    would_trade = false;
                }
            }

            // Check max positions
            let open_count = self.db.mc_get_open_trade_count().await.unwrap_or(0);
            if open_count >= tier_def.max_positions as i64 {
                reasons.push(format!("-SKIP max positions {} reached", tier_def.max_positions));
                would_trade = false;
            }
        }

        let side_str = format!("{}", fav_side);
        let now = Utc::now().to_rfc3339();

        Ok(Some(McScoutResult {
            market_id: market.id.clone(),
            condition_id: market.condition_id.clone(),
            question: market.question.clone(),
            slug: market.slug.clone(),
            side: side_str,
            price: fav_price.to_string(),
            volume: market.volume.to_string(),
            category: market.category.clone(),
            end_date: market.end_date.map(|d| d.to_rfc3339()),
            passed,
            certainty_score,
            reasons,
            slippage_pct,
            would_trade,
            token_id,
            scanned_at: now,
        }))
    }

    /// Score resolution certainty based on keywords and market properties
    fn resolution_certainty_score(
        &self,
        market: &TrackedMarket,
        disputes: &[DisputeAlert],
    ) -> (i32, Vec<String>) {
        let mut score: i32 = 50; // Base score
        let mut reasons = Vec::new();

        let question_lower = market.question.to_lowercase();
        let desc_lower = market.description.as_deref().unwrap_or("").to_lowercase();
        let source_lower = market.resolution_source.as_deref().unwrap_or("").to_lowercase();
        let combined = format!("{} {} {}", question_lower, desc_lower, source_lower);

        // Positive signals
        let mechanical_keywords = [
            "official", "chainlink", "api", "oracle", "data feed",
            "espn", "ap news", "reuters", "associated press",
            "sec filing", "government", "federal register",
        ];
        if mechanical_keywords.iter().any(|k| combined.contains(k)) {
            score += 30;
            reasons.push("+30 mechanical resolution source".to_string());
        }

        let determined_keywords = [
            "already determined", "outcome known", "result confirmed",
            "winner announced", "officially",
        ];
        if determined_keywords.iter().any(|k| combined.contains(k)) {
            score += 20;
            reasons.push("+20 outcome appears determined".to_string());
        }

        // Time decay bonus: closer to end = more likely resolved correctly
        if let Some(hours) = market.hours_until_close {
            if hours < 24.0 {
                score += 15;
                reasons.push("+15 resolves within 24h".to_string());
            } else if hours < 72.0 {
                score += 10;
                reasons.push("+10 resolves within 3 days".to_string());
            }
        }

        let unambiguous_keywords = [
            "binary", "yes or no", "will", "did", "has",
            "above", "below", "before", "after", "by",
        ];
        let unamb_count = unambiguous_keywords.iter().filter(|k| question_lower.contains(*k)).count();
        if unamb_count >= 2 {
            score += 10;
            reasons.push("+10 unambiguous question phrasing".to_string());
        }

        // Negative signals
        let subjective_keywords = [
            "likely", "probably", "opinion", "sentiment", "consensus",
            "believe", "expect", "forecast", "predict",
        ];
        if subjective_keywords.iter().any(|k| combined.contains(k)) {
            score -= 40;
            reasons.push("-40 subjective language detected".to_string());
        }

        // Active dispute penalty
        if disputes.iter().any(|d| d.condition_id == market.condition_id) {
            score -= 30;
            reasons.push("-30 active dispute on market".to_string());
        }

        // Category dispute history (check if this category has had disputes)
        if let Some(ref cat) = market.category {
            let dispute_cats = ["Politics", "Pop Culture"];
            if dispute_cats.iter().any(|c| cat.contains(c)) {
                score -= 20;
                reasons.push(format!("-20 category '{}' has dispute history", cat));
            }
        }

        // Single human resolution
        let human_keywords = ["single judge", "panel decision", "editorial", "moderator"];
        if human_keywords.iter().any(|k| combined.contains(k)) {
            score -= 15;
            reasons.push("-15 single human resolver".to_string());
        }

        // Price moved (high price already reflects certainty, less edge after fees)
        let fav_price = market.favorite().1.to_f64().unwrap_or(0.0);
        if fav_price > 0.96 {
            score -= 10;
            reasons.push("-10 price >96c (thin margin after fees)".to_string());
        }

        // Clamp score
        score = score.clamp(0, 100);

        (score, reasons)
    }

    /// Check orderbook depth for a given bet size
    async fn check_orderbook_depth(
        &self,
        token_id: &str,
        bet_size: f64,
    ) -> Result<(bool, Decimal, f64)> {
        let url = format!("https://clob.polymarket.com/book?token_id={}", token_id);

        let resp: OrderbookResponse = self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?
            .json()
            .await?;

        if resp.asks.is_empty() {
            return Ok((false, Decimal::ZERO, 100.0));
        }

        // Walk asks to calculate average fill price for bet_size
        let mut remaining = bet_size;
        let mut total_cost = 0.0;
        let mut total_shares = 0.0;

        for level in &resp.asks {
            let price = f64::from_str(&level.price).unwrap_or(0.0);
            let size = f64::from_str(&level.size).unwrap_or(0.0);

            if price <= 0.0 || size <= 0.0 {
                continue;
            }

            // How much USDC we can spend at this level
            let available_usdc = price * size;
            let fill_usdc = remaining.min(available_usdc);
            let fill_shares = fill_usdc / price;

            total_cost += fill_usdc;
            total_shares += fill_shares;
            remaining -= fill_usdc;

            if remaining <= 0.01 {
                break;
            }
        }

        if remaining > 0.01 {
            // Not enough depth
            return Ok((false, Decimal::ZERO, 100.0));
        }

        let avg_fill = total_cost / total_shares;
        let best_ask = f64::from_str(&resp.asks[0].price).unwrap_or(0.0);
        let slippage = if best_ask > 0.0 {
            ((avg_fill - best_ask) / best_ask) * 100.0
        } else {
            0.0
        };

        let avg_fill_dec = Decimal::from_f64_retain(avg_fill).unwrap_or(Decimal::ZERO);

        Ok((true, avg_fill_dec, slippage))
    }

    /// Simulate a trade entry
    async fn simulate_trade(
        &self,
        scout: &McScoutResult,
        config: &McConfig,
        tier_def: &TierDef,
    ) -> Result<()> {
        let price = Decimal::from_str(&scout.price)?;
        let bet_size = Decimal::from_f64_retain(tier_def.bet_size).unwrap_or(Decimal::from(5));

        // shares = bet_size / price
        let shares = if price > Decimal::ZERO { bet_size / price } else { Decimal::ZERO };

        self.db.mc_insert_trade(
            &scout.market_id,
            scout.condition_id.as_str(),
            &scout.question,
            &scout.slug,
            &scout.side,
            &scout.price,
            &bet_size.to_string(),
            &shares.to_string(),
            scout.certainty_score,
            scout.category.as_deref(),
            config.tier,
            scout.token_id.as_deref(),
            scout.end_date.as_deref(),
        ).await?;

        info!(
            "MC simulated trade: {} {} @ {}c (tier {}, ${} bet)",
            scout.side, scout.question.chars().take(50).collect::<String>(),
            scout.price, config.tier, tier_def.bet_size
        );

        Ok(())
    }

    /// Check if any simulated trades have resolved
    async fn check_simulated_resolutions(&self) -> Result<()> {
        let open_trades = self.db.mc_get_open_trades().await?;

        if open_trades.is_empty() {
            return Ok(());
        }

        for trade in &open_trades {
            // Check via Gamma API if market has resolved
            let url = format!(
                "https://gamma-api.polymarket.com/markets?id={}",
                trade.market_id
            );

            match self.client.get(&url).timeout(std::time::Duration::from_secs(10)).send().await {
                Ok(resp) => {
                    if let Ok(markets) = resp.json::<Vec<GammaMarket>>().await {
                        if let Some(gm) = markets.first() {
                            if gm.closed == Some(true) || gm.resolved == Some(true) {
                                // Determine exit price based on resolution
                                let won = self.did_trade_win(&trade, gm);
                                let exit_price = if won { "1.0" } else { "0.0" };

                                let entry = f64::from_str(&trade.entry_price).unwrap_or(0.0);
                                let shares = f64::from_str(&trade.shares).unwrap_or(0.0);
                                let exit = f64::from_str(exit_price).unwrap_or(0.0);
                                let pnl = (exit - entry) * shares;

                                self.db.mc_update_trade_resolution(
                                    trade.id,
                                    exit_price,
                                    &format!("{:.6}", pnl),
                                    if won { "won" } else { "lost" },
                                ).await?;

                                // Update bankroll
                                let config = self.db.mc_get_config().await?;
                                let current_bankroll = f64::from_str(&config.bankroll).unwrap_or(40.0);
                                let new_bankroll = current_bankroll + pnl;
                                let peak = f64::from_str(&config.peak_bankroll).unwrap_or(40.0);
                                let new_peak = peak.max(new_bankroll);

                                self.db.mc_update_bankroll(
                                    &format!("{:.2}", new_bankroll),
                                    &format!("{:.2}", new_peak),
                                ).await?;

                                info!(
                                    "MC trade resolved: {} — {} (PnL: ${:.2}, bankroll: ${:.2})",
                                    trade.question.chars().take(40).collect::<String>(),
                                    if won { "WON" } else { "LOST" },
                                    pnl,
                                    new_bankroll
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    debug!("MC resolution check failed for {}: {}", trade.market_id, e);
                }
            }
        }

        Ok(())
    }

    /// Determine if a simulated trade won based on resolution
    fn did_trade_win(&self, trade: &McTradeRow, market: &GammaMarket) -> bool {
        // Check the resolution outcome
        let outcome = market.outcome.as_deref().unwrap_or("");
        match trade.side.as_str() {
            "YES" | "Yes" => outcome == "Yes" || outcome == "yes" || outcome == "1",
            "NO" | "No" => outcome == "No" || outcome == "no" || outcome == "0",
            _ => false,
        }
    }

    /// Update risk state: drawdown checks, tier promotion/demotion
    async fn update_risk_state(&self) -> Result<()> {
        let config = self.db.mc_get_config().await?;
        let bankroll = f64::from_str(&config.bankroll).unwrap_or(40.0);
        let peak = f64::from_str(&config.peak_bankroll).unwrap_or(40.0);
        let tier_def = get_tier_def(config.tier);

        // Drawdown calculation
        let drawdown_pct = if peak > 0.0 {
            ((peak - bankroll) / peak) * 100.0
        } else {
            0.0
        };

        // Drawdown checks
        if drawdown_pct >= 35.0 && config.pause_state == "active" {
            // Pause trading for 48 hours
            let pause_until = (Utc::now() + Duration::hours(48)).to_rfc3339();
            self.db.mc_update_pause_state("drawdown_paused", Some(&pause_until)).await?;
            self.db.mc_insert_drawdown_event(
                "pause",
                &config.peak_bankroll,
                &config.bankroll,
                drawdown_pct,
                "48h pause: drawdown >= 35%",
            ).await?;
            info!("MC PAUSED: {:.1}% drawdown from peak ${}", drawdown_pct, peak);
        } else if drawdown_pct >= 20.0 && config.pause_state == "active" {
            // Reduce position size
            self.db.mc_update_pause_state("drawdown_reduced", None).await?;
            self.db.mc_insert_drawdown_event(
                "reduce",
                &config.peak_bankroll,
                &config.bankroll,
                drawdown_pct,
                "Position size halved: drawdown >= 20%",
            ).await?;
            info!("MC REDUCED: {:.1}% drawdown, halving position size", drawdown_pct);
        } else if drawdown_pct < 15.0 && config.pause_state == "drawdown_reduced" {
            // Resume normal trading
            self.db.mc_update_pause_state("active", None).await?;
            info!("MC RESUMED: drawdown recovered to {:.1}%", drawdown_pct);
        }

        // Weekly loss check (2 losses in 7 days = 48h pause)
        let recent_losses = self.db.mc_get_recent_losses(7).await.unwrap_or(0);
        if recent_losses >= 2 && config.pause_state == "active" {
            let pause_until = (Utc::now() + Duration::hours(48)).to_rfc3339();
            self.db.mc_update_pause_state("weekly_loss_pause", Some(&pause_until)).await?;
            info!("MC PAUSED: {} losses in 7 days, pausing 48h", recent_losses);
        }

        // Tier promotion/demotion
        if bankroll >= tier_def.promote_at && config.tier < 7 {
            let new_tier = config.tier + 1;
            self.db.mc_update_tier(new_tier).await?;
            self.db.mc_insert_tier_history(
                config.tier,
                new_tier,
                &config.bankroll,
                &format!("Promoted: bankroll ${:.2} >= ${:.2}", bankroll, tier_def.promote_at),
            ).await?;
            info!("MC PROMOTED to tier {} (bankroll: ${:.2})", new_tier, bankroll);
        } else if bankroll <= tier_def.demote_at && config.tier > 1 {
            let new_tier = config.tier - 1;
            self.db.mc_update_tier(new_tier).await?;
            self.db.mc_insert_tier_history(
                config.tier,
                new_tier,
                &config.bankroll,
                &format!("Demoted: bankroll ${:.2} <= ${:.2}", bankroll, tier_def.demote_at),
            ).await?;
            info!("MC DEMOTED to tier {} (bankroll: ${:.2})", new_tier, bankroll);
        }

        Ok(())
    }

    /// Build and broadcast status update
    async fn broadcast_status(
        &self,
        mc_tx: &broadcast::Sender<McStatusUpdate>,
        recent_scouts: &[McScoutResult],
        config: &McConfig,
        tier_def: &TierDef,
    ) -> Result<()> {
        let bankroll = f64::from_str(&config.bankroll).unwrap_or(40.0);
        let peak = f64::from_str(&config.peak_bankroll).unwrap_or(40.0);
        let drawdown = if peak > 0.0 { ((peak - bankroll) / peak) * 100.0 } else { 0.0 };

        // Get trade stats
        let (total_trades, wins, total_pnl) = self.db.mc_get_trade_stats().await.unwrap_or((0, 0, 0.0));
        let win_rate = if total_trades > 0 { (wins as f64 / total_trades as f64) * 100.0 } else { 0.0 };
        let open_trades = self.db.mc_get_open_trade_count().await.unwrap_or(0);

        // Effective bet size (halved if in drawdown_reduced state)
        let effective_bet = if config.pause_state == "drawdown_reduced" {
            tier_def.bet_size / 2.0
        } else {
            tier_def.bet_size
        };

        let status = McStatusUpdate {
            mode: config.mode.clone(),
            tier: config.tier,
            bankroll: config.bankroll.clone(),
            bet_size: format!("{:.2}", effective_bet),
            win_rate,
            total_pnl: format!("{:.2}", total_pnl),
            total_trades,
            open_trades,
            drawdown_pct: drawdown,
            peak_bankroll: config.peak_bankroll.clone(),
            pause_state: config.pause_state.clone(),
            pause_until: config.pause_until.clone(),
            recent_scouts: recent_scouts.to_vec(),
            max_positions: tier_def.max_positions,
        };

        let _ = mc_tx.send(status);
        Ok(())
    }
}

// ==================== DB CONFIG TYPE ====================

/// In-memory representation of mc_config row
#[derive(Debug, Clone)]
pub struct McConfig {
    pub bankroll: String,
    pub tier: i32,
    pub mode: String,
    pub peak_bankroll: String,
    pub pause_state: String,
    pub pause_until: Option<String>,
}

/// Row from mc_trades for resolution checking
#[derive(Debug, Clone)]
pub struct McTradeRow {
    pub id: i64,
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub side: String,
    pub entry_price: String,
    pub shares: String,
    pub status: String,
}

/// Gamma API market response
#[derive(Debug, Deserialize)]
struct GammaMarket {
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    resolved: Option<bool>,
    #[serde(default)]
    outcome: Option<String>,
}
