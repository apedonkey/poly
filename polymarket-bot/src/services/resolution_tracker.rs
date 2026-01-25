//! Resolution Tracker Service
//! Monitors open positions and updates them when markets resolve

use crate::types::{Position, Side};
use crate::Database;
use anyhow::Result;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Market data from Polymarket Gamma API
#[derive(Debug, Deserialize)]
struct GammaMarket {
    #[serde(rename = "conditionId")]
    condition_id: Option<String>,
    #[serde(rename = "questionID")]
    question_id: Option<String>,
    slug: Option<String>,
    resolved: Option<bool>,
    /// "Yes" or "No" - the winning outcome
    resolution: Option<String>,
    /// Outcome prices as JSON string like "[\"0.95\", \"0.05\"]"
    #[serde(rename = "outcomePrices")]
    outcome_prices: Option<String>,
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
    async fn check_resolutions(&self) -> Result<()> {
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
        }

        Ok(())
    }

    /// Check if a specific position's market has resolved
    async fn check_position_resolution(&self, position: &Position) -> Result<Option<(bool, Decimal)>> {
        // Fetch market data from Gamma API
        let market = self.fetch_market(&position.market_id).await?;

        // Check if market is resolved
        let is_resolved = market.resolved.unwrap_or(false);
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
        let we_won = match (&position.side, resolution.to_lowercase().as_str()) {
            (Side::Yes, "yes") | (Side::Yes, "1") => true,
            (Side::No, "no") | (Side::No, "0") => true,
            _ => false,
        };

        // Calculate PnL
        // If we won: we get $1 per share, so profit = (1 - entry_price) * size
        // If we lost: we lose our stake, so loss = entry_price * size
        let pnl = if we_won {
            // Winning: each share pays out $1
            // Profit = (payout - cost) = (1.0 - entry_price) * size
            (Decimal::ONE - position.entry_price) * position.size
        } else {
            // Losing: shares worth $0
            // Loss = -entry_price * size
            -position.entry_price * position.size
        };

        // Determine exit price (1.0 if won, 0.0 if lost)
        let exit_price = if we_won { Decimal::ONE } else { Decimal::ZERO };

        // Update position in database
        self.db.close_position(position.id, exit_price, pnl).await?;

        Ok(Some((we_won, pnl)))
    }

    /// Fetch market data from Polymarket Gamma API
    async fn fetch_market(&self, market_id: &str) -> Result<GammaMarket> {
        // Try fetching by condition ID first
        let url = format!(
            "https://gamma-api.polymarket.com/markets?condition_id={}",
            market_id
        );

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            // Try by slug if condition ID fails
            return self.fetch_market_by_slug(market_id).await;
        }

        let markets: Vec<GammaMarket> = response.json().await?;

        markets.into_iter().next().ok_or_else(|| {
            anyhow::anyhow!("Market not found: {}", market_id)
        })
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
    fn determine_winner_from_prices(&self, prices_str: &str) -> Result<String> {
        // Parse prices like "[\"1\", \"0\"]" or "[\"0.0\", \"1.0\"]"
        let prices: Vec<String> = serde_json::from_str(prices_str)
            .unwrap_or_else(|_| vec![]);

        if prices.len() >= 2 {
            let yes_price: f64 = prices[0].parse().unwrap_or(0.0);
            let no_price: f64 = prices[1].parse().unwrap_or(0.0);

            // Winner has price ~1.0
            if yes_price > 0.9 {
                return Ok("Yes".to_string());
            } else if no_price > 0.9 {
                return Ok("No".to_string());
            }
        }

        Err(anyhow::anyhow!("Could not determine winner from prices: {}", prices_str))
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
