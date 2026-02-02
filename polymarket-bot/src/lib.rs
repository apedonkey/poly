//! Polymarket Trading Bot Library
//!
//! A trading bot for Polymarket prediction markets.
//!
//! **Resolution Sniping**: Bet on heavy favorites in markets about to close.
//! At 4 hours before close, the favorite wins 95.3% of the time.
//! The profit is in the gap between the favorite's price and win rate.

pub mod api;
pub mod config;
pub mod db;
pub mod executor;
pub mod scanner;
pub mod services;
pub mod strategies;
pub mod types;
pub mod wallet;
pub mod webhook;

pub use config::Config;
pub use db::Database;
pub use executor::Executor;
pub use scanner::Scanner;
pub use services::{EndpointClass, McScanner, McStatusUpdate, McScoutResult, Metrics, MintMakerRunner, MintMakerStatusUpdate, OrderEvent, PriceUpdate, PriceUpdateTx, PriceWebSocket, RateLimiter, ResolutionTracker, TickSizeCache, UserWebSocket};
pub use strategies::{AllOpportunities, MintMakerStrategy, SniperStrategy, StrategyRunner};
pub use types::{Opportunity, Side, StrategyType, TrackedMarket};
pub use wallet::{generate_wallet, wallet_from_private_key, encrypt_private_key, decrypt_private_key, GeneratedWallet, EncryptedKey};
pub use webhook::DiscordWebhook;
