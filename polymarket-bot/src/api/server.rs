//! Axum server setup and configuration

use crate::api::routes;
use crate::api::ws::ws_handler;
use crate::{Config, Database, Scanner, StrategyRunner};
use crate::types::Opportunity;
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
}

impl AppState {
    pub async fn new(config: Config) -> Result<Self> {
        let db = Database::new(&config.database_path).await?;
        let scanner = Scanner::new(config.clone());
        let runner = StrategyRunner::new(&config);

        let (opportunity_tx, _) = broadcast::channel(16);

        Ok(Self {
            db: Arc::new(db),
            config: Arc::new(config),
            scanner: Arc::new(scanner),
            runner: Arc::new(runner),
            opportunities: Arc::new(RwLock::new(Vec::new())),
            opportunity_tx,
        })
    }

    /// Subscribe to opportunity updates
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<Opportunity>> {
        self.opportunity_tx.subscribe()
    }
}

/// Create the Axum application with all routes
pub fn create_app(state: AppState) -> Router {
    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
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
        // Trade routes
        .route("/trades/execute", post(routes::trades::execute_trade))
        .route("/trades/paper", post(routes::trades::paper_trade))
        .route("/trades/signed", post(routes::trades::execute_signed_trade))
        .route("/trades/record", post(routes::trades::record_position))
        // CLOB authentication routes
        .route("/auth/time", get(routes::clob_auth::get_server_time))
        .route("/auth/derive-api-key", post(routes::clob_auth::derive_api_key));

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
