//! Dedicated scanner for 15-minute crypto Up/Down markets.
//!
//! Queries the Gamma API by tag to find all currently open 15-minute
//! crypto markets in a single API call. No slug guessing needed.

use crate::config::GammaApi;
use crate::types::TrackedMarket;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;
use tracing::{debug, info};

/// Tag ID for "15M" markets on Polymarket
const TAG_15M: u32 = 102467;

/// Fetch all currently open 15-minute crypto markets via tag query.
///
/// Single API call: `/events?tag_id=102467&active=true&closed=false`
/// Returns all open BTC/ETH/SOL/XRP 15-min Up/Down markets.
pub async fn fetch_15m_crypto_markets(
    client: &reqwest::Client,
    _assets: &[String],
) -> Result<Vec<TrackedMarket>> {
    let url = format!(
        "{}?tag_id={}&active=true&closed=false&limit=20",
        GammaApi::events_url(),
        TAG_15M
    );

    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("15m scanner HTTP {}", resp.status());
    }

    // Use the server's Date header to get real UTC time (VPS clock may be wrong)
    let server_now = resp
        .headers()
        .get("date")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| DateTime::parse_from_rfc2822(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    let events: Vec<GammaEvent> = resp.json().await?;

    let mut all_markets = Vec::new();

    for event in &events {
        for market in &event.markets {
            // Only include non-closed markets that accept orders
            if market.closed {
                continue;
            }
            if !market.accepting_orders {
                continue;
            }

            // Parse token IDs from clobTokenIds (JSON array string)
            let token_ids: Vec<String> = match serde_json::from_str(&market.clob_token_ids) {
                Ok(ids) => ids,
                Err(_) => continue,
            };
            if token_ids.len() < 2 {
                continue;
            }

            // Parse outcome prices
            let prices: Vec<String> = match serde_json::from_str(&market.outcome_prices) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if prices.len() < 2 {
                continue;
            }

            let yes_price = Decimal::from_str(&prices[0]).unwrap_or(Decimal::ZERO);
            let no_price = Decimal::from_str(&prices[1]).unwrap_or(Decimal::ZERO);

            // Parse end date — use endDate (full RFC3339 timestamp), not endDateIso (date only)
            let end_date = market
                .end_date
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc));

            // Use server time instead of system clock for accurate time remaining
            let hours_until_close = end_date.map(|end| {
                let duration = end.signed_duration_since(server_now);
                duration.num_seconds() as f64 / 3600.0
            });

            let liquidity = market
                .liquidity_num
                .and_then(|v| Decimal::from_str(&format!("{}", v)).ok())
                .unwrap_or(Decimal::ZERO);

            let volume = market
                .volume_num
                .and_then(|v| Decimal::from_str(&format!("{}", v)).ok())
                .unwrap_or(Decimal::ZERO);

            let question = if market.question.is_empty() {
                event.title.clone()
            } else {
                market.question.clone()
            };

            let market_slug = market.slug.clone().unwrap_or_default();

            all_markets.push(TrackedMarket {
                id: market.condition_id.clone(),
                condition_id: market.condition_id.clone(),
                question,
                slug: market_slug,
                resolution_source: market.resolution_source.clone(),
                description: market.description.clone(),
                end_date,
                yes_price,
                no_price,
                volume,
                liquidity,
                category: Some("Crypto".to_string()),
                active: true,
                closed: false,
                yes_token_id: Some(token_ids[0].clone()),
                no_token_id: Some(token_ids[1].clone()),
                hours_until_close,
                neg_risk: market.neg_risk,
            });
        }
    }

    // Fetch live CLOB midpoint prices for markets currently in their 15-min window.
    // A market is "live" when server_now >= endDate - 15 minutes.
    let live_window = Duration::minutes(15);
    let mut live_count = 0;
    for market in &mut all_markets {
        let is_live = market.end_date.map_or(false, |end| {
            server_now >= end - live_window
        });
        if !is_live {
            continue;
        }
        live_count += 1;

        // Fetch midpoint for YES and NO tokens independently.
        // Don't derive one from the other — real book prices may not sum to 1.00.
        if let Some(ref yes_token) = market.yes_token_id {
            if let Some(mid) = fetch_clob_midpoint(client, yes_token).await {
                market.yes_price = mid;
            }
        }
        if let Some(ref no_token) = market.no_token_id {
            if let Some(mid) = fetch_clob_midpoint(client, no_token).await {
                market.no_price = mid;
            }
        }
    }

    info!(
        "MintMaker scanner: {} open 15m markets from {} events ({} live with CLOB prices)",
        all_markets.len(),
        events.len(),
        live_count,
    );

    Ok(all_markets)
}

/// Fetch the midpoint price for a single token from the CLOB.
async fn fetch_clob_midpoint(client: &reqwest::Client, token_id: &str) -> Option<Decimal> {
    let url = format!("https://clob.polymarket.com/midpoint?token_id={}", token_id);
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        debug!("CLOB midpoint HTTP {} for token {}", resp.status(), &token_id[..8.min(token_id.len())]);
        return None;
    }
    let body: MidpointResponse = resp.json().await.ok()?;
    Decimal::from_str(&body.mid).ok()
}

#[derive(Deserialize)]
struct MidpointResponse {
    mid: String,
}

// ==================== Gamma API Response Types ====================
// The /events endpoint returns camelCase JSON.

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GammaEvent {
    #[serde(default)]
    title: String,
    #[serde(default)]
    markets: Vec<GammaMarket>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct GammaMarket {
    #[serde(default)]
    condition_id: String,
    #[serde(default)]
    question: String,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    resolution_source: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    accepting_orders: bool,
    #[serde(default)]
    neg_risk: bool,
    /// JSON string: e.g. "[\"12345\",\"67890\"]"
    #[serde(default)]
    clob_token_ids: String,
    /// JSON string: e.g. "[\"0.45\",\"0.55\"]"
    #[serde(default)]
    outcome_prices: String,
    /// Full RFC3339 timestamp e.g. "2026-01-31T22:45:00Z"
    #[serde(default)]
    end_date: Option<String>,
    /// Date-only string e.g. "2026-01-31" (not useful for time calc)
    #[serde(default)]
    end_date_iso: Option<String>,
    /// Numeric liquidity (the `liquidity` field is a string, use `liquidityNum` instead)
    #[serde(default)]
    liquidity_num: Option<f64>,
    /// Numeric volume
    #[serde(default)]
    volume_num: Option<f64>,
}
