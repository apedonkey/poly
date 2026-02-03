//! Polymarket Trading Bot Web Server
//!
//! Multi-user web interface for the Polymarket trading bot.

use anyhow::Result;
use chrono::Utc;
use polymarket_bot::api::{create_app, AppState, ScanStatus, WalletBalanceUpdate};
use polymarket_bot::services::{AutoBuyer, AutoSeller, DisputeSniper, DisputeTracker, McScanner, MintMakerRunner, PositionMonitor, PriceWebSocket};
use polymarket_bot::{Config, ResolutionTracker};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging — default to warn, show info only for trading-related modules.
    // Override with RUST_LOG env var for full debugging, e.g. RUST_LOG=info
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(
            "warn,polymarket_bot::services::mint_maker::runner=info,polymarket_bot::services::mint_maker::order_manager=info,polymarket_bot::services::mint_maker::scanner=info"
        )
    });
    tracing_subscriber::fmt()
        .with_env_filter(filter)
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
    let ws_tick_cache = state.tick_size_cache.clone();
    let ws_metrics = state.metrics.clone();
    tokio::spawn(async move {
        info!("Starting real-time price WebSocket...");
        PriceWebSocket::run(token_rx, ws_opportunities, ws_opportunity_tx, ws_price_tx, ws_tick_cache, ws_metrics).await;
    });

    // ==================== AUTO-TRADING SERVICES ====================

    // Channel for sell signals from position monitor to auto-seller
    let (sell_tx, sell_rx) = mpsc::channel(64);

    // Clone sell_tx for dispute sniper before it's moved into PositionMonitor
    let sniper_sell_tx = sell_tx.clone();

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
    let buyer_rpc_url = config.polygon_rpc_url.clone();
    let buyer_slippage = config.slippage_tolerance;
    tokio::spawn(async move {
        info!("Starting auto-buyer service...");
        let buyer = AutoBuyer::new(buyer_db, buyer_key_store, buyer_opportunities, buyer_rpc_url, buyer_slippage);
        buyer.run(buyer_opp_rx).await;
    });

    // Spawn Dispute Sniper (auto-buys proposed dispute outcomes with edge)
    let sniper_db = state.db.clone();
    let sniper_key_store = state.key_store.clone();
    let sniper_dispute_rx = state.dispute_tx.subscribe();
    let sniper_rpc_url = config.polygon_rpc_url.clone();
    tokio::spawn(async move {
        info!("Starting dispute auto-sniper...");
        let sniper = DisputeSniper::new(sniper_db, sniper_key_store, sniper_rpc_url);
        sniper.run(sniper_dispute_rx, sniper_sell_tx).await;
    });

    // Clone state for background scanner
    let scanner_state = state.clone();

    // Spawn background scanner task (also handles resolution tracking)
    // Discord alerts are now sent via the frontend calling /api/discord/alerts
    tokio::spawn(async move {
        info!("Starting background scanner with resolution tracking...");
        run_scanner(scanner_state, token_tx).await;
    });

    // ==================== DISPUTE MONITORING ====================

    // Spawn Dispute Tracker (monitors UMA oracle disputes)
    let dispute_db = state.db.clone();
    let dispute_tx = state.dispute_tx.clone();
    let dispute_cache = state.disputes.clone();
    tokio::spawn(async move {
        info!("Starting UMA dispute tracker...");
        let mut rx = dispute_tx.subscribe();
        let tracker = DisputeTracker::new(dispute_db);

        // Spawn cache updater
        let cache = dispute_cache.clone();
        tokio::spawn(async move {
            while let Ok(alerts) = rx.recv().await {
                *cache.write().await = alerts;
            }
        });

        tracker.run(Duration::from_secs(60), dispute_tx).await;
    });

    // ==================== MILLIONAIRES CLUB SCANNER ====================

    let mc_db = state.db.clone();
    let mc_tx = state.mc_tx.clone();
    let mc_status_cache = state.mc_status.clone();
    let mc_disputes = state.disputes.clone();
    let mc_markets_rx = state.mc_markets_tx.subscribe();
    tokio::spawn(async move {
        // Spawn cache updater
        let cache = mc_status_cache.clone();
        let mut cache_rx = mc_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(status) = cache_rx.recv().await {
                *cache.write().await = Some(status);
            }
        });

        let mut scanner = McScanner::new(mc_db).await;
        scanner.run(mc_markets_rx, mc_disputes, mc_tx).await;
    });

    // ==================== MINT MAKER SERVICE ====================

    let mm_db = state.db.clone();
    let mm_key_store = state.key_store.clone();
    let mm_config = config.mint_maker.clone();
    let mm_tx = state.mint_maker_tx.clone();
    let mm_status_cache = state.mint_maker_status.clone();
    let mm_client = reqwest::Client::new();
    let mm_tick_size_cache = state.tick_size_cache.clone();
    let mm_price_tx = state.price_tx.clone();
    let mm_live_tokens = state.mm_live_tokens.clone();
    tokio::spawn(async move {
        // Spawn cache updater
        let cache = mm_status_cache.clone();
        let mut cache_rx = mm_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(status) = cache_rx.recv().await {
                *cache.write().await = Some(status);
            }
        });

        info!("Starting Mint Maker runner (dedicated scanner)...");
        let runner = MintMakerRunner::new(mm_db, mm_key_store, mm_config, mm_client, mm_tick_size_cache, mm_price_tx, mm_live_tokens);
        runner.run(mm_tx).await;
    });

    // ==================== USER CHANNEL WEBSOCKET ====================

    // Spawn User WebSocket connections for wallets with existing API credentials
    // Uses the dynamic spawn_user_ws() so they're tracked and can be stopped on disconnect
    {
        let startup_state = state.clone();
        tokio::spawn(async move {
            match startup_state.db.get_wallets_with_api_credentials().await {
                Ok(wallets) => {
                    for (address, api_key, api_secret, api_passphrase) in wallets {
                        info!("Starting User WebSocket for wallet {}", address);
                        startup_state.spawn_user_ws(address, api_key, api_secret, api_passphrase).await;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load wallets for User WebSocket: {}", e);
                }
            }
        });
    }

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
        let scan_start = Instant::now();

        // Check for resolved positions (real-time with each scan)
        if let Err(e) = resolution_tracker.check_resolutions().await {
            tracing::warn!("Resolution check failed: {}", e);
        }

        // Scan for new opportunities
        match state.scanner.fetch_markets().await {
            Ok(markets) => {
                // Feed filtered markets to MC scanner (it can use sniper-filtered set)
                let _ = state.mc_markets_tx.send(markets.clone());

                let all_opps = state.runner.find_all_opportunities(&markets);

                let mut combined = all_opps.sniper;

                // Sort by edge descending
                combined.sort_by(|a, b| {
                    b.edge.partial_cmp(&a.edge).unwrap_or(std::cmp::Ordering::Equal)
                });

                let count = combined.len();

                // Build reverse map: token_id -> condition_id for holders API
                // The Data API returns results keyed by CLOB token_id, not condition_id
                let mut token_to_condition: HashMap<String, String> = HashMap::new();
                for m in &markets {
                    if let Some(yes_tid) = &m.yes_token_id {
                        token_to_condition.insert(yes_tid.clone(), m.condition_id.clone());
                    }
                    if let Some(no_tid) = &m.no_token_id {
                        token_to_condition.insert(no_tid.clone(), m.condition_id.clone());
                    }
                }

                // Fetch top holders for each opportunity
                let condition_ids: Vec<String> = combined
                    .iter()
                    .map(|o| o.condition_id.clone())
                    .collect::<HashSet<String>>()
                    .into_iter()
                    .collect();

                match state.scanner.fetch_holders(&condition_ids, &token_to_condition).await {
                    Ok(holders_map) => {
                        for opp in &mut combined {
                            if let Some(holders) = holders_map.get(&opp.condition_id) {
                                opp.holders = Some(holders.clone());
                            }
                        }
                        debug!("Attached holders for {} markets", holders_map.len());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to fetch holders: {}", e);
                    }
                }

                // Update cached opportunities
                *state.opportunities.write().await = combined.clone();

                // Broadcast to all WebSocket clients
                let _ = state.opportunity_tx.send(combined.clone());

                // Collect token IDs for real-time price subscriptions
                let mut all_tokens: HashSet<String> = HashSet::new();

                // Opportunity tokens
                for opp in &combined {
                    if let Some(token) = &opp.token_id {
                        all_tokens.insert(token.clone());
                    }
                }

                // Open position tokens (user's active holdings)
                if let Ok(position_tokens) = state.db.get_all_open_position_token_ids().await {
                    all_tokens.extend(position_tokens);
                }

                // Mint Maker live market tokens (for real-time price updates)
                {
                    let mm_tokens = state.mm_live_tokens.read().await;
                    if !mm_tokens.is_empty() {
                        info!("Including {} mint maker tokens in price WS subscription", mm_tokens.len());
                    }
                    all_tokens.extend(mm_tokens.iter().cloned());
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

        // Broadcast wallet balances for all active wallets
        broadcast_wallet_balances(&state).await;

        // Adaptive sleep: account for scan duration
        let elapsed = scan_start.elapsed();
        let target = Duration::from_secs(scan_interval);
        if let Some(remaining) = target.checked_sub(elapsed) {
            tokio::time::sleep(remaining).await;
        }
    }
}

/// USDC.e (bridged) contract address on Polygon
const USDC_ADDRESS: &str = "0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";

/// Fetch and broadcast USDC balances for all wallets with active sessions
async fn broadcast_wallet_balances(state: &AppState) {
    // Get all wallets that have active sessions
    let wallets = match state.db.get_active_wallet_addresses().await {
        Ok(w) => w,
        Err(e) => {
            tracing::debug!("Failed to get active wallets for balance update: {}", e);
            return;
        }
    };

    let client = reqwest::Client::new();
    let rpc_url = &state.config.polygon_rpc_url;

    for address in wallets {
        match fetch_usdc_balance(&client, rpc_url, &address).await {
            Ok(balance) => {
                let _ = state.balance_tx.send(WalletBalanceUpdate {
                    address,
                    usdc_balance: balance,
                });
            }
            Err(e) => {
                tracing::debug!("Failed to fetch balance for {}: {}", address, e);
            }
        }
    }
}

/// Fetch USDC balance from Polygon RPC
async fn fetch_usdc_balance(
    client: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> Result<String> {
    let padded = format!(
        "000000000000000000000000{}",
        address.trim_start_matches("0x")
    );
    let data = format!("0x70a08231{}", padded);

    #[derive(serde::Deserialize)]
    struct RpcResponse {
        result: Option<String>,
    }

    let resp: RpcResponse = client
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"to": USDC_ADDRESS, "data": data}, "latest"],
            "id": 1
        }))
        .send()
        .await?
        .json()
        .await?;

    let hex = resp.result.unwrap_or_default();
    let raw = u128::from_str_radix(hex.trim_start_matches("0x"), 16).unwrap_or(0);
    let whole = raw / 1_000_000;
    let frac = (raw % 1_000_000) * 100 / 1_000_000;
    Ok(format!("{}.{:02}", whole, frac))
}
