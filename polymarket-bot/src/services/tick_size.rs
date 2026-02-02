//! Tick Size Cache - fetches and caches tick size info from the CLOB API
//!
//! Polymarket uses two tick size regimes:
//! - 0.01 for prices in [0.04, 0.96]
//! - 0.001 for prices outside that range (< 0.04 or > 0.96)
//!
//! The CLOB endpoint `GET /tick-size?token_id=X` returns the current tick size.
//! The CLOB endpoint `GET /markets/{condition_id}` returns minimum_order_size.

use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

const CLOB_ENDPOINT: &str = "https://clob.polymarket.com";

/// Default tick size (most tokens are in the 0.04-0.96 range)
const DEFAULT_TICK_SIZE: &str = "0.01";

/// Default minimum order size (shares) — conservative fallback
const DEFAULT_MIN_ORDER_SIZE: &str = "5";

/// Tick size information for a token
#[derive(Debug, Clone)]
pub struct TickSizeInfo {
    pub tick_size: Decimal,
    pub minimum_order_size: Decimal,
}

impl Default for TickSizeInfo {
    fn default() -> Self {
        Self {
            tick_size: Decimal::from_str(DEFAULT_TICK_SIZE).unwrap(),
            minimum_order_size: Decimal::from_str(DEFAULT_MIN_ORDER_SIZE).unwrap(),
        }
    }
}

/// CLOB tick-size API response — fields are numbers, not strings
#[derive(Debug, Deserialize)]
struct TickSizeResponse {
    #[serde(default)]
    minimum_tick_size: Option<f64>,
    #[serde(default)]
    minimum_order_size: Option<f64>,
}

/// CLOB market API response — only the fields we need
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MarketResponse {
    #[serde(default)]
    minimum_order_size: Option<f64>,
    #[serde(default)]
    minimum_tick_size: Option<f64>,
}

/// Cache for tick size lookups
pub struct TickSizeCache {
    /// Keyed by token_id
    cache: Arc<RwLock<HashMap<String, TickSizeInfo>>>,
    /// Keyed by condition_id → minimum_order_size (shared across both tokens in a market)
    market_cache: Arc<RwLock<HashMap<String, Decimal>>>,
    client: reqwest::Client,
}

impl TickSizeCache {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            market_cache: Arc::new(RwLock::new(HashMap::new())),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Get tick size for a token, fetching from API if not cached
    pub async fn get_tick_size(&self, token_id: &str) -> TickSizeInfo {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(info) = cache.get(token_id) {
                return info.clone();
            }
        }

