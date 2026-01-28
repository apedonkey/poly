//! Resolution Tracker Service
//! Monitors open positions and updates them when markets resolve

use crate::types::{Position, Side};
use crate::Database;
use anyhow::Result;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Market data from Polymarket Gamma API
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaMarket {
    condition_id: Option<String>,
    #[serde(rename = "questionID")]
    question_id: Option<String>,
    slug: Option<String>,
    resolved: Option<bool>,
    /// "Yes" or "No" - the winning outcome
    resolution: Option<String>,
    /// Outcome prices as JSON string like "[\"0.95\", \"0.05\"]"
    outcome_prices: Option<String>,
    /// Market end date (full ISO timestamp)
    end_date: Option<String>,
    /// Market end date (ISO date only, e.g. "2024-01-15")
    end_date_iso: Option<String>,
    /// UMA resolution status - "resolved" when market is resolved
    uma_resolution_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GammaResponse {
    #[serde(default)]
    data: Vec<GammaMarket>,
}

/// Resolution tracker service
pub struct ResolutionTracker {
    db: Arc<Database>,
    client: reqwest::Client,
}

impl ResolutionTracker {
    pub fn new(db: Arc<Database>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { db, client }
    }

    /// Start the resolution tracking loop
    pub async fn run(&self, check_interval: Duration) {
        info!("Resolution tracker started (interval: {:?})", check_interval);

        loop {
            if let Err(e) = self.check_resolutions().await {
                error!("Resolution check failed: {}", e);
            }

            tokio::time::sleep(check_interval).await;
        }
    }

    /// Check all open positions for resolutions
    pub async fn check_resolutions(&self) -> Result<()> {
        let open_positions = self.db.get_open_positions().await?;

        if open_positions.is_empty() {
            debug!("No open positions to check");
            return Ok(());
        }

        info!("Checking {} open positions for resolutions", open_positions.len());

        for position in open_positions {
            match self.check_position_resolution(&position).await {
                Ok(Some((resolved, pnl))) => {
                    info!(
                        "Position {} resolved: {} | PnL: {} | Market: {}",
                        position.id,
                        if resolved { "WON" } else { "LOST" },
                        pnl,
                        position.question
                    );
                }
                Ok(None) => {
                    debug!("Position {} still open", position.id);
                }
                Err(e) => {
                    warn!("Failed to check position {}: {}", position.id, e);
                }
            }

            // Rate limit: wait 500ms between API calls to avoid being rate limited
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    /// Check if a specific position's market has resolved
    async fn check_position_resolution(&self, position: &Position) -> Result<Option<(bool, Decimal)>> {
        // Fetch market data from Gamma API
        // Prefer slug if available (more reliable), fall back to market_id
        let market = if let Some(slug) = &position.slug {
            self.fetch_market_by_slug(slug).await?
        } else {
            self.fetch_market(&position.market_id).await?
        };

        // If position doesn't have end_date, update it from market data
        if position.end_date.is_none() {
            if let Some(end_date) = self.parse_market_end_date(&market) {
                if let Err(e) = self.db.update_position_end_date(position.id, end_date).await {
                    warn!("Failed to update end_date for position {}: {}", position.id, e);
                } else {
                    debug!("Updated end_date for position {} to {}", position.id, end_date);
                }
            }
        }

        // Check if market is resolved
        // Some markets use `resolved: true`, others use `umaResolutionStatus: "resolved"`
        let is_resolved = market.resolved.unwrap_or(false)
            || market.uma_resolution_status.as_deref() == Some("resolved");
        if !is_resolved {
            return Ok(None);
        }

        // Get the winning outcome
        let resolution = match &market.resolution {
            Some(r) => r.clone(),
            None => {
                // Try to determine from outcome prices (winner = 1.0, loser = 0.0)
                if let Some(prices_str) = &market.outcome_prices {
                    self.determine_winner_from_prices(prices_str)?
                } else {
                    warn!("Market resolved but no resolution or prices found: {}", position.market_id);
                    return Ok(None);
                }
            }
        };

        // Determine if we won
        // Resolution can be: "Yes", "No", "yes", "no", "1", "0", "1.0", "0.0", etc.
        let resolution_lower = resolution.to_lowercase();
        let resolution_trimmed = resolution_lower.trim();

        let yes_won = resolution_trimmed == "yes"
            || resolution_trimmed == "1"
            || resolution_trimmed == "1.0"
            || resolution_trimmed.starts_with("yes");

        let no_won = resolution_trimmed == "no"
            || resolution_trimmed == "0"
            || resolution_trimmed == "0.0"
            || resolution_trimmed.starts_with("no");

        let we_won = match &position.side {
            Side::Yes => yes_won,
            Side::No => no_won,
        };

        debug!(
            "Position {} resolution check: side={:?}, resolution='{}', yes_won={}, no_won={}, we_won={}",
            position.id, position.side, resolution, yes_won, no_won, we_won
        );

        // Calculate PnL correctly:
        // When you buy shares at price P with SIZE dollars:
        // - Number of shares = SIZE / P
        //
        // If you WIN (shares worth $1 each):
        // - Payout = shares * $1 = SIZE / P
        // - Profit = payout - cost = (SIZE / P) - SIZE = SIZE * (1 - P) / P
        //
        // If you LOSE (shares worth $0):
        // - Payout = $0
        // - Loss = -SIZE (you lose your entire stake)
        let shares = position.size / position.entry_price;
        let pnl = if we_won {
            // Winning: each share pays out $1
            // Profit = (shares * $1) - cost = shares - size
            shares - position.size
        } else {
            // Losing: shares worth $0, we lose our entire stake
            -position.size
        };

        // Determine exit price (1.0 if won, 0.0 if lost)
        let exit_price = if we_won { Decimal::ONE } else { Decimal::ZERO };

        // Update position in database (PnL is calculated inside close_position)
        self.db.close_position(position.id, exit_price, None).await?;

        Ok(Some((we_won, pnl)))
    }

    /// Fetch market data from Polymarket Gamma API
    async fn fetch_market(&self, market_id: &str) -> Result<GammaMarket> {
        // Try fetching by numeric ID first (market_id is the numeric ID like "1273344")
        let url = format!(
            "https://gamma-api.polymarket.com/markets?id={}",
            market_id
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            // Try by slug if ID fails
            return self.fetch_market_by_slug(market_id).await;
        }

        let markets: Vec<GammaMarket> = response.json().await?;

        if let Some(market) = markets.into_iter().next() {
            return Ok(market);
        }

        // Fall back to slug query if no results
        self.fetch_market_by_slug(market_id).await
    }

    /// Fetch market by slug as fallback
    async fn fetch_market_by_slug(&self, slug: &str) -> Result<GammaMarket> {
        let url = format!(
            "https://gamma-api.polymarket.com/markets?slug={}",
            slug
        );

        let response = self.client.get(&url).send().await?;
        let markets: Vec<GammaMarket> = response.json().await?;

        markets.into_iter().next().ok_or_else(|| {
            anyhow::anyhow!("Market not found by slug: {}", slug)
        })
    }

    /// Determine winner from outcome prices (winner = 1.0, loser = 0.0)
    /// Note: For binary markets, if the first outcome (index 0) has price ~1.0, YES won.
    /// If the second outcome (index 1) has price ~1.0, NO won.
    /// This is based on Polymarket's convention where outcome 0 = YES, outcome 1 = NO for binary markets.
    fn determine_winner_from_prices(&self, prices_str: &str) -> Result<String> {
        // Parse prices like "[\"1\", \"0\"]" or "[\"0.0\", \"1.0\"]"
        let prices: Vec<String> = serde_json::from_str(prices_str)
            .unwrap_or_else(|_| vec![]);

        if prices.len() >= 2 {
            let price_0: f64 = prices[0].parse().unwrap_or(0.0);
            let price_1: f64 = prices[1].parse().unwrap_or(0.0);

            // In Polymarket binary markets:
            // - Outcome index 0 typically corresponds to YES
            // - Outcome index 1 typically corresponds to NO
            // Winner has price ~1.0
            if price_0 > 0.9 {
                // First outcome (YES) won
                return Ok("Yes".to_string());
            } else if price_1 > 0.9 {
                // Second outcome (NO) won
                return Ok("No".to_string());
            }
        }

        Err(anyhow::anyhow!("Could not determine winner from prices: {}", prices_str))
    }

    /// Parse market end date from Gamma API response
    fn parse_market_end_date(&self, market: &GammaMarket) -> Option<DateTime<Utc>> {
        // Try full ISO timestamp first (end_date)
        if let Some(end_date_str) = &market.end_date {
            if let Ok(dt) = DateTime::parse_from_rfc3339(end_date_str) {
                return Some(dt.with_timezone(&Utc));
            }
            // Try without timezone
            if let Ok(dt) = NaiveDateTime::parse_from_str(end_date_str, "%Y-%m-%dT%H:%M:%S") {
                return Some(dt.and_utc());
            }
        }

        // Try date-only format (end_date_iso) with midnight UTC
        if let Some(date_str) = &market.end_date_iso {
            if let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                let datetime = NaiveDateTime::new(date, NaiveTime::from_hms_opt(23, 59, 59).unwrap());
                return Some(datetime.and_utc());
            }
        }

        None
    }
}

/// Check a single market's resolution status (utility function)
pub async fn check_market_resolved(market_id: &str) -> Result<Option<String>> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0")
        .build()?;

    let url = format!(
        "https://gamma-api.polymarket.com/markets?condition_id={}",
        market_id
    );

    let response = client.get(&url).send().await?;
    let markets: Vec<GammaMarket> = response.json().await?;

    if let Some(market) = markets.first() {
        if market.resolved.unwrap_or(false) {
            return Ok(market.resolution.clone());
        }
    }

    Ok(None)
}
