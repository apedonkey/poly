//! Polymarket Trading Bot Web Server
//!
//! Multi-user web interface for the Polymarket trading bot.

use anyhow::Result;
use polymarket_bot::api::{create_app, AppState};
use polymarket_bot::Config;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
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
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // Create application state
    info!("Initializing application state...");
    let state = AppState::new(config.clone()).await?;

    // Clone state for background scanner
    let scanner_state = state.clone();

    // Spawn background scanner task
    tokio::spawn(async move {
        info!("Starting background scanner...");
        run_scanner(scanner_state).await;
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

/// Background scanner that periodically fetches opportunities
async fn run_scanner(state: AppState) {
    let scan_interval = state.config.scan_interval_seconds;

    loop {
        match state.scanner.fetch_markets().await {
            Ok(markets) => {
                let all_opps = state.runner.find_all_opportunities(&markets);

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

                info!(
                    "Scan complete: {} markets, {} opportunities",
                    markets.len(),
                    count
                );
            }
            Err(e) => {
                tracing::error!("Scan failed: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(scan_interval)).await;
    }
}
