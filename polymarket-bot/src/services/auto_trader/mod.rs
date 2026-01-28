//! Auto-trading service module
//!
//! Provides automated trading functionality:
//! - Position monitoring for take-profit/stop-loss/trailing-stop
//! - Auto-buying opportunities based on configured criteria
//! - Activity logging and statistics

pub mod auto_buyer;
pub mod auto_seller;
pub mod config;
pub mod executor;
pub mod key_store;
pub mod position_monitor;
pub mod types;

pub use auto_buyer::AutoBuyer;
pub use auto_seller::AutoSeller;
pub use config::{AutoTradingSettings, UpdateSettingsRequest};
pub use executor::AutoTradingExecutor;
pub use key_store::KeyStore;
pub use position_monitor::{PositionMonitor, SellSignal};
pub use types::{AutoTradeLog, AutoTradingStats, ExitTrigger, PositionPeak};
