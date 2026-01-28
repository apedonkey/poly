//! Axum server setup and configuration

use crate::api::routes;
use crate::api::ws::ws_handler;
use crate::services::{KeyStore, PriceUpdate, PriceUpdateTx};
use crate::types::{ClarificationAlert, DisputeAlert, Opportunity};
use crate::{Config, Database, Scanner, StrategyRunner};
use anyhow::Result;
use axum::{
    http::{header, Method},
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Scan status info for frontend progress bar
#[derive(Debug, Clone)]
pub struct ScanStatus {
    pub scan_interval_seconds: u64,
    pub last_scan_at: i64, // Unix timestamp ms
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub config: Arc<Config>,
    pub scanner: Arc<Scanner>,
    pub runner: Arc<StrategyRunner>,
    /// Cached opportunities from last scan
    pub opportunities: Arc<RwLock<Vec<Opportunity>>>,
    /// Broadcast channel for opportunity updates
    pub opportunity_tx: broadcast::Sender<Vec<Opportunity>>,
    /// Broadcast channel for real-time price updates
    pub price_tx: PriceUpdateTx,
    /// Broadcast channel for scan status updates
    pub scan_status_tx: broadcast::Sender<ScanStatus>,
    /// Timestamp of last completed scan (ms)
    pub last_scan_at: Arc<RwLock<i64>>,
    /// In-memory key store for auto-trading (decrypted private keys)
    pub key_store: KeyStore,
    /// Broadcast channel for clarification alerts
    pub clarification_tx: broadcast::Sender<Vec<ClarificationAlert>>,
    /// Broadcast channel for dispute alerts
    pub dispute_tx: broadcast::Sender<Vec<DisputeAlert>>,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self> {
        let db = Database::new(&config.database_path).await?;
        let scanner = Scanner::new(config.clone());
        let runner = StrategyRunner::new(&config);

        let (opportunity_tx, _) = broadcast::channel(64); // Higher capacity for large opportunity lists
        let (price_tx, _) = broadcast::channel(256); // Higher capacity for frequent price updates
        let (scan_status_tx, _) = broadcast::channel(16);
        let (clarification_tx, _) = broadcast::channel(32);
        let (dispute_tx, _) = broadcast::channel(32);

        Ok(Self {
            db: Arc::new(db),
            config: Arc::new(config),
            scanner: Arc::new(scanner),
            runner: Arc::new(runner),
            opportunities: Arc::new(RwLock::new(Vec::new())),
            opportunity_tx,
            price_tx,
            scan_status_tx,
            last_scan_at: Arc::new(RwLock::new(0)),
            key_store: KeyStore::new(),
            clarification_tx,
            dispute_tx,
        })
    }

    /// Subscribe to opportunity updates
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<Opportunity>> {
        self.opportunity_tx.subscribe()
    }

    /// Subscribe to real-time price updates
    pub fn subscribe_prices(&self) -> broadcast::Receiver<PriceUpdate> {
        self.price_tx.subscribe()
    }

    /// Subscribe to scan status updates
    pub fn subscribe_scan_status(&self) -> broadcast::Receiver<ScanStatus> {
        self.scan_status_tx.subscribe()
    }

    /// Subscribe to clarification alerts
    pub fn subscribe_clarifications(&self) -> broadcast::Receiver<Vec<ClarificationAlert>> {
        self.clarification_tx.subscribe()
    }

    /// Subscribe to dispute alerts
    pub fn subscribe_disputes(&self) -> broadcast::Receiver<Vec<DisputeAlert>> {
        self.dispute_tx.subscribe()
    }
}

/// Create the Axum application with all routes
pub fn create_app(state: AppState) -> Router {
    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    // API routes
    let api_routes = Router::new()
        // Wallet routes
        .route("/wallet/generate", post(routes::wallet::generate_wallet))
        .route("/wallet/import", post(routes::wallet::import_wallet))
        .route("/wallet/unlock", post(routes::wallet::unlock_wallet))
        .route("/wallet/connect", post(routes::wallet::connect_external_wallet))
        .route("/wallet/balance", get(routes::wallet::get_balance))
        // Opportunity routes
        .route("/opportunities", get(routes::opportunities::list_opportunities))
        // Position routes
        .route("/positions", get(routes::positions::list_positions))
        .route("/positions/stats", get(routes::positions::get_stats))
        .route("/positions/:id/close", post(routes::positions::close_position))
        .route("/positions/:id/redeem", post(routes::positions::redeem_position))
        .route("/positions/:id/token", post(routes::positions::update_token_id))
        .route("/positions/:id/entry-price", post(routes::positions::update_entry_price))
        // Trade routes
        .route("/trades/execute", post(routes::trades::execute_trade))
        .route("/trades/signed", post(routes::trades::execute_signed_trade))
        .route("/trades/record", post(routes::trades::record_position))
        .route("/trades/submit-order", post(routes::trades::submit_sdk_order))
        .route("/trades/enable", post(routes::trades::enable_trading))
        // Builder relay routes
        .route("/builder/sign", post(routes::builder::sign_builder_request))
        .route("/builder/relay", post(routes::builder::relay_proxy))
        // Relay proxy routes (for SDK to use)
        .route("/relay/submit", post(routes::builder::relay_submit))
        .route("/relay/nonce", get(routes::builder::relay_nonce))
        .route("/relay/relay", get(routes::builder::relay_address))
        .route("/relay/deployed", get(routes::builder::relay_deployed))
        .route("/relay/transaction", get(routes::builder::relay_transaction))
        // CLOB authentication routes
        .route("/auth/time", get(routes::clob_auth::get_server_time))
        .route("/auth/derive-api-key", post(routes::clob_auth::derive_api_key))
        // Discord webhook routes
        .route("/discord/alerts", post(routes::discord::send_alerts))
        // Auto-trading routes
        .route("/auto-trading/settings", get(routes::auto_trading::get_settings))
        .route("/auto-trading/settings", axum::routing::put(routes::auto_trading::update_settings))
        .route("/auto-trading/enable", post(routes::auto_trading::enable))
        .route("/auto-trading/disable", post(routes::auto_trading::disable))
        .route("/auto-trading/history", get(routes::auto_trading::get_history))
        .route("/auto-trading/stats", get(routes::auto_trading::get_stats))
        .route("/auto-trading/status", get(routes::auto_trading::get_status));

    Router::new()
        .nest("/api", api_routes)
        .route("/ws", get(ws_handler))
        .route("/health", get(health_check))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Health check endpoint
async fn health_check() -> &'static str {
    "OK"
}
