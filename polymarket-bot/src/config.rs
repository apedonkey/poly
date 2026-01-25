//! Configuration management for the Polymarket bot

use anyhow::Result;
use rust_decimal::Decimal;
use std::env;
use std::str::FromStr;

/// Bot configuration loaded from environment
#[derive(Debug, Clone)]
pub struct Config {
    /// Private key for trading (optional, only for live trading)
    pub private_key: Option<String>,

    /// Path to SQLite database
    pub database_path: String,

    /// Polygon RPC URL for balance queries
    pub polygon_rpc_url: String,

    /// Whether running in paper trading mode
    pub paper_trading: bool,

    /// Maximum size per position in USDC
    pub max_position_size: Decimal,

    /// Maximum total exposure in USDC
    pub max_total_exposure: Decimal,

    /// Scan interval in seconds
    pub scan_interval_seconds: u64,

    /// Minimum liquidity for markets to consider
    pub min_liquidity: Decimal,

    /// Sniper strategy settings
    pub sniper: SniperConfig,

    /// NO bias strategy settings
    pub no_bias: NoBiasConfig,
}

#[derive(Debug, Clone)]
pub struct SniperConfig {
    /// Minimum hours until close (default: 1)
    pub min_hours: f64,
    /// Maximum hours until close (default: 12)
    pub max_hours: f64,
    /// Minimum favorite price (default: 0.70)
    pub min_favorite_price: f64,
    /// Maximum favorite price (default: 0.90)
    pub max_favorite_price: f64,
    /// Minimum expected value threshold (default: 0.05)
    pub min_ev: f64,
}

impl Default for SniperConfig {
    fn default() -> Self {
        Self {
            min_hours: 1.0,
            max_hours: 12.0,
            min_favorite_price: 0.70,
            max_favorite_price: 0.90,
            min_ev: 0.05,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NoBiasConfig {
    /// Historical NO resolution rate (78.4%)
    pub historical_no_rate: f64,
    /// Minimum edge required (default: 0.10)
    pub min_edge: f64,
    /// Minimum hours until close to avoid overlap with sniper
    pub min_hours: f64,
    /// Categories to exclude (fairly priced)
    pub excluded_categories: Vec<String>,
}

impl Default for NoBiasConfig {
    fn default() -> Self {
        Self {
            historical_no_rate: 0.784,
            min_edge: 0.10,
            min_hours: 12.0,
            excluded_categories: vec![
                "Sports".to_string(),
                "Crypto".to_string(),
                "NBA".to_string(),
                "NFL".to_string(),
                "MLB".to_string(),
            ],
        }
    }
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        // Load .env file if present
        dotenvy::dotenv().ok();

        let private_key = env::var("POLYMARKET_PRIVATE_KEY").ok().filter(|s| !s.is_empty());

        let database_path = env::var("DATABASE_PATH")
            .unwrap_or_else(|_| "polymarket.db".to_string());

        let paper_trading = env::var("PAPER_TRADING")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true); // Default to paper trading for safety

        let max_position_size = env::var("MAX_POSITION_SIZE")
            .ok()
            .and_then(|v| Decimal::from_str(&v).ok())
            .unwrap_or_else(|| Decimal::from(50));

        let max_total_exposure = env::var("MAX_TOTAL_EXPOSURE")
            .ok()
            .and_then(|v| Decimal::from_str(&v).ok())
            .unwrap_or_else(|| Decimal::from(500));

        let scan_interval_seconds = env::var("SCAN_INTERVAL_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);

        let min_liquidity = env::var("MIN_LIQUIDITY")
            .ok()
            .and_then(|v| Decimal::from_str(&v).ok())
            .unwrap_or_else(|| Decimal::from(1000));

        let polygon_rpc_url = env::var("POLYGON_RPC_URL")
            .unwrap_or_else(|_| "https://polygon-rpc.com".to_string());

        // Validate configuration
        if !paper_trading && private_key.is_none() {
            anyhow::bail!("POLYMARKET_PRIVATE_KEY required for live trading");
        }

        Ok(Self {
            private_key,
            database_path,
            polygon_rpc_url,
            paper_trading,
            max_position_size,
            max_total_exposure,
            scan_interval_seconds,
            min_liquidity,
            sniper: SniperConfig::default(),
            no_bias: NoBiasConfig::default(),
        })
    }

    /// Check if live trading is enabled
    pub fn is_live(&self) -> bool {
        !self.paper_trading && self.private_key.is_some()
    }
}

/// Gamma API configuration
pub struct GammaApi;

impl GammaApi {
    pub const BASE_URL: &'static str = "https://gamma-api.polymarket.com";

    pub fn markets_url() -> String {
        format!("{}/markets", Self::BASE_URL)
    }

    pub fn events_url() -> String {
        format!("{}/events", Self::BASE_URL)
    }
}
