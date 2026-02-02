//! Background services for the trading bot

pub mod auto_trader;
pub mod clob_errors;
pub mod ctf;
pub mod dispute_tracker;
pub mod mc_scanner;
pub mod mint_maker;
pub mod price_ws;
pub mod rate_limiter;
pub mod resolution_tracker;
pub mod metrics;
pub mod retry;
pub mod safe_activation;
pub mod safe_proxy;
pub mod tick_size;
pub mod user_ws;


pub use auto_trader::{
    AutoBuyer, AutoSeller, AutoTradeLog, AutoTradingExecutor, AutoTradingSettings,
    AutoTradingStats, DisputeSniper, ExitTrigger, KeyStore, PositionMonitor, SellSignal,
};
pub use dispute_tracker::DisputeTracker;
pub use mc_scanner::{McScanner, McStatusUpdate, McScoutResult};
pub use price_ws::{PriceUpdate, PriceUpdateTx, PriceWebSocket};
pub use clob_errors::ClobError;
pub use rate_limiter::{EndpointClass, RateLimiter};
pub use resolution_tracker::ResolutionTracker;
pub use retry::{RetryConfig, with_retry};
pub use safe_proxy::derive_safe_wallet;
pub use tick_size::TickSizeCache;
pub use ctf::CtfService;
pub use metrics::Metrics;
pub use mint_maker::{MintMakerRunner, MintMakerStatusUpdate};
pub use user_ws::{OrderEvent, UserWebSocket};
