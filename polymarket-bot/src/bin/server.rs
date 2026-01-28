//! Polymarket Trading Bot Web Server
//!
//! Multi-user web interface for the Polymarket trading bot.

use anyhow::Result;
use chrono::Utc;
use polymarket_bot::api::{create_app, AppState, ScanStatus};
use polymarket_bot::services::{AutoBuyer, AutoSeller, ClarificationMonitor, DisputeTracker, PositionMonitor, PriceWebSocket};
use polymarket_bot::{Config, ResolutionTracker};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .init();

    // Load configuration
    let config = Config::from_env()?;

    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║       POLYMARKET TRADING BOT - WEB SERVER                     ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Paper Trading: {:<44} ║", if config.paper_trading { "YES (safe mode)" } else { "NO - LIVE MODE" });
    println!("║  Discord Webhook: {:<42} ║", if config.discord_webhook_url.is_some() { "ENABLED" } else { "DISABLED" });
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Create application state
    info!("Initializing application state...");
    let state = AppState::new(config.clone()).await?;

    // Channel for sending token IDs to price WebSocket
    let (token_tx, token_rx) = mpsc::channel::<Vec<String>>(16);

    // Spawn price WebSocket task for real-time price updates
    let ws_opportunities = state.opportunities.clone();
    let ws_opportunity_tx = state.opportunity_tx.clone();
    let ws_price_tx = state.price_tx.clone();
    tokio::spawn(async move {
        info!("Starting real-time price WebSocket...");
        PriceWebSocket::run(token_rx, ws_opportunities, ws_opportunity_tx, ws_price_tx).await;
    });

    // ==================== AUTO-TRADING SERVICES ====================

    // Channel for sell signals from position monitor to auto-seller
    let (sell_tx, sell_rx) = mpsc::channel(64);

    // Spawn Position Monitor (monitors prices and generates sell signals)
    let monitor_db = state.db.clone();
    let monitor_price_rx = state.price_tx.subscribe();
    tokio::spawn(async move {
        info!("Starting position monitor for auto-sell triggers...");
        let monitor = PositionMonitor::new(monitor_db);
        if let Err(e) = monitor.load_peaks().await {
            tracing::warn!("Failed to load position peaks: {}", e);
        }
        monitor.run(monitor_price_rx, sell_tx).await;
    });

    // Spawn Auto-Seller (executes sell orders from position monitor)
    let seller_db = state.db.clone();
    let seller_key_store = state.key_store.clone();
    tokio::spawn(async move {
        info!("Starting auto-seller service...");
        let seller = AutoSeller::new(seller_db, seller_key_store);
        seller.run(sell_rx).await;
    });

    // Spawn Auto-Buyer (automatically buys matching opportunities)
    let buyer_db = state.db.clone();
    let buyer_key_store = state.key_store.clone();
    let buyer_opportunities = state.opportunities.clone();
    let buyer_opp_rx = state.opportunity_tx.subscribe();
    tokio::spawn(async move {
        info!("Starting auto-buyer service...");
        let buyer = AutoBuyer::new(buyer_db, buyer_key_store, buyer_opportunities);
        buyer.run(buyer_opp_rx).await;
    });

    // Clone state for background scanner
    let scanner_state = state.clone();

    // Spawn background scanner task (also handles resolution tracking)
    // Discord alerts are now sent via the frontend calling /api/discord/alerts
    tokio::spawn(async move {
        info!("Starting background scanner with resolution tracking...");
        run_scanner(scanner_state, token_tx).await;
    });

    // ==================== CLARIFICATION & DISPUTE MONITORING ====================

    // Spawn Clarification Monitor (detects market description changes)
    let clarification_db = state.db.clone();
    let clarification_tx = state.clarification_tx.clone();
    tokio::spawn(async move {
        info!("Starting clarification monitor...");
        let monitor = ClarificationMonitor::new(clarification_db);
        monitor.run(Duration::from_secs(60), clarification_tx).await;
    });

    // Spawn Dispute Tracker (monitors UMA oracle disputes)
    let dispute_db = state.db.clone();
    let dispute_tx = state.dispute_tx.clone();
    tokio::spawn(async move {
        info!("Starting UMA dispute tracker...");
        let tracker = DisputeTracker::new(dispute_db);
        tracker.run(Duration::from_secs(60), dispute_tx).await;
    });

    // Create the Axum app
    let app = create_app(state);

    // Bind to address
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = TcpListener::bind(addr).await?;

    info!("Server listening on http://{}", addr);
    println!();
    println!("  API:       http://localhost:3000/api");
    println!("  WebSocket: ws://localhost:3000/ws");
    println!("  Health:    http://localhost:3000/health");
    println!();

    // Run the server
    axum::serve(listener, app).await?;

    Ok(())
}

/// Background scanner that periodically fetches opportunities and checks resolutions
async fn run_scanner(state: AppState, token_tx: mpsc::Sender<Vec<String>>) {
    let scan_interval = state.config.scan_interval_seconds;
    let resolution_tracker = ResolutionTracker::new(state.db.clone());

    loop {
        // Check for resolved positions (real-time with each scan)
        if let Err(e) = resolution_tracker.check_resolutions().await {
            tracing::warn!("Resolution check failed: {}", e);
        }

        // Scan for new opportunities
        match state.scanner.fetch_markets().await {
            Ok(markets) => {
                let all_opps = state.runner.find_all_opportunities(&markets);

                // Keep a copy of sniper opportunities for token extraction
                let sniper_opps = all_opps.sniper.clone();

                // Combine sniper and NO bias opportunities
                let mut combined = all_opps.sniper;
                combined.extend(all_opps.no_bias);

                // Sort by edge descending
                combined.sort_by(|a, b| {
                    b.edge.partial_cmp(&a.edge).unwrap_or(std::cmp::Ordering::Equal)
                });

                let count = combined.len();

                // Update cached opportunities
                *state.opportunities.write().await = combined.clone();

                // Broadcast to all WebSocket clients
                let _ = state.opportunity_tx.send(combined);

                // Collect token IDs for real-time price subscriptions
                let mut all_tokens: HashSet<String> = HashSet::new();

                // 1. Sniper opportunity tokens (time-sensitive)
                for opp in &sniper_opps {
                    if let Some(token) = &opp.token_id {
                        all_tokens.insert(token.clone());
                    }
                }

                // 2. Open position tokens (user's active holdings)
                if let Ok(position_tokens) = state.db.get_all_open_position_token_ids().await {
                    all_tokens.extend(position_tokens);
                }

                // Send combined, deduplicated tokens to price WebSocket
                let token_count = all_tokens.len();
                let _ = token_tx.send(all_tokens.into_iter().collect()).await;

                info!(
                    "Scan complete: {} markets, {} opportunities, {} tokens for live prices",
                    markets.len(),
                    count,
                    token_count
                );

                // Update and broadcast scan status for frontend progress bar
                let now = Utc::now().timestamp_millis();
                *state.last_scan_at.write().await = now;
                let _ = state.scan_status_tx.send(ScanStatus {
                    scan_interval_seconds: scan_interval,
                    last_scan_at: now,
                });
            }
            Err(e) => {
                tracing::error!("Scan failed: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(scan_interval)).await;
    }
}
