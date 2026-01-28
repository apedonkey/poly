//! Web API module for the Polymarket trading bot
//!
//! Provides REST endpoints and WebSocket support for multi-user trading.

pub mod routes;
pub mod server;
pub mod ws;

pub use server::{create_app, AppState, ScanStatus};
