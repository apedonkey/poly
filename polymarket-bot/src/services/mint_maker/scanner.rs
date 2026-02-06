//! Dedicated scanner for 15-minute crypto Up/Down markets.
//!
//! Queries the Gamma API by tag to find all currently open 15-minute
//! crypto markets in a single API call. No slug guessing needed.

use crate::config::GammaApi;
use crate::types::TrackedMarket;
use anyhow::Result;
use chrono::{DateTime, Utc};
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

    // Fetch CLOB midpoint prices for ALL markets (including upcoming/future ones).
    // The CLOB accepts orders before the 15-min window opens, so midpoints are
    // available early and give us real book prices instead of stale Gamma data.
    let mut midpoint_count = 0;
    for market in &mut all_markets {
        // Skip already-closed markets
        let is_open = market.end_date.map_or(false, |end| server_now < end);
        if !is_open {
            continue;
        }

        // Fetch midpoint for YES and NO tokens independently.
        // Don't derive one from the other — real book prices may not sum to 1.00.
        let mut got_midpoint = false;
        if let Some(ref yes_token) = market.yes_token_id {
            if let Some(mid) = fetch_clob_midpoint(client, yes_token).await {
                market.yes_price = mid;
                got_midpoint = true;
            }
        }
        if let Some(ref no_token) = market.no_token_id {
            if let Some(mid) = fetch_clob_midpoint(client, no_token).await {
                market.no_price = mid;
                got_midpoint = true;
            }
        }
        if got_midpoint {
            midpoint_count += 1;
        }
    }

    info!(
        "MintMaker scanner: {} open 15m markets from {} events ({} with CLOB midpoints)",
        all_markets.len(),
        events.len(),
        midpoint_count,
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

// ==================== Orderbook Depth Analysis ====================

#[derive(Debug, Deserialize)]
struct OrderbookResponse {
    #[serde(default)]
    bids: Vec<OrderbookLevel>,
    #[serde(default)]
    asks: Vec<OrderbookLevel>,
}

#[derive(Debug, Deserialize)]
struct OrderbookLevel {
    price: String,
    size: String,
}

/// Result of orderbook depth analysis
#[derive(Debug, Clone)]
pub struct DepthAnalysis {
    /// Total USD value of bids
    pub total_bid_value: Decimal,
    /// Best bid price
    pub best_bid: Decimal,
    /// Number of bid levels
    pub bid_levels: usize,
}

/// Fetch orderbook and calculate total bid depth for a token.
/// Returns the sum of (price * size) for all bids.
pub async fn fetch_orderbook_depth(
    client: &reqwest::Client,
    token_id: &str,
) -> Option<DepthAnalysis> {
    let url = format!("https://clob.polymarket.com/book?token_id={}", token_id);
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        debug!("CLOB book HTTP {} for token {}", resp.status(), &token_id[..8.min(token_id.len())]);
        return None;
    }

    let book: OrderbookResponse = resp.json().await.ok()?;

    if book.bids.is_empty() {
        return None;
    }

    let mut total_value = Decimal::ZERO;
    let mut best_bid = Decimal::ZERO;

    for (i, level) in book.bids.iter().enumerate() {
        let price = Decimal::from_str(&level.price).unwrap_or(Decimal::ZERO);
        let size = Decimal::from_str(&level.size).unwrap_or(Decimal::ZERO);
        let value = price * size;
        total_value += value;

        if i == 0 {
            best_bid = price;
        }
    }

    Some(DepthAnalysis {
        total_bid_value: total_value,
        best_bid,
        bid_levels: book.bids.len(),
    })
}

/// Check if a market passes the momentum filter.
/// Returns (passes, reason) tuple.
pub fn check_momentum_filter(
    yes_bid: Decimal,
    no_bid: Decimal,
    momentum_threshold: Decimal,
) -> (bool, String) {
    // Price deviation from 50/50
    let half = Decimal::from_str("0.50").unwrap();
    let yes_deviation = (yes_bid - half).abs();
    let no_deviation = (no_bid - half).abs();
    let max_deviation = yes_deviation.max(no_deviation);

    if momentum_threshold > Decimal::ZERO && max_deviation < momentum_threshold {
        return (
            false,
            format!(
                "price deviation {:.2} < threshold {:.2} (yes={:.2}, no={:.2})",
                max_deviation, momentum_threshold, yes_bid, no_bid
            ),
        );
    }

    (true, "momentum OK".to_string())
}

/// Check if orderbook depth confirms the momentum signal.
/// The expensive side should have more bid depth.
/// Auto-scales the required ratio based on momentum strength:
///   - Weak momentum (0.55/0.45) → need 2.0x depth
///   - Strong momentum (0.70/0.30) → need 1.2x depth
/// Returns (passes, reason) tuple.
pub fn check_depth_filter_auto(
    yes_depth: Decimal,
    no_depth: Decimal,
    yes_is_expensive: bool,
    expensive_price: Decimal,
) -> (bool, String) {
    if yes_depth == Decimal::ZERO || no_depth == Decimal::ZERO {
        return (false, "missing depth data".to_string());
    }

    // Calculate price deviation from 50/50
    let half = Decimal::from_str("0.50").unwrap();
    let deviation = (expensive_price - half).abs();

    // Auto-scale required ratio: stronger momentum = less depth confirmation needed
    // deviation 0.05 → ratio 2.25, deviation 0.10 → ratio 2.0, deviation 0.20 → ratio 1.5, deviation 0.30 → ratio 1.0
    let base_ratio = Decimal::from_str("2.5").unwrap();
    let scale = Decimal::from(5);
    let min_ratio_raw = base_ratio - (deviation * scale);
    let min_ratio = min_ratio_raw
        .max(Decimal::from_str("1.2").unwrap())
        .min(Decimal::from_str("2.0").unwrap());

    let ratio = if yes_is_expensive {
        yes_depth / no_depth
    } else {
        no_depth / yes_depth
    };

    if ratio < min_ratio {
        return (
            false,
            format!(
                "depth {:.1}x < required {:.1}x (YES=${:.0}, NO=${:.0}, exp={}@{:.0}¢)",
                ratio, min_ratio, yes_depth, no_depth,
                if yes_is_expensive { "YES" } else { "NO" },
                expensive_price * Decimal::from(100)
            ),
        );
    }

    (
        true,
        format!(
            "depth {:.1}x >= {:.1}x (YES=${:.0}, NO=${:.0})",
            ratio, min_ratio, yes_depth, no_depth
        ),
    )
}
