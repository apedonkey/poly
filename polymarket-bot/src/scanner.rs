//! Market scanner for Polymarket Gamma API

use crate::config::{Config, GammaApi};
use crate::types::TrackedMarket;
use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use regex::Regex;
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
    /// Full market description containing resolution rules
    #[serde(default)]
    description: Option<String>,
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
        // Parse end date from API - try end_date first (full ISO timestamp), then end_date_iso (just date)
        let api_end_date = gm
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

        // Try to extract a more accurate resolution date from the description
        // This handles cases where the API end_date is wrong (e.g., midnight vs actual event time)
        let description_end_date = gm
            .description
            .as_ref()
            .and_then(|desc| self.parse_resolution_date_from_description(desc));

        // Use the description date if:
        // 1. We found one, AND
        // 2. Either there's no API date, OR the description date is later (more specific)
        let end_date = match (description_end_date, api_end_date) {
            (Some(desc_date), Some(api_date)) => {
                // If description date is later than API date, use it (it's more specific)
                // e.g., API says midnight, description says 2:30 PM
                if desc_date > api_date {
                    debug!(
                        "Using description date {} instead of API date {} for market",
                        desc_date, api_date
                    );
                    Some(desc_date)
                } else {
                    Some(api_date)
                }
            }
            (Some(desc_date), None) => Some(desc_date),
            (None, Some(api_date)) => Some(api_date),
            (None, None) => None,
        };

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
            description: gm.description,
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

    /// Parse resolution date/time from market description text.
    /// Looks for common patterns like "by January 28, 2026, 11:59 PM ET"
    /// Returns None if no valid date is found.
    fn parse_resolution_date_from_description(&self, description: &str) -> Option<DateTime<Utc>> {
        // Common date patterns in Polymarket rules
        // Pattern: "January 28, 2026" or "January 28 2026"
        let date_pattern = r"(?i)(January|February|March|April|May|June|July|August|September|October|November|December)\s+(\d{1,2}),?\s+(\d{4})";

        // Pattern: "11:59 PM ET" or "2:30 PM EST" or "14:30 UTC"
        let time_pattern = r"(\d{1,2}):(\d{2})\s*(AM|PM|am|pm)?\s*(ET|EST|EDT|PT|PST|PDT|CT|CST|CDT|MT|MST|MDT|UTC)?";

        let date_re = Regex::new(date_pattern).ok()?;
        let time_re = Regex::new(time_pattern).ok()?;

        // Find all date matches
        let date_caps = date_re.captures(description)?;

        let month_str = date_caps.get(1)?.as_str();
        let day: u32 = date_caps.get(2)?.as_str().parse().ok()?;
        let year: i32 = date_caps.get(3)?.as_str().parse().ok()?;

        let month = match month_str.to_lowercase().as_str() {
            "january" => 1,
            "february" => 2,
            "march" => 3,
            "april" => 4,
            "may" => 5,
            "june" => 6,
            "july" => 7,
            "august" => 8,
            "september" => 9,
            "october" => 10,
            "november" => 11,
            "december" => 12,
            _ => return None,
        };

        let date = NaiveDate::from_ymd_opt(year, month, day)?;

        // Try to find a time near the date match
        let (hour, minute, tz_str) = if let Some(time_caps) = time_re.captures(description) {
            let mut hour: u32 = time_caps.get(1)?.as_str().parse().ok()?;
            let minute: u32 = time_caps.get(2)?.as_str().parse().ok()?;

            // Handle AM/PM
            if let Some(ampm) = time_caps.get(3) {
                let ampm_str = ampm.as_str().to_uppercase();
                if ampm_str == "PM" && hour != 12 {
                    hour += 12;
                } else if ampm_str == "AM" && hour == 12 {
                    hour = 0;
                }
            }

            let tz = time_caps.get(4).map(|m| m.as_str()).unwrap_or("ET");
            (hour, minute, tz)
        } else {
            // Default to 11:59 PM ET if no time specified
            (23, 59, "ET")
        };

        let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
        let naive_dt = NaiveDateTime::new(date, time);

        // Convert timezone to UTC
        let tz: Tz = match tz_str.to_uppercase().as_str() {
            "ET" | "EST" | "EDT" => "America/New_York".parse().ok()?,
            "PT" | "PST" | "PDT" => "America/Los_Angeles".parse().ok()?,
            "CT" | "CST" | "CDT" => "America/Chicago".parse().ok()?,
            "MT" | "MST" | "MDT" => "America/Denver".parse().ok()?,
            "UTC" => "UTC".parse().ok()?,
            _ => "America/New_York".parse().ok()?, // Default to ET
        };

        // Convert to UTC
        let local_dt = tz.from_local_datetime(&naive_dt).single()?;
        let utc_dt = local_dt.with_timezone(&Utc);

        // Only return if the date is in the future
        if utc_dt > Utc::now() {
            Some(utc_dt)
        } else {
            None
        }
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

    #[test]
    fn test_parse_resolution_date_common_format() {
        let config = Config::from_env().unwrap();
        let scanner = Scanner::new(config);

        // Test common Polymarket format: "by January 28, 2027, 11:59 PM ET"
        let desc = "This market will resolve to No if no such statement happens by January 28, 2027, 11:59 PM ET.";
        let result = scanner.parse_resolution_date_from_description(desc);
        assert!(result.is_some(), "Should parse date from description");

        let dt = result.unwrap();
        assert_eq!(dt.year(), 2027);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 28);
    }

    #[test]
    fn test_parse_resolution_date_with_time() {
        let config = Config::from_env().unwrap();
        let scanner = Scanner::new(config);

        // Test "at 2:30 PM ET on January 28, 2027"
        let desc = "Jerome Powell is scheduled to speak at 2:30 PM ET on January 28, 2027.";
        let result = scanner.parse_resolution_date_from_description(desc);
        assert!(result.is_some(), "Should parse date with specific time");
    }

    #[test]
    fn test_parse_resolution_date_no_time() {
        let config = Config::from_env().unwrap();
        let scanner = Scanner::new(config);

        // Test date without time - should default to 11:59 PM
        let desc = "This market resolves on December 31, 2027.";
        let result = scanner.parse_resolution_date_from_description(desc);
        assert!(result.is_some(), "Should parse date without time");
    }

    #[test]
    fn test_parse_resolution_date_past_date() {
        let config = Config::from_env().unwrap();
        let scanner = Scanner::new(config);

        // Past dates should return None
        let desc = "This market resolved on January 1, 2020.";
        let result = scanner.parse_resolution_date_from_description(desc);
        assert!(result.is_none(), "Past dates should return None");
    }
}
