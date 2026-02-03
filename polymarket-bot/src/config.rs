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

    /// Mint Maker strategy settings
    pub mint_maker: MintMakerConfig,

    /// Discord webhook URL for sniper alerts (optional)
    pub discord_webhook_url: Option<String>,

    /// Polymarket Builder credentials (for relay service)
    pub builder_api_key: Option<String>,
    pub builder_secret: Option<String>,
    pub builder_passphrase: Option<String>,

    /// Taker fee in basis points (default: 200 = 2%)
    pub taker_fee_bps: u32,

    /// Slippage tolerance for market orders (default: 0.005 = 0.5%)
    pub slippage_tolerance: f64,
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

/// Mint Maker configuration (defaults = Balanced preset)
#[derive(Debug, Clone)]
pub struct MintMakerConfig {
    /// Maximum combined cost of YES+NO pair (must be < 1.00 for profit)
    pub max_pair_cost: f64,
    /// Minimum spread profit per pair in dollars
    pub min_spread_profit: f64,
    /// Offset in cents below mid-price for bids
    pub bid_offset_cents: u32,
    /// Max open pairs per market
    pub max_pairs_per_market: u32,
    /// Max total open pairs across all markets
    pub max_total_pairs: u32,
    /// Seconds before an unfilled order is considered stale
    pub stale_order_seconds: u64,
    /// Crypto assets to trade
    pub assets: Vec<String>,
    /// How often to rebalance/check (seconds)
    pub rebalance_interval_seconds: u64,
    /// Minimum minutes to market close for eligibility
    pub min_minutes_to_close: f64,
    /// Maximum minutes to market close for eligibility
    pub max_minutes_to_close: f64,
}

impl Default for MintMakerConfig {
    fn default() -> Self {
        Self {
            max_pair_cost: 0.98,
            min_spread_profit: 0.01,
            bid_offset_cents: 2,
            max_pairs_per_market: 5,
            max_total_pairs: 20,
            stale_order_seconds: 120,
            assets: vec!["BTC".to_string(), "ETH".to_string(), "SOL".to_string(), "XRP".to_string()],
            rebalance_interval_seconds: 3,
            min_minutes_to_close: 2.0,
            max_minutes_to_close: 14.0,
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
            .unwrap_or(15);

        let min_liquidity = env::var("MIN_LIQUIDITY")
            .ok()
            .and_then(|v| Decimal::from_str(&v).ok())
            .unwrap_or_else(|| Decimal::from(1000));

        let polygon_rpc_url = env::var("POLYGON_RPC_URL")
            .unwrap_or_else(|_| "https://polygon-rpc.com".to_string());

        let discord_webhook_url = env::var("DISCORD_WEBHOOK_URL").ok().filter(|s| !s.is_empty());

        // Builder credentials for relay service
        let builder_api_key = env::var("POLY_BUILDER_API_KEY").ok().filter(|s| !s.is_empty());
        let builder_secret = env::var("POLY_BUILDER_SECRET").ok().filter(|s| !s.is_empty());
        let builder_passphrase = env::var("POLY_BUILDER_PASSPHRASE").ok().filter(|s| !s.is_empty());

        // Trading parameters
        let taker_fee_bps = env::var("TAKER_FEE_BPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200); // Default 2%

        let slippage_tolerance = env::var("SLIPPAGE_TOLERANCE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.005); // Default 0.5%

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
            mint_maker: MintMakerConfig::default(),
            discord_webhook_url,
            builder_api_key,
            builder_secret,
            builder_passphrase,
            taker_fee_bps,
            slippage_tolerance,
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
