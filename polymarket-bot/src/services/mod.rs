//! Background services for the trading bot

pub mod auto_trader;
pub mod clarification_monitor;
pub mod dispute_tracker;
pub mod price_ws;
pub mod resolution_tracker;

pub use auto_trader::{
    AutoBuyer, AutoSeller, AutoTradeLog, AutoTradingExecutor, AutoTradingSettings,
    AutoTradingStats, ExitTrigger, KeyStore, PositionMonitor, SellSignal,
};
pub use clarification_monitor::ClarificationMonitor;
pub use dispute_tracker::DisputeTracker;
pub use price_ws::{PriceUpdate, PriceUpdateTx, PriceWebSocket};
pub use resolution_tracker::ResolutionTracker;
