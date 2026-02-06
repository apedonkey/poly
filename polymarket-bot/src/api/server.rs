//! Axum server setup and configuration

use crate::api::routes;
use crate::api::ws::{ws_handler, WalletBalanceUpdate};
use crate::services::{KeyStore, McStatusUpdate, MintMakerStatusUpdate, Metrics, OrderEvent, PriceUpdate, PriceUpdateTx, RateLimiter, TickSizeCache, UserWebSocket};
use crate::types::{DisputeAlert, Opportunity, TrackedMarket};
use crate::{Config, Database, Scanner, StrategyRunner};
use anyhow::Result;
use axum::{
    http::{header, Method},
    routing::{get, post},
    Router,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock, watch};
use tokio::task::JoinHandle;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

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
    /// Broadcast channel for dispute alerts
    pub dispute_tx: broadcast::Sender<Vec<DisputeAlert>>,
    /// Cached dispute alerts
    pub disputes: Arc<RwLock<Vec<DisputeAlert>>>,
    /// Broadcast channel for wallet balance updates
    pub balance_tx: broadcast::Sender<WalletBalanceUpdate>,
    /// Tick size cache for price validation
    pub tick_size_cache: Arc<TickSizeCache>,
    /// Rate limiter for CLOB API calls
    pub rate_limiter: Arc<RateLimiter>,
    /// Broadcast channel for order events from User Channel WebSocket
    pub order_event_tx: broadcast::Sender<OrderEvent>,
    /// Metrics collector
    pub metrics: Metrics,
    /// Broadcast channel for MC status updates
    pub mc_tx: broadcast::Sender<McStatusUpdate>,
    /// Cached MC status for new WS connections
    pub mc_status: Arc<RwLock<Option<McStatusUpdate>>>,
    /// Broadcast channel for feeding raw markets to MC scanner
    pub mc_markets_tx: broadcast::Sender<Vec<TrackedMarket>>,
    /// Active User WebSocket connections: wallet_address -> (shutdown_sender, join_handle)
    /// Used to dynamically spawn/stop per-wallet WebSocket connections
    pub user_ws_handles: Arc<Mutex<HashMap<String, (watch::Sender<bool>, JoinHandle<()>)>>>,
    /// Broadcast channel for Mint Maker status updates
    pub mint_maker_tx: broadcast::Sender<MintMakerStatusUpdate>,
    /// Cached Mint Maker status for new WS connections
    pub mint_maker_status: Arc<RwLock<Option<MintMakerStatusUpdate>>>,
    /// Broadcast channel for feeding raw markets to Mint Maker runner
    pub mint_maker_markets_tx: broadcast::Sender<Vec<TrackedMarket>>,
    /// Token IDs from live mint maker markets (shared with main scanner for price WS subscription)
    pub mm_live_tokens: Arc<RwLock<HashSet<String>>>,
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self> {
        let db = Database::new(&config.database_path).await?;
        let scanner = Scanner::new(config.clone());
        let runner = StrategyRunner::new(&config);

        let (opportunity_tx, _) = broadcast::channel(64); // Higher capacity for large opportunity lists
        let (price_tx, _) = broadcast::channel(256); // Higher capacity for frequent price updates
        let (scan_status_tx, _) = broadcast::channel(16);
        let (dispute_tx, _) = broadcast::channel(32);
        let (balance_tx, _) = broadcast::channel(32);
        let (order_event_tx, _) = broadcast::channel(128);
        let (mc_tx, _) = broadcast::channel(32);
        let (mc_markets_tx, _) = broadcast::channel(16);
        let (mint_maker_tx, _) = broadcast::channel(32);
        let (mint_maker_markets_tx, _) = broadcast::channel(16);

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
            dispute_tx,
            disputes: Arc::new(RwLock::new(Vec::new())),
            balance_tx,
            tick_size_cache: Arc::new(TickSizeCache::new()),
            rate_limiter: Arc::new(RateLimiter::new()),
            order_event_tx,
            metrics: Metrics::new(),
            mc_tx,
            mc_status: Arc::new(RwLock::new(None)),
            mc_markets_tx,
            user_ws_handles: Arc::new(Mutex::new(HashMap::new())),
            mint_maker_tx,
            mint_maker_status: Arc::new(RwLock::new(None)),
            mint_maker_markets_tx,
            mm_live_tokens: Arc::new(RwLock::new(HashSet::new())),
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

    /// Subscribe to dispute alerts
    pub fn subscribe_disputes(&self) -> broadcast::Receiver<Vec<DisputeAlert>> {
        self.dispute_tx.subscribe()
    }

    /// Subscribe to wallet balance updates
    pub fn subscribe_balances(&self) -> broadcast::Receiver<WalletBalanceUpdate> {
        self.balance_tx.subscribe()
    }

    /// Subscribe to order events from user channel WebSocket
    pub fn subscribe_order_events(&self) -> broadcast::Receiver<OrderEvent> {
        self.order_event_tx.subscribe()
    }

    /// Subscribe to MC status updates
    pub fn subscribe_mc(&self) -> broadcast::Receiver<McStatusUpdate> {
        self.mc_tx.subscribe()
    }

    /// Subscribe to Mint Maker status updates
    pub fn subscribe_mint_maker(&self) -> broadcast::Receiver<MintMakerStatusUpdate> {
        self.mint_maker_tx.subscribe()
    }

    /// Spawn a User WebSocket connection for a wallet.
    /// If one already exists for this wallet, it's stopped first.
    pub async fn spawn_user_ws(
        &self,
        wallet_address: String,
        api_key: String,
        api_secret: String,
        api_passphrase: String,
    ) {
        // Stop existing connection if any
        self.stop_user_ws(&wallet_address).await;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let db = self.db.clone();
        let order_event_tx = self.order_event_tx.clone();
        let metrics = self.metrics.clone();
        let addr = wallet_address.clone();

        let handle = tokio::spawn(async move {
            info!("[User WS] Spawning for wallet {}", addr);
            UserWebSocket::run(
                api_key,
                api_secret,
                api_passphrase,
                addr,
                db,
                order_event_tx,
                metrics,
                shutdown_rx,
            )
            .await;
        });

        self.user_ws_handles
            .lock()
            .await
            .insert(wallet_address, (shutdown_tx, handle));
    }

    /// Stop the User WebSocket connection for a wallet (frees resources).
    pub async fn stop_user_ws(&self, wallet_address: &str) {
        let mut handles = self.user_ws_handles.lock().await;
        if let Some((shutdown_tx, handle)) = handles.remove(wallet_address) {
            info!("[User WS] Stopping for wallet {}", wallet_address);
            // Signal shutdown
            let _ = shutdown_tx.send(true);
            // Abort the task if it doesn't stop cleanly
            handle.abort();
        }
    }

    /// Check if a wallet has an active User WebSocket connection.
    pub async fn has_user_ws(&self, wallet_address: &str) -> bool {
        self.user_ws_handles.lock().await.contains_key(wallet_address)
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
        .route("/wallet/export-key", post(routes::wallet::export_private_key))
        .route("/wallet/disconnect", post(routes::wallet::disconnect_wallet))
        .route("/wallet/deposit", post(routes::wallet::deposit_to_safe))
        .route("/wallet/withdraw", post(routes::wallet::withdraw_from_safe))
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
        .route("/auto-trading/status", get(routes::auto_trading::get_status))
        // Market data routes
        .route("/market/prices", get(routes::market_data::get_price_history))
        .route("/market/tick-size", get(routes::market_data::get_tick_size))
        .route("/metrics", get(routes::market_data::get_metrics))
        // Millionaires Club routes
        .route("/mc/status", get(routes::mc::get_status))
        .route("/mc/scout-log", get(routes::mc::get_scout_log))
        .route("/mc/trades", get(routes::mc::get_trades))
        .route("/mc/tier-history", get(routes::mc::get_tier_history))
        .route("/mc/config", axum::routing::put(routes::mc::update_config))
        // Mint Maker routes
        .route("/mint-maker/settings", get(routes::mint_maker::get_settings))
        .route("/mint-maker/settings", axum::routing::put(routes::mint_maker::update_settings))
        .route("/mint-maker/enable", post(routes::mint_maker::enable))
        .route("/mint-maker/disable", post(routes::mint_maker::disable))
        .route("/mint-maker/pairs", get(routes::mint_maker::get_pairs))
        .route("/mint-maker/stats", get(routes::mint_maker::get_stats))
        .route("/mint-maker/analytics", get(routes::mint_maker::get_analytics))
        .route("/mint-maker/log", get(routes::mint_maker::get_log))
        .route("/mint-maker/place", post(routes::mint_maker::place_pair))
        .route("/mint-maker/cancel-pair", post(routes::mint_maker::cancel_pair))
        // Open orders routes
        .route("/orders", get(routes::orders::get_open_orders))
        .route("/orders/lifecycle", get(routes::orders::get_order_lifecycle))
        .route("/orders/:id", axum::routing::delete(routes::orders::cancel_order))
        .route("/orders/cancel-all", axum::routing::delete(routes::orders::cancel_all_orders))
        .route("/orders/market/:id", axum::routing::delete(routes::orders::cancel_market_orders));

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