        // Fetch from API
        match self.fetch_tick_size(token_id).await {
            Ok(info) => {
                let mut cache = self.cache.write().await;
                cache.insert(token_id.to_string(), info.clone());
                info
            }
            Err(e) => {
                warn!("Failed to fetch tick size for {}: {}. Using default.", token_id, e);
                TickSizeInfo::default()
            }
        }
    }

    /// Get the per-market minimum order size (in shares) by condition_id.
    /// This fetches from /markets/{condition_id} which always has minimum_order_size.
    pub async fn get_min_order_size(&self, condition_id: &str) -> Decimal {
        // Check cache first
        {
            let cache = self.market_cache.read().await;
            if let Some(min) = cache.get(condition_id) {
                return *min;
            }
        }

        // Fetch from market endpoint
        match self.fetch_market_min_order_size(condition_id).await {
            Ok(min) => {
                info!("Market {} minimum_order_size: {}", condition_id, min);
                let mut cache = self.market_cache.write().await;
                cache.insert(condition_id.to_string(), min);
                min
            }
            Err(e) => {
                warn!("Failed to fetch min order size for market {}: {}. Using default {}.", condition_id, e, DEFAULT_MIN_ORDER_SIZE);
                Decimal::from_str(DEFAULT_MIN_ORDER_SIZE).unwrap()
            }
        }
    }

    /// Round a price to the nearest valid tick boundary
    pub fn round_to_tick(price: Decimal, tick_size: Decimal) -> Decimal {
        if tick_size.is_zero() {
            return price;
        }
        // Round to nearest tick: floor(price / tick) * tick
        let ticks = price / tick_size;
        let rounded_ticks = ticks.floor();
        rounded_ticks * tick_size
    }

    /// Validate that a price is on a valid tick boundary
    pub fn validate_price(price: Decimal, tick_size: Decimal) -> bool {
        if tick_size.is_zero() {
            return true;
        }
        let remainder = price % tick_size;
        remainder.is_zero()
    }

    /// Update cached tick size for a token (e.g., from WebSocket tick_size_change event)
    pub async fn update_tick_size(&self, token_id: &str, tick_size: Decimal) {
        let mut cache = self.cache.write().await;
        let entry = cache.entry(token_id.to_string()).or_insert_with(TickSizeInfo::default);
        entry.tick_size = tick_size;
    }

    /// Fetch tick size from the CLOB /tick-size endpoint
    async fn fetch_tick_size(&self, token_id: &str) -> Result<TickSizeInfo> {
        let url = format!("{}/tick-size?token_id={}", CLOB_ENDPOINT, token_id);

        let response = self.client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch tick size")?;

        if !response.status().is_success() {
            debug!("Tick size API returned {}, using default", response.status());
            return Ok(TickSizeInfo::default());
        }

        let body: TickSizeResponse = response
            .json()
            .await
            .context("Failed to parse tick size response")?;

        let tick_size = body
            .minimum_tick_size
            .and_then(|v| Decimal::try_from(v).ok())
            .unwrap_or_else(|| Decimal::from_str(DEFAULT_TICK_SIZE).unwrap());

        // The tick-size endpoint may or may not include minimum_order_size
        let minimum_order_size = body
            .minimum_order_size
            .and_then(|v| Decimal::try_from(v).ok())
            .unwrap_or_else(|| Decimal::from_str(DEFAULT_MIN_ORDER_SIZE).unwrap());

        Ok(TickSizeInfo {
            tick_size,
            minimum_order_size,
        })
    }

    /// Fetch minimum_order_size from the CLOB /markets/{condition_id} endpoint
    async fn fetch_market_min_order_size(&self, condition_id: &str) -> Result<Decimal> {
        let url = format!("{}/markets/{}", CLOB_ENDPOINT, condition_id);

        let response = self.client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch market info")?;

        if !response.status().is_success() {
            anyhow::bail!("Market API returned {}", response.status());
        }

        let body: MarketResponse = response
            .json()
            .await
            .context("Failed to parse market response")?;

        let min_order_size = body
            .minimum_order_size
            .and_then(|v| Decimal::try_from(v).ok())
            .unwrap_or_else(|| Decimal::from_str(DEFAULT_MIN_ORDER_SIZE).unwrap());

        Ok(min_order_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_to_tick() {
        let tick = Decimal::from_str("0.01").unwrap();
        let price = Decimal::from_str("0.456").unwrap();
        let rounded = TickSizeCache::round_to_tick(price, tick);
        assert_eq!(rounded, Decimal::from_str("0.45").unwrap());
    }

    #[test]
    fn test_round_to_tick_small() {
        let tick = Decimal::from_str("0.001").unwrap();
        let price = Decimal::from_str("0.9876").unwrap();
        let rounded = TickSizeCache::round_to_tick(price, tick);
        assert_eq!(rounded, Decimal::from_str("0.987").unwrap());
    }

    #[test]
    fn test_validate_price_valid() {
        let tick = Decimal::from_str("0.01").unwrap();
        let price = Decimal::from_str("0.45").unwrap();
        assert!(TickSizeCache::validate_price(price, tick));
    }

    #[test]
    fn test_validate_price_invalid() {
        let tick = Decimal::from_str("0.01").unwrap();
        let price = Decimal::from_str("0.456").unwrap();
        assert!(!TickSizeCache::validate_price(price, tick));
    }
}
