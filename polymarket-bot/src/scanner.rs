//! Market scanner for Polymarket Gamma API

use crate::config::{Config, GammaApi};
use crate::types::TrackedMarket;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;
use tracing::{debug, info, warn};

/// Scanner for fetching and processing Polymarket markets
pub struct Scanner {
    client: Client,
    config: Config,
}

/// Raw market response from Gamma API
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
    resolution_source: Option<String>,
    #[serde(default)]
    end_date_iso: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(default)]
    outcomes: Option<String>,
    #[serde(default)]
    outcome_prices: Option<String>,
    #[serde(default)]
    volume: Option<String>,
    #[serde(default)]
    liquidity: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    clob_token_ids: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Option<Vec<GammaTag>>,
    #[serde(default)]
    events: Option<Vec<GammaEvent>>,
}

#[derive(Debug, Deserialize)]
struct GammaTag {
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    slug: Option<String>,
}

/// Event data from Gamma API (parent of markets)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaEvent {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    title: Option<String>,
}

impl Scanner {
    pub fn new(config: Config) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    /// Fetch all active markets from Gamma API
    pub async fn fetch_markets(&self) -> Result<Vec<TrackedMarket>> {
        let mut all_markets = Vec::new();
        let mut offset = 0;
        let limit = 100;

        loop {
            let url = format!(
                "{}?active=true&closed=false&limit={}&offset={}",
                GammaApi::markets_url(),
                limit,
                offset
            );

            debug!("Fetching markets from: {}", url);

            let response = self
                .client
                .get(&url)
                .send()
                .await
                .context("Failed to fetch markets")?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("API error {}: {}", status, body);
            }

            let gamma_markets: Vec<GammaMarket> = response
                .json()
                .await
                .context("Failed to parse market response")?;

            let batch_size = gamma_markets.len();
            debug!("Fetched {} markets", batch_size);

            for gm in gamma_markets {
                if let Some(market) = self.parse_market(gm) {
                    all_markets.push(market);
                }
            }

            if batch_size < limit {
                break;
            }

            offset += limit;

            // Safety limit to avoid infinite loops
            if offset > 35000 {
                warn!("Reached safety limit of 35000 markets");
                break;
            }
        }

