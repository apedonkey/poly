//! Clarification Monitor Service
//!
//! Monitors Polymarket markets for description/clarification changes.
//! When a market's description hash changes, emits a ClarificationAlert.

use crate::types::ClarificationAlert;
use crate::Database;
use anyhow::Result;
use chrono::Utc;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

/// Response from Gamma API for market data
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaMarket {
    id: String,
    #[serde(default)]
    condition_id: String,
    question: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    outcome_prices: Option<String>,
    #[serde(default)]
    liquidity: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    events: Option<Vec<GammaEvent>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaEvent {
    #[serde(default)]
    slug: Option<String>,
}

/// Clarification monitor service
pub struct ClarificationMonitor {
    db: Arc<Database>,
    client: Client,
    /// In-memory cache of market_id -> description_hash
    hashes: HashMap<String, String>,
}

impl ClarificationMonitor {
    pub fn new(db: Arc<Database>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            db,
            client,
            hashes: HashMap::new(),
        }
    }

    /// Load cached hashes from database on startup
    pub async fn load_hashes(&mut self) -> Result<()> {
        let stored = self.db.get_all_description_hashes().await?;
        for (market_id, hash) in stored {
            self.hashes.insert(market_id, hash);
        }
        info!("Loaded {} description hashes from database", self.hashes.len());
        Ok(())
    }

    /// Run the clarification monitor loop
    pub async fn run(
        mut self,
        interval: Duration,
        tx: broadcast::Sender<Vec<ClarificationAlert>>,
    ) {
        // Load cached hashes on startup
        if let Err(e) = self.load_hashes().await {
            warn!("Failed to load description hashes: {}", e);
        }

        loop {
            match self.check_for_changes().await {
                Ok(alerts) => {
                    if !alerts.is_empty() {
                        info!("Found {} clarification changes", alerts.len());
                        let _ = tx.send(alerts);
                    }
                }
                Err(e) => {
                    error!("Clarification monitor scan failed: {}", e);
                }
            }

            tokio::time::sleep(interval).await;
        }
    }

    /// Check all markets for description changes
    async fn check_for_changes(&mut self) -> Result<Vec<ClarificationAlert>> {
        let markets = self.fetch_markets().await?;
        let mut alerts = Vec::new();
        let now = Utc::now().timestamp();

        for market in markets {
            // Skip markets without descriptions
            let Some(description) = &market.description else {
                continue;
            };

            // Compute hash of current description
            let new_hash = self.compute_hash(description);

            // Check against cached hash
            let is_changed = match self.hashes.get(&market.id) {
                Some(old_hash) => *old_hash != new_hash,
                None => false, // First time seeing this market, not a "change"
            };

            if is_changed {
                let old_hash = self.hashes.get(&market.id).cloned().unwrap_or_default();

                // Parse prices
                let (yes_price, no_price) = self.parse_prices(&market.outcome_prices);
                let liquidity = market.liquidity
                    .as_ref()
                    .and_then(|l| Decimal::from_str(l).ok())
                    .unwrap_or_default();

                // Get event slug for URL
                let slug = market.events
                    .as_ref()
                    .and_then(|events| events.first())
                    .and_then(|e| e.slug.clone())
                    .unwrap_or_else(|| market.slug.clone());

                let alert = ClarificationAlert {
                    market_id: market.id.clone(),
                    condition_id: market.condition_id.clone(),
                    question: market.question.clone(),
                    slug,
                    old_description_hash: old_hash,
                    new_description_preview: description.chars().take(500).collect(),
                    detected_at: now,
                    current_yes_price: yes_price,
                    current_no_price: no_price,
                    liquidity,
                };

                info!(
                    "Description changed for market: {} ({})",
                    market.question,
                    market.id
                );
                alerts.push(alert);
            }

            // Update cache and database
            if self.hashes.get(&market.id) != Some(&new_hash) {
                self.hashes.insert(market.id.clone(), new_hash.clone());
                if let Err(e) = self.db.upsert_description_hash(&market.id, &new_hash).await {
                    warn!("Failed to store description hash: {}", e);
                }
            }
        }

        debug!("Checked {} markets for clarifications", self.hashes.len());
        Ok(alerts)
    }

    /// Fetch active markets from Gamma API
    async fn fetch_markets(&self) -> Result<Vec<GammaMarket>> {
        let mut all_markets = Vec::new();
        let mut offset = 0;
        let limit = 100;

        loop {
            let url = format!(
                "https://gamma-api.polymarket.com/markets?active=true&closed=false&limit={}&offset={}",
                limit, offset
            );

            let response = self.client.get(&url).send().await?;

            if !response.status().is_success() {
                anyhow::bail!("API error: {}", response.status());
            }

            let markets: Vec<GammaMarket> = response.json().await?;
            let batch_size = markets.len();

            all_markets.extend(markets);

            if batch_size < limit {
                break;
            }

            offset += limit;

            // Safety limit
            if offset > 35000 {
                break;
            }
        }

        Ok(all_markets)
    }

    /// Compute SHA256 hash of description text
    fn compute_hash(&self, description: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(description.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Parse outcome prices from JSON string
    fn parse_prices(&self, prices_str: &Option<String>) -> (Decimal, Decimal) {
        let Some(prices_str) = prices_str else {
            return (Decimal::ZERO, Decimal::ZERO);
        };

        if let Ok(prices) = serde_json::from_str::<Vec<String>>(prices_str) {
            if prices.len() >= 2 {
                let yes = Decimal::from_str(&prices[0]).unwrap_or_default();
                let no = Decimal::from_str(&prices[1]).unwrap_or_default();
                return (yes, no);
            }
        }

        (Decimal::ZERO, Decimal::ZERO)
    }
}