        info!("Total markets fetched: {}", all_markets.len());
        Ok(all_markets)
    }

    /// Fetch markets with specific filters for sniper strategy
    pub async fn fetch_closing_soon(&self, max_hours: f64) -> Result<Vec<TrackedMarket>> {
        let markets = self.fetch_markets().await?;

        let closing_soon: Vec<_> = markets
            .into_iter()
            .filter(|m| {
                m.hours_until_close
                    .map(|h| h > 0.0 && h <= max_hours)
                    .unwrap_or(false)
            })
            .collect();

        info!("Markets closing within {} hours: {}", max_hours, closing_soon.len());
        Ok(closing_soon)
    }

    /// Parse a Gamma API market into our TrackedMarket type
    fn parse_market(&self, gm: GammaMarket) -> Option<TrackedMarket> {
        // Parse end date - try end_date first (full ISO timestamp), then end_date_iso (just date)
        let end_date = gm
            .end_date
            .as_ref()
            .and_then(|d| DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.with_timezone(&Utc))
            .or_else(|| {
                // Try parsing end_date_iso with a default time
                gm.end_date_iso.as_ref().and_then(|d| {
                    let with_time = format!("{}T23:59:59Z", d);
                    DateTime::parse_from_rfc3339(&with_time)
                        .ok()
                        .map(|dt| dt.with_timezone(&Utc))
                })
            });

        // Calculate hours until close
        let hours_until_close = end_date.map(|end| {
            let now = Utc::now();
            let duration = end - now;
            duration.num_minutes() as f64 / 60.0
        });

        // Parse outcome prices (format: "[\"0.65\", \"0.35\"]")
        let (yes_price, no_price) = self.parse_outcome_prices(&gm.outcome_prices)?;

        // Parse volume and liquidity
        let volume = gm
            .volume
            .as_ref()
            .and_then(|v| Decimal::from_str(v).ok())
            .unwrap_or_default();

        let liquidity = gm
            .liquidity
            .as_ref()
            .and_then(|v| Decimal::from_str(v).ok())
            .unwrap_or_default();

        // Skip low liquidity markets
        if liquidity < self.config.min_liquidity {
            return None;
        }

        // Parse token IDs
        let (yes_token_id, no_token_id) = self.parse_token_ids(&gm.clob_token_ids);

        // Get event slug (correct URL slug) - fall back to market slug if no event
        let event_slug = gm.events
            .as_ref()
            .and_then(|events| events.first())
            .and_then(|e| e.slug.clone())
            .unwrap_or_else(|| gm.slug.clone());

        // Detect category from tags, question text, or slug
        let category = gm.category
            .or_else(|| {
                gm.tags
                    .as_ref()
                    .and_then(|tags| tags.first())
                    .and_then(|t| t.label.clone())
            })
            .or_else(|| self.detect_category_from_text(&gm.question, &event_slug));

        Some(TrackedMarket {
            id: gm.id,
            condition_id: gm.condition_id,
            question: gm.question,
            slug: event_slug,
            resolution_source: gm.resolution_source,
            end_date,
            yes_price,
            no_price,
            volume,
            liquidity,
            category,
            active: gm.active,
            closed: gm.closed,
            yes_token_id,
            no_token_id,
            hours_until_close,
        })
    }

    /// Detect category from question text and slug using keyword matching
    fn detect_category_from_text(&self, question: &str, slug: &str) -> Option<String> {
        let text = format!("{} {}", question.to_lowercase(), slug.to_lowercase());

        // Sports keywords
        let sports_keywords = [
            "nba", "nfl", "mlb", "nhl", "mls", "premier league", "la liga",
            "bundesliga", "serie a", "ligue 1", "champions league", "playoffs",
            "super bowl", "world cup", "world series", "stanley cup",
            "lakers", "celtics", "warriors", "yankees", "dodgers", "cowboys",
            "patriots", "chiefs", "bills", "eagles", "49ers", "packers",
            "soccer", "football", "basketball", "baseball", "hockey",
            "tennis", "golf", "formula 1", "f1", "ufc", "boxing", "mma",
            "win on 2026", "win on 2025", "match", "game", "vs", " vs ",
            "feyenoord", "galatasaray", "barcelona", "real madrid", "bayern",
            "manchester", "liverpool", "arsenal", "chelsea", "tottenham",
            "esports", "call of duty", "league of legends", "valorant", "csgo",
        ];

        for keyword in sports_keywords {
            if text.contains(keyword) {
                return Some("Sports".to_string());
            }
        }

        // Crypto keywords
        let crypto_keywords = [
            "bitcoin", "btc", "ethereum", "eth", "solana", "sol",
            "crypto", "token", "defi", "nft", "blockchain",
            "above $", "below $", "price", "market cap",
        ];

        for keyword in crypto_keywords {
            if text.contains(keyword) {
                return Some("Crypto".to_string());
            }
        }

        None
    }

    /// Parse outcome prices from JSON string
    fn parse_outcome_prices(&self, prices_str: &Option<String>) -> Option<(Decimal, Decimal)> {
        let prices_str = prices_str.as_ref()?;

        // Try parsing as JSON array
        if let Ok(prices) = serde_json::from_str::<Vec<String>>(prices_str) {
            if prices.len() >= 2 {
                let yes = Decimal::from_str(&prices[0]).ok()?;
                let no = Decimal::from_str(&prices[1]).ok()?;
                return Some((yes, no));
            }
        }

        // Try parsing as array of numbers
        if let Ok(prices) = serde_json::from_str::<Vec<f64>>(prices_str) {
            if prices.len() >= 2 {
                let yes = Decimal::try_from(prices[0]).ok()?;
                let no = Decimal::try_from(prices[1]).ok()?;
                return Some((yes, no));
            }
        }

        None
    }

    /// Parse token IDs from JSON string
    fn parse_token_ids(&self, ids_str: &Option<String>) -> (Option<String>, Option<String>) {
        let Some(ids_str) = ids_str else {
            return (None, None);
        };

        if let Ok(ids) = serde_json::from_str::<Vec<String>>(ids_str) {
            let yes_id = ids.first().cloned();
            let no_id = ids.get(1).cloned();
            return (yes_id, no_id);
        }

        (None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_outcome_prices() {
        let config = Config::from_env().unwrap();
        let scanner = Scanner::new(config);

        // Test JSON string array
        let prices = Some(r#"["0.65", "0.35"]"#.to_string());
        let result = scanner.parse_outcome_prices(&prices);
        assert!(result.is_some());
        let (yes, no) = result.unwrap();
        assert_eq!(yes, Decimal::from_str("0.65").unwrap());
        assert_eq!(no, Decimal::from_str("0.35").unwrap());
    }
}
