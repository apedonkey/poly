//! Mint Maker autonomous runner - the core service loop

use crate::config::MintMakerConfig;
use crate::db::Database;
use crate::services::auto_trader::KeyStore;
use crate::services::price_ws::PriceUpdate;
use crate::services::safe_activation::{self, BuilderCredentials};
use crate::services::TickSizeCache;
use crate::strategies::MintMakerStrategy;
use crate::types::MintMakerMarket;
use alloy::signers::{local::PrivateKeySigner, Signer};
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::Instant;
use chrono::Utc;
use tracing::{debug, info, warn};

use super::order_manager::{self, FillStatus};
use super::inventory;
use super::scanner;
use super::types::{MintMakerMarketStatus, MintMakerStatsSnapshot, MintMakerStatusUpdate};

/// The Mint Maker runner - manages the autonomous loop
pub struct MintMakerRunner {
    db: Arc<Database>,
    key_store: KeyStore,
    config: MintMakerConfig,
    client: reqwest::Client,
    _tick_size_cache: Arc<TickSizeCache>,
    /// Live CLOB prices from price WebSocket (token_id -> midpoint)
    price_cache: Arc<RwLock<HashMap<String, Decimal>>>,
    /// Best bid prices from orderbook (token_id -> best bid)
    bid_cache: Arc<RwLock<HashMap<String, Decimal>>>,
    /// Shared set of token IDs for live markets (main scanner subscribes these to price WS)
    mm_live_tokens: Arc<RwLock<HashSet<String>>>,
    /// Cache of wallet addresses whose Safe is confirmed activated (deployed + approved)
    activated_wallets: Mutex<HashSet<String>>,
    /// Merge attempt tracker: pair_id -> (attempt_count, last_attempt_time)
    merge_tracker: Mutex<HashMap<i64, (u32, Instant)>>,
}

impl MintMakerRunner {
    pub fn new(
        db: Arc<Database>,
        key_store: KeyStore,
        config: MintMakerConfig,
        client: reqwest::Client,
        tick_size_cache: Arc<TickSizeCache>,
        price_tx: broadcast::Sender<PriceUpdate>,
        mm_live_tokens: Arc<RwLock<HashSet<String>>>,
    ) -> Self {
        let price_cache: Arc<RwLock<HashMap<String, Decimal>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let bid_cache: Arc<RwLock<HashMap<String, Decimal>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Spawn background listener for real-time price updates
        let cache = price_cache.clone();
        let bcache = bid_cache.clone();
        let mut price_rx = price_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match price_rx.recv().await {
                    Ok(update) => {
                        if let Ok(price) = Decimal::from_str(&update.price) {
                            cache.write().await.insert(update.token_id.clone(), price);
                        }
                        if let Some(bid_str) = &update.best_bid {
                            if let Ok(bid) = Decimal::from_str(bid_str) {
                                bcache.write().await.insert(update.token_id, bid);
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!("MintMaker price listener lagged {} messages", n);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        Self {
            db,
            key_store,
            config,
            client,
            _tick_size_cache: tick_size_cache,
            price_cache,
            bid_cache,
            mm_live_tokens,
            activated_wallets: Mutex::new(HashSet::new()),
            merge_tracker: Mutex::new(HashMap::new()),
        }
    }

    /// Main run loop - fetches markets directly and manages pairs
    pub async fn run(
        &self,
        status_tx: broadcast::Sender<MintMakerStatusUpdate>,
    ) {
        info!("MintMaker runner started (dedicated scanner mode)");

        let mut interval = tokio::time::interval(
            std::time::Duration::from_secs(self.config.rebalance_interval_seconds),
        );

        loop {
            interval.tick().await;
            if let Err(e) = self.run_cycle(&status_tx).await {
                warn!("MintMaker cycle error: {}", e);
            }
        }
    }

    /// Run one management cycle
    async fn run_cycle(
        &self,
        status_tx: &broadcast::Sender<MintMakerStatusUpdate>,
    ) -> anyhow::Result<()> {
        // Fetch 15-minute markets directly instead of using broadcast channel
        let markets = scanner::fetch_15m_crypto_markets(&self.client, &self.config.assets)
            .await
            .unwrap_or_default();

        // Convert ALL scanned markets to MintMakerMarket (for UI display)
        // Use a permissive config that doesn't filter by time
        let mut display_config = self.config.clone();
        display_config.min_minutes_to_close = 0.0;
        display_config.max_minutes_to_close = 999.0;
        let display_strategy = MintMakerStrategy::new(display_config);
        let mut all_markets = display_strategy.find_markets(&markets);

        // Overlay real-time CLOB prices from the price WebSocket onto all upcoming markets.
        // This covers both live (in 15-min window) and future markets we may bid into early.
        {
            let cache = self.price_cache.read().await;
            let mut live_tokens = HashSet::new();
            for market in &mut all_markets {
                if market.minutes_to_close > 0.0 {
                    // Register tokens for price WS (live + future)
                    live_tokens.insert(market.yes_token_id.clone());
                    live_tokens.insert(market.no_token_id.clone());

                    // Overlay cached live prices if available (independently for each side)
                    if let Some(&yes_mid) = cache.get(&market.yes_token_id) {
                        market.yes_price = yes_mid;
                    }
                    if let Some(&no_mid) = cache.get(&market.no_token_id) {
                        market.no_price = no_mid;
                    }
                }
            }
            // Also register tokens from half-filled pairs so stop loss can track them
            // even after the market drops out of the scanner
            if let Ok(hf_tokens) = self.db.get_mint_maker_half_filled_token_ids().await {
                for (yes_tid, no_tid) in &hf_tokens {
                    if let Some(tid) = yes_tid {
                        if !tid.is_empty() { live_tokens.insert(tid.clone()); }
                    }
                    if let Some(tid) = no_tid {
                        if !tid.is_empty() { live_tokens.insert(tid.clone()); }
                    }
                }
            }

            // Update shared token set so main scanner subscribes these in the price WS
            let token_count = live_tokens.len();
            *self.mm_live_tokens.write().await = live_tokens;
            if token_count > 0 {
                info!("MintMaker: {} live tokens registered for real-time prices", token_count);
            }
        }

        debug!(
            "MintMaker: scanned {} raw markets, {} displayable",
            markets.len(),
            all_markets.len()
        );

        // Get wallets to process: enabled wallets + any wallet with open pairs
        // (so stop loss and fill tracking work even when mint maker is disabled)
        let enabled_wallets = self.db.get_mint_maker_enabled_wallets().await?;
        let open_pair_wallets = self.db.get_mint_maker_wallets_with_open_pairs().await.unwrap_or_default();

        let mut all_wallets = enabled_wallets.clone();
        for w in &open_pair_wallets {
            if !all_wallets.contains(w) {
                all_wallets.push(w.clone());
            }
        }

        if all_wallets.is_empty() {
            // Still broadcast status for frontend — show ALL markets
            let update = self.build_status_update(&all_markets, "").await?;
            let _ = status_tx.send(update);
            return Ok(());
        }

        // Process each wallet (fill checks + stop loss always run; new placements only when enabled)
        for wallet_address in &all_wallets {
            if let Err(e) = self
                .process_wallet(wallet_address, &markets)
                .await
            {
                warn!("MintMaker error for wallet {}: {}", wallet_address, e);
            }
        }

        // Broadcast status update (use first wallet for settings, or empty)
        // Show ALL markets so the UI always sees them
        let primary_wallet = enabled_wallets.first().map(|s| s.as_str()).unwrap_or("");
        let update = self.build_status_update(&all_markets, primary_wallet).await?;
        let _ = status_tx.send(update);

        Ok(())
    }

    /// Process a single wallet's mint maker operations
    async fn process_wallet(
        &self,
        wallet_address: &str,
        raw_markets: &[crate::types::TrackedMarket],
    ) -> anyhow::Result<()> {
        let settings = self.db.get_mint_maker_settings(wallet_address).await?;

        // Get API credentials for order management.
        // If missing (e.g. runner restarted, enable never called), derive them now.
        let (api_key, api_secret, api_passphrase) = match self.db.get_api_credentials(wallet_address).await? {
            Some(c) => c,
            None => {
                // Try to derive credentials if we have the private key
                match self.key_store.get_key(wallet_address).await {
                    Some(pk) => {
                        match order_manager::ensure_clob_api_credentials(&pk, &self.db, wallet_address).await {
                            Ok(c) => c,
                            Err(e) => {
                                warn!("MintMaker: Failed to derive API credentials for {}: {}", wallet_address, e);
                                return Ok(());
                            }
                        }
                    }
                    None => {
                        debug!("MintMaker: No API credentials or key for wallet {}", wallet_address);
                        return Ok(());
                    }
                }
            }
        };

        // === Always run: fill checks, stop loss, and stop loss tracking ===
        // These must run even when the mint maker is disabled so existing positions
        // are protected and tracked properly.

        // 1. Check fills on open pairs
        let open_pairs = self.db.get_mint_maker_open_pairs(wallet_address).await?;
        for pair in &open_pairs {
            if pair.status == "Pending" || pair.status == "HalfFilled" {
                self.check_pair_fills(
                    wallet_address,
                    pair,
                    &api_key,
                    &api_secret,
                    &api_passphrase,
                )
                .await;
            }
        }

        // 1b. Stop loss — disabled. Half-filled pairs ride to market close.

        // === Beyond here, only run if mint maker is enabled ===
        if !settings.enabled {
            return Ok(());
        }

        // Build strategy using the wallet's own asset list (not global config)
        let mut wallet_config = self.config.clone();
        wallet_config.min_minutes_to_close = 0.0;
        wallet_config.max_minutes_to_close = 999.0;
        wallet_config.assets = settings.assets.clone();
        let strategy = MintMakerStrategy::new(wallet_config);
        let eligible_markets = strategy.find_markets(raw_markets);

        let eligible_ids: Vec<String> = eligible_markets.iter()
            .map(|m| format!("{}({})", m.asset, &m.market_id[..10]))
            .collect();
        info!(
            "MintMaker: wallet={} assets={:?} raw_markets={} eligible={} ids={:?}",
            &wallet_address[..8], settings.assets, raw_markets.len(), eligible_markets.len(), eligible_ids
        );

        // 2. Merge matched pairs (with cooldown + retry limit)
        let matched_pairs = self
            .db
            .get_mint_maker_pairs_by_status(wallet_address, "Matched")
            .await?;
        if !matched_pairs.is_empty() {
            if let Some(private_key) = self.key_store.get_key(wallet_address).await {
                match (
                    std::env::var("POLY_BUILDER_API_KEY").ok(),
                    std::env::var("POLY_BUILDER_SECRET").ok(),
                    std::env::var("POLY_BUILDER_PASSPHRASE").ok(),
                ) {
                    (Some(bk), Some(bs), Some(bp)) => {
                        for pair in &matched_pairs {
                            // Check merge cooldown: wait 30s between attempts, give up after 5 min
                            const MERGE_COOLDOWN_SECS: u64 = 30;
                            const MERGE_MAX_ATTEMPTS: u32 = 10; // 10 * 30s = 5 min

                            let mut tracker = self.merge_tracker.lock().await;
                            let (attempts, last_attempt) = tracker
                                .entry(pair.id)
                                .or_insert((0, Instant::now() - std::time::Duration::from_secs(MERGE_COOLDOWN_SECS)));

                            if *attempts >= MERGE_MAX_ATTEMPTS {
                                // Too many failed attempts — give up
                                warn!(
                                    "MintMaker: Pair {} failed to merge after {} attempts — marking MergeFailed",
                                    pair.id, attempts
                                );
                                let _ = self.db.update_mint_maker_pair_status(pair.id, "MergeFailed").await;
                                tracker.remove(&pair.id);
                                continue;
                            }

                            if last_attempt.elapsed() < std::time::Duration::from_secs(MERGE_COOLDOWN_SECS) {
                                debug!(
                                    "MintMaker: Pair {} merge cooldown ({}/{} attempts, {}s since last)",
                                    pair.id, attempts, MERGE_MAX_ATTEMPTS, last_attempt.elapsed().as_secs()
                                );
                                continue;
                            }

                            *attempts += 1;
                            *last_attempt = Instant::now();
                            let attempt_num = *attempts;
                            drop(tracker);

                            match inventory::merge_matched_pair(
                                &self.db,
                                pair.id,
                                &pair.condition_id,
                                &pair.size,
                                &private_key,
                                &bk,
                                &bs,
                                &bp,
                                pair.yes_token_id.as_deref(),
                                pair.no_token_id.as_deref(),
                                pair.neg_risk,
                            )
                            .await
                            {
                                Ok(Some(tx_id)) => {
                                    // Success — remove from tracker
                                    self.merge_tracker.lock().await.remove(&pair.id);
                                    let _ = self
                                        .db
                                        .log_mint_maker_action(
                                            wallet_address,
                                            "merge",
                                            Some(&pair.market_id),
                                            Some(&pair.question),
                                            Some(&pair.asset),
                                            None,
                                            None,
                                            pair.pair_cost.as_deref(),
                                            pair.profit.as_deref(),
                                            Some(&pair.size),
                                            Some(&format!("tx: {}", tx_id)),
                                        )
                                        .await;
                                }
                                Ok(None) => {
                                    info!(
                                        "MintMaker: Pair {} merge attempt {}/{} — waiting for token settlement",
                                        pair.id, attempt_num, MERGE_MAX_ATTEMPTS
                                    );
                                }
                                Err(e) => warn!("MintMaker merge error for pair {}: {}", pair.id, e),
                            }
                        }
                    }
                    _ => {
                        warn!(
                            "MintMaker: {} matched pairs waiting to merge but POLY_BUILDER_* env vars not set! \
                             Set POLY_BUILDER_API_KEY, POLY_BUILDER_SECRET, POLY_BUILDER_PASSPHRASE.",
                            matched_pairs.len()
                        );
                    }
                }
            } else {
                warn!(
                    "MintMaker: {} matched pairs waiting to merge but no private key loaded for {}. \
                     Re-click Enable in the UI.",
                    matched_pairs.len(), &wallet_address[..8]
                );
            }
        }

        // 2b. Auto-redeem resolved markets
        if let Err(e) = self.check_auto_redeem(wallet_address).await {
            warn!("MintMaker auto-redeem error for {}: {}", &wallet_address[..8], e);
        }

        // 3. Cancel expired pairs — only cancel Pending orders whose market has closed
        //    (end_date has passed). Orders on open/future markets should keep waiting.
        let pending_pairs = self
            .db
            .get_mint_maker_pairs_by_status(wallet_address, "Pending")
            .await?;
        let now = Utc::now();
        for pair in &pending_pairs {
            // Find the market's end_date from the scanned data
            let market_ended = raw_markets.iter()
                .find(|m| m.condition_id == pair.condition_id)
                .and_then(|m| m.end_date)
                .map(|end| now > end)
                .unwrap_or_else(|| {
                    // Market not in scanner anymore — it's gone, cancel the pair
                    true
                });

            if !market_ended {
                continue;
            }

            info!("MintMaker: Cancelling expired pair {} — market has closed", pair.id);
            let _ = order_manager::cancel_order(
                wallet_address,
                &pair.yes_order_id,
                &api_key,
                &api_secret,
                &api_passphrase,
            )
            .await;
            let _ = order_manager::cancel_order(
                wallet_address,
                &pair.no_order_id,
                &api_key,
                &api_secret,
                &api_passphrase,
            )
            .await;
            let _ = self
                .db
                .update_mint_maker_pair_status(pair.id, "Cancelled")
                .await;
            let _ = self
                .db
                .log_mint_maker_action(
                    wallet_address,
                    "cancel_expired",
                    Some(&pair.market_id),
                    Some(&pair.question),
                    Some(&pair.asset),
                    None,
                    None,
                    None,
                    None,
                    Some(&pair.size),
                    Some("Market closed — unfilled orders cancelled"),
                )
                .await;
        }

        // 4. Auto-place pairs on eligible markets if enabled
        if settings.auto_place {
            info!(
                "MintMaker: AUTO-PLACE ON for {} — assets={:?} eligible={} max_markets={}",
                &wallet_address[..8], settings.assets, eligible_markets.len(), settings.auto_max_markets
            );
            if let Some(private_key) = self.key_store.get_key(wallet_address).await {
                // Ensure Safe has CLOB approval before placing any orders
                let signer_result: Result<PrivateKeySigner, _> = private_key.parse();
                if let Ok(signer) = signer_result {
                    let signer = signer.with_chain_id(Some(137));
                    if let (Some(bk), Some(bs), Some(bp)) = (
                        std::env::var("POLY_BUILDER_API_KEY").ok(),
                        std::env::var("POLY_BUILDER_SECRET").ok(),
                        std::env::var("POLY_BUILDER_PASSPHRASE").ok(),
                    ) {
                        let bcreds = BuilderCredentials { api_key: bk, secret: bs, passphrase: bp };
                        // Only check Safe activation once per session per wallet
                        let already_activated = self.activated_wallets.lock().await.contains(wallet_address);
                        if !already_activated {
                            match safe_activation::ensure_safe_activated(&signer, &bcreds).await {
                                Ok(addr) => {
                                    info!("MintMaker: Safe ready at {}", addr);
                                    self.activated_wallets.lock().await.insert(wallet_address.to_string());
                                }
                                Err(e) => warn!("MintMaker: Safe activation failed: {}", e),
                            }
                        }
                    }
                }

                // Refresh CLOB's cached view of on-chain balance & allowances.
                // Without this, the CLOB rejects orders with "insufficient balance"
                // even though on-chain approvals are set.
                if let Err(e) = order_manager::refresh_clob_allowance_cache(&private_key).await {
                    warn!("MintMaker: CLOB cache refresh failed: {}", e);
                }

                // Check Safe balance once before placing
                let safe_balance = match order_manager::derive_safe_address(&private_key) {
                    Ok(safe_addr) => {
                        order_manager::fetch_safe_usdc_balance(&self.client, &safe_addr)
                            .await
                            .unwrap_or_else(|e| {
                                warn!("MintMaker: Could not check balance: {}", e);
                                Decimal::ZERO
                            })
                    }
                    Err(_) => Decimal::ZERO,
                };
                // Subtract reserve from available balance
                let reserve = Decimal::from_str(&format!("{:.2}", settings.balance_reserve)).unwrap_or(Decimal::ZERO);
                let available = safe_balance - reserve;
                if available <= Decimal::ZERO {
                    info!("MintMaker: Balance ${} <= reserve ${}, skipping placement", safe_balance, reserve);
                    // Fall through — remaining_balance of 0 will cause all pairs to be skipped
                }
                let mut remaining_balance = if available > Decimal::ZERO { available } else { Decimal::ZERO };

                // USD per side: either balance-based (auto_size_pct > 0) or fixed
                let usd_per_side_raw = if settings.auto_size_pct > 0 {
                    let pct = Decimal::from(settings.auto_size_pct) / Decimal::from(100);
                    let capital = available * pct;
                    let per_market = capital / Decimal::from(settings.auto_max_markets);
                    per_market / Decimal::from(2) // split per side
                } else {
                    Decimal::from_str(&settings.auto_place_size).unwrap_or(Decimal::from(2))
                };

                let mut total_open = self
                    .db
                    .count_mint_maker_total_open_pairs(wallet_address)
                    .await
                    .unwrap_or(0);

                // Smart mode: compute derived settings dynamically
                let num_selected_assets = settings.assets.len() as i32;
                let smart_min_profit = Decimal::from_str(&format!("{:.4}", settings.min_spread_profit)).unwrap_or(Decimal::from_str("0.01").unwrap());
                let smart_max_cost = Decimal::ONE - smart_min_profit;
                let smart_max_price = std::cmp::max(
                    eligible_markets.iter().map(|m| std::cmp::max(m.yes_price, m.no_price)).max().unwrap_or(Decimal::from_str("0.50").unwrap()),
                    Decimal::from_str("0.50").unwrap(),
                );
                let smart_pairs_per_market = std::cmp::min(
                    (usd_per_side_raw / (smart_max_price * Decimal::from(5))).floor().to_string().parse::<i32>().unwrap_or(1).max(1),
                    5,
                );
                let smart_total_pairs = std::cmp::min(smart_pairs_per_market * num_selected_assets, 30);

                // In smart mode, split the per-market budget evenly across pairs
                let usd_per_side = if settings.smart_mode && smart_pairs_per_market > 1 {
                    let split = usd_per_side_raw / Decimal::from(smart_pairs_per_market);
                    info!("MintMaker: SMART splitting ${}/side into {} pairs → ${}/pair/side", usd_per_side_raw, smart_pairs_per_market, split);
                    split
                } else {
                    usd_per_side_raw
                };

                // Use smart overrides or stored settings
                let effective_delay_mins = if settings.smart_mode { 2.0 } else { settings.auto_place_delay_mins as f64 };
                let effective_max_pairs_per_market = if settings.smart_mode { smart_pairs_per_market } else { settings.max_pairs_per_market };
                let effective_max_total_pairs = if settings.smart_mode { smart_total_pairs } else { settings.max_total_pairs };

                if settings.smart_mode {
                    info!(
                        "MintMaker: SMART MODE — max_cost={} pairs/mkt={} total={} delay=2m",
                        smart_max_cost, smart_pairs_per_market, smart_total_pairs
                    );
                }

                // Auto-place on markets in the current 15-min window.
                // auto_place_delay_mins: wait N minutes into the window before placing.
                // e.g. delay=3 means only place when minutes_to_close <= 12.
                let delay_mins = effective_delay_mins;
                let place_cutoff = 15.0 - delay_mins;
                // Markets past the delay cutoff that still have capacity for more pairs.
                // We allow 1 new pair per market per cycle (natural 30s cooldown).
                // Smart mode: don't bid on markets with less than 2 minutes left
                let min_time = if settings.smart_mode { 2.0 } else { 0.0 };

                let mut placeable_markets: Vec<&MintMakerMarket> = Vec::new();
                for m in &eligible_markets {
                    if m.minutes_to_close > place_cutoff || m.minutes_to_close <= min_time {
                        continue;
                    }
                    let market_pairs = self
                        .db
                        .count_mint_maker_open_pairs_for_market(wallet_address, &m.market_id)
                        .await
                        .unwrap_or(0);
                    if market_pairs >= effective_max_pairs_per_market as i64 {
                        continue;
                    }
                    // Cap total attempts (all statuses) per market
                    let total_attempts = self
                        .db
                        .count_mint_maker_all_pairs_for_market(wallet_address, &m.market_id)
                        .await
                        .unwrap_or(0);
                    if total_attempts >= settings.auto_max_attempts as i64 {
                        continue;
                    }
                    placeable_markets.push(m);
                }

                {

                let mut markets_placed = 0i32;

                // Use wallet-specific settings for profitability thresholds
                let manual_max_cost = Decimal::from_str(&format!("{:.4}", settings.max_pair_cost)).unwrap_or(Decimal::from_str("0.98").unwrap());
                let max_cost = if settings.smart_mode { smart_max_cost } else { manual_max_cost };
                let min_profit = Decimal::from_str(&format!("{:.4}", settings.min_spread_profit)).unwrap_or(Decimal::from_str("0.01").unwrap());

                    if placeable_markets.is_empty() {
                        info!(
                            "MintMaker: Waiting — {} eligible, none placeable (delay={}m)",
                            eligible_markets.len(), delay_mins
                        );
                    } else {
                        let manual_bid_off = Decimal::from(settings.bid_offset_cents) / Decimal::from(100);
                        info!("MintMaker: {} placeable market(s){}:{:?}",
                            placeable_markets.len(),
                            if settings.smart_mode { " [SMART+BOOK]" } else { "" },
                            placeable_markets.iter()
                                .map(|m| {
                                    if settings.smart_mode {
                                        // Smart mode bids are computed per-market at placement time (book + fill rate)
                                        format!("{} {:.1}m YES@{} NO@{} (book-aware)",
                                            m.asset, m.minutes_to_close, m.yes_price, m.no_price)
                                    } else {
                                        let cheap = std::cmp::min(m.yes_price, m.no_price);
                                        let cheap_bid = cheap - manual_bid_off;
                                        let exp_bid = max_cost - cheap_bid;
                                        let (yb, nb) = if m.yes_price <= m.no_price {
                                            (cheap_bid, exp_bid)
                                        } else {
                                            (exp_bid, cheap_bid)
                                        };
                                        format!("{} {:.1}m YES@{} NO@{} → bid YES@{} NO@{} cost={}",
                                            m.asset, m.minutes_to_close, m.yes_price, m.no_price,
                                            yb, nb, yb + nb)
                                    }
                                })
                                .collect::<Vec<_>>()
                        );
                    }

                for market in &placeable_markets {
                    // Respect auto_max_markets setting (unique markets placed this cycle)
                    if markets_placed >= settings.auto_max_markets {
                        info!("MintMaker: Hit auto_max_markets limit ({})", settings.auto_max_markets);
                        break;
                    }

                    // In smart mode, place multiple pairs per market in one cycle
                    let pairs_this_market = if settings.smart_mode { effective_max_pairs_per_market } else { 1 };

                    'pairs: for _pair_idx in 0..pairs_this_market {

                    // Check total capacity
                    if total_open >= effective_max_total_pairs as i64 {
                        info!("MintMaker: Hit max_total_pairs limit ({})", effective_max_total_pairs);
                        break;
                    }

                    // Cheap-side bidding: bid near the cheaper side, derive expensive
                    // side from max_pair_cost so total cost is controlled.
                    let yes_is_cheap = market.yes_price <= market.no_price;
                    let (cheap_current, expensive_current) = if yes_is_cheap {
                        (market.yes_price, market.no_price)
                    } else {
                        (market.no_price, market.yes_price)
                    };

                    let (cheap_bid, expensive_bid) = if settings.smart_mode {
                        // Smart mode: use actual orderbook best_bid for both sides
                        let cheap_token = if yes_is_cheap { &market.yes_token_id } else { &market.no_token_id };
                        let exp_token = if yes_is_cheap { &market.no_token_id } else { &market.yes_token_id };
                        let bid_snapshot = self.bid_cache.read().await;
                        let cheap_book_bid = bid_snapshot.get(cheap_token).copied();
                        let exp_book_bid = bid_snapshot.get(exp_token).copied();
                        drop(bid_snapshot);

                        // Fill rate feedback: query recent outcomes for this asset
                        let (total, filled) = self.db
                            .get_mint_maker_asset_fill_rate(wallet_address, &market.asset.to_string(), 4)
                            .await
                            .unwrap_or((0, 0));
                        let fill_rate = if total >= 3 { filled as f64 / total as f64 } else { 0.5 };
                        let fill_adj = if fill_rate > 0.80 {
                            Decimal::from_str("-0.01").unwrap() // too aggressive → bid lower
                        } else if fill_rate < 0.40 {
                            Decimal::from_str("0.01").unwrap()  // too conservative → bid higher
                        } else {
                            Decimal::ZERO
                        };
                        let one_cent = Decimal::from_str("0.01").unwrap();

                        // === CHEAP SIDE: bid best_bid + 1¢ for queue priority ===
                        let cheap_base = match cheap_book_bid {
                            Some(bb) if bb > Decimal::ZERO => {
                                let priority_bid = bb + one_cent + fill_adj;
                                info!(
                                    "MintMaker: SMART {} cheap book_bid={}¢ +1¢ priority, fill={:.0}%({}/{}) adj={}¢ → {}¢",
                                    market.asset, bb * Decimal::from(100),
                                    fill_rate * 100.0, filled, total,
                                    fill_adj * Decimal::from(100), priority_bid * Decimal::from(100)
                                );
                                priority_bid
                            }
                            _ => {
                                // Fallback: formula-based offset
                                let raw_off = ((Decimal::ONE - cheap_current) * Decimal::from(10) / Decimal::from(100)).floor() / Decimal::from(100);
                                let offset = raw_off.max(one_cent).min(Decimal::from_str("0.05").unwrap());
                                let fallback = cheap_current - offset + fill_adj;
                                info!(
                                    "MintMaker: SMART {} no cheap book data, formula offset={}¢ adj={}¢ → {}¢",
                                    market.asset, offset * Decimal::from(100),
                                    fill_adj * Decimal::from(100), fallback * Decimal::from(100)
                                );
                                fallback
                            }
                        };
                        // Clamp: never bid at or above current price
                        let cb = cheap_base.min(cheap_current - one_cent);

                        // === EXPENSIVE SIDE: derived from max_cost, but book-aware ===
                        let derived_exp = max_cost - cb;
                        let eb = match exp_book_bid {
                            Some(ebb) if ebb > Decimal::ZERO => {
                                if derived_exp < ebb - Decimal::from_str("0.03").unwrap() {
                                    // Our derived bid is 3+¢ behind the book — we'd never fill.
                                    // Use book's best_bid + 1¢ instead for queue priority,
                                    // but only if total cost still respects min_profit.
                                    let book_bid = ebb + one_cent;
                                    let book_cost = cb + book_bid;
                                    let book_profit = Decimal::ONE - book_cost;
                                    if book_profit >= min_profit {
                                        info!(
                                            "MintMaker: SMART {} exp derived={}¢ too far from book={}¢, using book+1={}¢ (profit={}¢)",
                                            market.asset, derived_exp * Decimal::from(100),
                                            ebb * Decimal::from(100), book_bid * Decimal::from(100),
                                            book_profit * Decimal::from(100)
                                        );
                                        book_bid
                                    } else {
                                        // Book bid would push us below min profit — stick with derived
                                        info!(
                                            "MintMaker: SMART {} exp book={}¢ too expensive (profit {}¢ < min {}¢), using derived={}¢",
                                            market.asset, book_bid * Decimal::from(100),
                                            book_profit * Decimal::from(100), min_profit * Decimal::from(100),
                                            derived_exp * Decimal::from(100)
                                        );
                                        derived_exp
                                    }
                                } else {
                                    // Derived bid is within 3¢ of the book — good position, use it
                                    derived_exp
                                }
                            }
                            _ => derived_exp, // no book data for expensive side
                        };
                        (cb, eb)
                    } else {
                        let offset = Decimal::from(settings.bid_offset_cents) / Decimal::from(100);
                        let cb = cheap_current - offset;
                        let eb = max_cost - cb;
                        (cb, eb)
                    };

                    // Validate bids are positive
                    if cheap_bid <= Decimal::ZERO || expensive_bid <= Decimal::ZERO {
                        info!("MintMaker: SKIP {} — bid <= 0 (cheap_bid={} expensive_bid={})", market.asset, cheap_bid, expensive_bid);
                        break 'pairs;
                    }

                    // Don't bid at or above current price on the expensive side
                    // (would cross the spread and taker-fill immediately)
                    if expensive_bid >= expensive_current {
                        info!(
                            "MintMaker: SKIP {} — expensive bid {}c >= current {}c (lower max_pair_cost or raise offset)",
                            market.asset, expensive_bid, expensive_current
                        );
                        break 'pairs;
                    }

                    let (yes_price, no_price) = if yes_is_cheap {
                        (cheap_bid, expensive_bid)
                    } else {
                        (expensive_bid, cheap_bid)
                    };

                    let actual_pair_cost = yes_price + no_price;
                    let actual_profit = Decimal::ONE - actual_pair_cost;
                    if actual_profit < min_profit {
                        info!(
                            "MintMaker: SKIP {} — profit {} < min {} (bid YES@{} NO@{})",
                            market.asset, actual_profit, min_profit, yes_price, no_price
                        );
                        break 'pairs;
                    }

                    // Calculate EQUAL shares for both sides so every share can be merged.
                    // Use the more expensive side to determine share count (ensures we
                    // can afford both sides and don't end up with unmatched leftovers).
                    let max_price = std::cmp::max(yes_price, no_price);
                    let shares = if max_price > Decimal::ZERO { (usd_per_side / max_price).floor() } else { Decimal::ZERO };

                    // CLOB requires minimum 5 shares per order
                    let min_shares = Decimal::from(5);
                    if shares < min_shares {
                        info!(
                            "MintMaker: SKIP {} — shares {} below min 5 at max_price={}. Need ${}/side.",
                            market.asset, shares, max_price,
                            (min_shares * max_price).ceil()
                        );
                        break 'pairs;
                    }
                    let yes_shares = shares;
                    let no_shares = shares;
                    let merge_size = shares;
                    let total_cost = (shares * yes_price) + (shares * no_price);

                    // Check balance
                    if total_cost > remaining_balance {
                        warn!(
                            "MintMaker: Insufficient balance ${} for ${} pair on {}",
                            remaining_balance, total_cost, market.asset
                        );
                        break 'pairs;
                    }

                    // Momentum bias: look at the expiring market for this asset to decide
                    // which side to place first. If the previous market's YES > 0.90, YES
                    // is winning → place YES first. If YES < 0.10, NO is winning → place
                    // NO first. This biases toward filling the winning side first.
                    let yes_first = {
                        let expiring = eligible_markets.iter().find(|m| {
                            m.asset == market.asset
                                && m.market_id != market.market_id
                                && m.minutes_to_close < 0.5
                                && m.minutes_to_close >= 0.0
                        });
                        match expiring {
                            Some(prev) if prev.yes_price > Decimal::from_str("0.85").unwrap() => {
                                info!("MintMaker: Momentum bias → YES first (prev {} YES@{})", market.asset, prev.yes_price);
                                true
                            }
                            Some(prev) if prev.yes_price < Decimal::from_str("0.15").unwrap() => {
                                info!("MintMaker: Momentum bias → NO first (prev {} YES@{})", market.asset, prev.yes_price);
                                false
                            }
                            _ => true, // default: YES first
                        }
                    };

                    // Place orders: momentum side first for better fill probability
                    let (first_token, first_price, first_shares, first_label,
                         second_token, second_price, second_shares, second_label) = if yes_first {
                        (&market.yes_token_id, yes_price, yes_shares, "YES",
                         &market.no_token_id, no_price, no_shares, "NO")
                    } else {
                        (&market.no_token_id, no_price, no_shares, "NO",
                         &market.yes_token_id, yes_price, yes_shares, "YES")
                    };

                    let first_order_id = match order_manager::place_gtc_bid(
                        &private_key,
                        first_token,
                        first_price,
                        first_shares,
                    )
                    .await
                    {
                        Ok(id) => id,
                        Err(e) => {
                            warn!("MintMaker auto-place {} GTC failed for {}: {}", first_label, market.market_id, e);
                            break 'pairs;
                        }
                    };

                    // Place second side. If this fails, try to cancel first; if cancel fails, record orphan.
                    let second_order_id = match order_manager::place_gtc_bid(
                        &private_key,
                        second_token,
                        second_price,
                        second_shares,
                    )
                    .await
                    {
                        Ok(id) => id,
                        Err(e) => {
                            warn!(
                                "MintMaker auto-place {} GTC failed, cancelling {} {}: {}",
                                second_label, first_label, first_order_id, e
                            );
                            let cancel_ok = order_manager::cancel_order(
                                wallet_address,
                                &first_order_id,
                                &api_key,
                                &api_secret,
                                &api_passphrase,
                            )
                            .await
                            .is_ok();

                            if !cancel_ok {
                                warn!("MintMaker: {} cancel failed — recording orphaned order {}", first_label, first_order_id);
                                let asset_str = market.asset.to_string();
                                let first_price_str = first_price.to_string();
                                let shares_str = first_shares.to_string();
                                let (orphan_yes_id, orphan_no_id) = if yes_first {
                                    (first_order_id.as_str(), "")
                                } else {
                                    ("", first_order_id.as_str())
                                };
                                let (orphan_yes_price, orphan_no_price) = if yes_first {
                                    (first_price_str.as_str(), "0")
                                } else {
                                    ("0", first_price_str.as_str())
                                };
                                if let Ok(pair_id) = self.db.create_mint_maker_pair(
                                    wallet_address,
                                    &market.market_id,
                                    &market.condition_id,
                                    &market.question,
                                    &asset_str,
                                    orphan_yes_id, orphan_no_id,
                                    orphan_yes_price, orphan_no_price,
                                    &shares_str,
                                    Some(&shares_str), None,
                                    Some(&market.slug),
                                    Some(&market.yes_token_id),
                                    Some(&market.no_token_id),
                                    market.neg_risk,
                                ).await {
                                    let _ = self.db.update_mint_maker_pair_status(pair_id, "Orphaned").await;
                                }
                                let _ = self.db.log_mint_maker_action(
                                    wallet_address,
                                    "orphaned",
                                    Some(&market.market_id),
                                    Some(&market.question),
                                    Some(&asset_str),
                                    Some(&first_price_str), None,
                                    None, None,
                                    Some(&shares_str),
                                    Some(&format!("{} GTC {} — {} failed, cancel failed — orphaned", first_label, first_order_id, second_label)),
                                ).await;
                            }
                            break 'pairs;
                        }
                    };

                    // Map back to YES/NO order IDs for DB record
                    let (yes_order_id, no_order_id) = if yes_first {
                        (first_order_id, second_order_id)
                    } else {
                        (second_order_id, first_order_id)
                    };

                    // Both GTC orders placed. Record the pair.
                    let pair_cost = yes_price + no_price;
                    let per_pair_profit = Decimal::ONE - pair_cost;
                    let asset_str = market.asset.to_string();
                    let yes_price_str = yes_price.to_string();
                    let no_price_str = no_price.to_string();
                    let pair_cost_str = pair_cost.to_string();
                    let profit_str = per_pair_profit.to_string();
                    let merge_size_str = merge_size.to_string();
                    let yes_shares_str = yes_shares.to_string();
                    let no_shares_str = no_shares.to_string();

                    let _ = self
                        .db
                        .create_mint_maker_pair(
                            wallet_address,
                            &market.market_id,
                            &market.condition_id,
                            &market.question,
                            &asset_str,
                            &yes_order_id,
                            &no_order_id,
                            &yes_price_str,
                            &no_price_str,
                            &merge_size_str,
                            Some(&yes_shares_str),
                            Some(&no_shares_str),
                            Some(&market.slug),
                            Some(&market.yes_token_id),
                            Some(&market.no_token_id),
                            market.neg_risk,
                        )
                        .await;

                    let _ = self
                        .db
                        .log_mint_maker_action(
                            wallet_address,
                            "auto_place",
                            Some(&market.market_id),
                            Some(&market.question),
                            Some(&asset_str),
                            Some(&yes_price_str),
                            Some(&no_price_str),
                            Some(&pair_cost_str),
                            Some(&profit_str),
                            Some(&merge_size_str),
                            Some(&format!(
                                "GTC@market ${}/side YES@{}x{} NO@{}x{}",
                                usd_per_side, yes_price, yes_shares, no_price, no_shares
                            )),
                        )
                        .await;

                    info!(
                        "MintMaker: Auto-placed GTC pair for {} - YES@{}x{} NO@{}x{} (${}/side)",
                        market.asset, yes_price, yes_shares, no_price, no_shares, usd_per_side
                    );

                    remaining_balance -= total_cost;
                    total_open += 1;
                    } // end 'pairs loop
                    markets_placed += 1;
                } // end for market in &placeable_markets
                } // end placement block
            } else {
                warn!(
                    "MintMaker: Auto-place enabled but no private key in memory for {}. Re-click Enable in the UI to load your key.",
                    &wallet_address[..8]
                );
            }
        }

        Ok(())
    }

    /// Stop loss: if a HalfFilled pair's filled side has dropped 25% from its
    /// fill price, cancel the unfilled order and sell the filled shares.
    async fn check_stop_loss(
        &self,
        wallet_address: &str,
        pair: &crate::db::MintMakerPairRow,
        _raw_markets: &[crate::types::TrackedMarket],
        api_key: &str,
        api_secret: &str,
        api_passphrase: &str,
        stop_loss_pct: i32,
        stop_loss_delay_secs: i32,
    ) {
        // Grace period: don't trigger stop loss until N seconds after the half-fill.
        // updated_at is set when the pair transitions to HalfFilled.
        if stop_loss_delay_secs > 0 {
            if let Ok(filled_at) = chrono::DateTime::parse_from_rfc3339(&pair.updated_at) {
                let elapsed = Utc::now().signed_duration_since(filled_at);
                if elapsed.num_seconds() < stop_loss_delay_secs as i64 {
                    debug!(
                        "MintMaker: Stop loss grace period for pair {} — {}s of {}s elapsed",
                        pair.id, elapsed.num_seconds(), stop_loss_delay_secs
                    );
                    return;
                }
            }
        }

        // Determine which side filled
        let yes_filled = pair.yes_fill_price.is_some();
        let no_filled = pair.no_fill_price.is_some();

        // Only act on pairs where exactly one side filled
        if yes_filled == no_filled {
            return; // both filled (shouldn't be HalfFilled) or neither filled
        }

        // Get fill price of the filled side
        let fill_price = if yes_filled {
            pair.yes_fill_price.as_ref()
        } else {
            pair.no_fill_price.as_ref()
        };
        let fill_price = match fill_price.and_then(|p| Decimal::from_str(p).ok()) {
            Some(p) if p > Decimal::ZERO => p,
            _ => return,
        };

        // Get the token ID of the filled side from the pair itself (not the scanner)
        let token_id = if yes_filled {
            pair.yes_token_id.as_deref()
        } else {
            pair.no_token_id.as_deref()
        };
        let token_id = match token_id {
            Some(id) if !id.is_empty() => id,
            _ => {
                warn!("MintMaker: No token ID on pair {} for stop loss check", pair.id);
                return;
            }
        };

        // Get current price from the live CLOB WebSocket price cache.
        // This works even after the market drops out of the scanner.
        let current_price = {
            let cache = self.price_cache.read().await;
            match cache.get(token_id).copied() {
                Some(p) => p,
                None => {
                    warn!("MintMaker: No live price for pair {} token {}…, skipping stop loss check", pair.id, &token_id[..10.min(token_id.len())]);
                    return;
                }
            }
        };

        // Stop loss threshold: configurable % drop from fill price
        let threshold_multiplier = Decimal::from(100 - stop_loss_pct) / Decimal::from(100);
        let stop_price = fill_price * threshold_multiplier;

        let drop_pct = ((fill_price - current_price) / fill_price * Decimal::from(100)).round();
        info!(
            "MintMaker: Stop loss check pair {} — filled@{} now@{} drop={}% threshold={}% ({})",
            pair.id, fill_price, current_price, drop_pct, stop_loss_pct, stop_price
        );

        if current_price >= stop_price {
            return; // price hasn't dropped enough, no stop loss
        }

        let filled_side = if yes_filled { "YES" } else { "NO" };
        warn!(
            "MintMaker STOP LOSS: Pair {} {} filled@{} now@{} (threshold {}). Selling.",
            pair.id, filled_side, fill_price, current_price, stop_price
        );

        // Cancel the unfilled order — use SDK cancel (correct endpoint)
        let unfilled_order_id = if yes_filled { &pair.no_order_id } else { &pair.yes_order_id };
        if !unfilled_order_id.is_empty() {
            if let Some(ref pk) = self.key_store.get_key(wallet_address).await {
                match order_manager::cancel_order_with_key(pk, unfilled_order_id).await {
                    Ok(()) => info!("MintMaker: Cancelled unfilled {} order for pair {}", if yes_filled { "NO" } else { "YES" }, pair.id),
                    Err(e) => {
                        warn!("MintMaker: SDK cancel failed for pair {}: {}. Trying raw.", pair.id, e);
                        let _ = order_manager::cancel_order(
                            wallet_address, unfilled_order_id, api_key, api_secret, api_passphrase,
                        ).await;
                    }
                }
            } else {
                let _ = order_manager::cancel_order(
                    wallet_address, unfilled_order_id, api_key, api_secret, api_passphrase,
                ).await;
            }
        }

        // Sell the filled tokens at an aggressive price to ensure fill.
        // Price 5 cents below current to maximize fill probability on a fast-moving market.

        let shares = if yes_filled {
            pair.yes_size.as_ref()
        } else {
            pair.no_size.as_ref()
        };
        let shares = match shares.and_then(|s| Decimal::from_str(s).ok()) {
            Some(s) if s > Decimal::ZERO => s,
            _ => {
                warn!("MintMaker: No share count for {} side of pair {}", filled_side, pair.id);
                let _ = self.db.update_mint_maker_pair_status(pair.id, "StopLoss").await;
                return;
            }
        };

        let private_key = match self.key_store.get_key(wallet_address).await {
            Some(pk) => pk,
            None => {
                warn!("MintMaker: No private key for stop loss on pair {}", pair.id);
                let _ = self.db.update_mint_maker_pair_status(pair.id, "StopLoss").await;
                return;
            }
        };

        // Sell aggressively: 5 cents below current price (minimum 1 cent)
        let sell_price = (current_price - Decimal::from_str("0.05").unwrap())
            .max(Decimal::from_str("0.01").unwrap());

        match order_manager::place_gtc_sell(&private_key, token_id, sell_price, shares).await {
            Ok(sell_order_id) => {
                info!(
                    "MintMaker: Stop loss sell placed for pair {} — {} {}@{} order={}",
                    pair.id, filled_side, shares, sell_price, sell_order_id
                );
                // Store the sell order ID so we can track its fill status
                let _ = self.db.set_mint_maker_stop_loss_order(pair.id, &sell_order_id, "StopLoss").await;
                let loss = (fill_price - sell_price) * shares;
                let _ = self.db.log_mint_maker_action(
                    wallet_address,
                    "stop_loss",
                    Some(&pair.market_id),
                    Some(&pair.question),
                    Some(&pair.asset),
                    Some(&fill_price.to_string()),
                    Some(&sell_price.to_string()),
                    None,
                    Some(&format!("-{}", loss)),
                    Some(&shares.to_string()),
                    Some(&format!(
                        "STOP LOSS {} filled@{} sell@{} order={}",
                        filled_side, fill_price, sell_price, sell_order_id
                    )),
                ).await;
            }
            Err(e) => {
                warn!("MintMaker: Stop loss sell FAILED for pair {}: {}. Marking for auto-redeem.", pair.id, e);
                let _ = self.db.update_mint_maker_pair_status(pair.id, "StopLoss").await;
            }
        }
    }

    /// Check if a stop loss sell order has filled
    async fn check_stop_loss_fill(
        &self,
        wallet_address: &str,
        pair: &crate::db::MintMakerPairRow,
        sell_order_id: &str,
        api_key: &str,
        api_secret: &str,
        api_passphrase: &str,
    ) {
        let result = order_manager::check_order_status(
            wallet_address,
            sell_order_id,
            api_key,
            api_secret,
            api_passphrase,
        ).await;

        match result {
            Ok(r) => {
                match r.fill_status {
                    FillStatus::Filled => {
                        let sell_price = r.fill_price.as_deref().unwrap_or("?");
                        info!(
                            "MintMaker: Stop loss SELL FILLED for pair {} — order={} price={} matched={}",
                            pair.id, &sell_order_id[..16.min(sell_order_id.len())], sell_price, r.size_matched
                        );
                        let _ = self.db.update_mint_maker_pair_status(pair.id, "StopLossFilled").await;
                        let _ = self.db.log_mint_maker_action(
                            wallet_address,
                            "stop_loss_filled",
                            Some(&pair.market_id),
                            Some(&pair.question),
                            Some(&pair.asset),
                            None,
                            None,
                            None,
                            None,
                            Some(&r.size_matched),
                            Some(&format!("Sell filled@{} order={}", sell_price, &sell_order_id[..16.min(sell_order_id.len())])),
                        ).await;
                    }
                    FillStatus::Cancelled => {
                        // Sell was cancelled (expired or by CLOB) — auto-redeem will handle it
                        warn!(
                            "MintMaker: Stop loss sell CANCELLED for pair {} — leaving for auto-redeem",
                            pair.id
                        );
                        let _ = self.db.log_mint_maker_action(
                            wallet_address,
                            "stop_loss_sell_cancelled",
                            Some(&pair.market_id),
                            Some(&pair.question),
                            Some(&pair.asset),
                            None, None, None, None,
                            Some(&pair.size),
                            Some(&format!("Sell order {} cancelled — auto-redeem fallback", &sell_order_id[..16.min(sell_order_id.len())])),
                        ).await;
                    }
                    FillStatus::PartiallyFilled => {
                        debug!(
                            "MintMaker: Stop loss sell partially filled for pair {} — matched={}",
                            pair.id, r.size_matched
                        );
                    }
                    _ => {
                        // Still open or unknown — check again next cycle
                        debug!(
                            "MintMaker: Stop loss sell still open for pair {} — status={:?}",
                            pair.id, r.fill_status
                        );
                    }
                }
            }
            Err(e) => {
                debug!("MintMaker: Failed to check stop loss sell for pair {}: {}", pair.id, e);
            }
        }
    }

    /// Check fill status of a pair's orders
    async fn check_pair_fills(
        &self,
        wallet_address: &str,
        pair: &crate::db::MintMakerPairRow,
        api_key: &str,
        api_secret: &str,
        api_passphrase: &str,
    ) {
        let yes_result = order_manager::check_order_status(
            wallet_address,
            &pair.yes_order_id,
            api_key,
            api_secret,
            api_passphrase,
        )
        .await;

        let no_result = order_manager::check_order_status(
            wallet_address,
            &pair.no_order_id,
            api_key,
            api_secret,
            api_passphrase,
        )
        .await;

        // Extract fill status, treating API errors as Unknown (not Cancelled)
        let (yes_status, yes_price, yes_matched) = match &yes_result {
            Ok(r) => (r.fill_status, r.fill_price.clone(), r.size_matched.clone()),
            Err(_) => (FillStatus::Unknown, None, "0".to_string()),
        };
        let (no_status, no_price, no_matched) = match &no_result {
            Ok(r) => (r.fill_status, r.fill_price.clone(), r.size_matched.clone()),
            Err(_) => (FillStatus::Unknown, None, "0".to_string()),
        };

        let yes_filled = yes_status == FillStatus::Filled;
        let no_filled = no_status == FillStatus::Filled;
        let yes_cancelled = yes_status == FillStatus::Cancelled;
        let no_cancelled = no_status == FillStatus::Cancelled;

        if yes_filled && no_filled {
            // Both filled - update to Matched, ready for merge.
            // Cap merge size at min(yes_matched, no_matched) to avoid merging
            // more than actually filled.
            let yes_sz: f64 = yes_matched.parse().unwrap_or(0.0);
            let no_sz: f64 = no_matched.parse().unwrap_or(0.0);
            let actual_merge_size = yes_sz.min(no_sz);
            let orig_size: f64 = pair.size.parse().unwrap_or(0.0);

            if actual_merge_size > 0.0 && actual_merge_size < orig_size {
                let new_size = format!("{}", actual_merge_size);
                warn!(
                    "MintMaker: Pair {} fill size mismatch — ordered {} but matched YES={} NO={}. Capping merge to {}",
                    pair.id, pair.size, yes_matched, no_matched, new_size
                );
                let _ = self.db.update_mint_maker_pair_size(pair.id, &new_size).await;
            }

            let _ = self
                .db
                .update_mint_maker_pair_fill(
                    pair.id,
                    yes_price.as_deref(),
                    no_price.as_deref(),
                    "Matched",
                )
                .await;
            let _ = self
                .db
                .log_mint_maker_action(
                    wallet_address,
                    "matched",
                    Some(&pair.market_id),
                    Some(&pair.question),
                    Some(&pair.asset),
                    yes_price.as_deref(),
                    no_price.as_deref(),
                    None,
                    None,
                    Some(&pair.size),
                    Some(&format!("Both sides filled (YES={} NO={})", yes_matched, no_matched)),
                )
                .await;
            info!("MintMaker: Pair {} fully matched (YES={} NO={})", pair.id, yes_matched, no_matched);
        } else if (yes_filled && no_cancelled) || (no_filled && yes_cancelled) {
            // One side filled, other side cancelled by CLOB.
            // The filled side is a REAL position — mark as Orphaned so the user can see it.
            let which_filled = if yes_filled { "YES" } else { "NO" };
            let which_cancelled = if yes_cancelled { "YES" } else { "NO" };
            warn!(
                "MintMaker: Pair {} - {} filled, {} cancelled by CLOB — marking Orphaned",
                pair.id, which_filled, which_cancelled
            );

            // Record the fill price for the filled side
            let _ = self
                .db
                .update_mint_maker_pair_fill(
                    pair.id,
                    if yes_filled { yes_price.as_deref() } else { None },
                    if no_filled { no_price.as_deref() } else { None },
                    "Orphaned",
                )
                .await;

            let _ = self
                .db
                .log_mint_maker_action(
                    wallet_address,
                    "orphaned",
                    Some(&pair.market_id),
                    Some(&pair.question),
                    Some(&pair.asset),
                    if yes_filled { yes_price.as_deref() } else { None },
                    if no_filled { no_price.as_deref() } else { None },
                    None,
                    None,
                    Some(&pair.size),
                    Some(&format!("{} filled, {} cancelled by CLOB — orphaned position", which_filled, which_cancelled)),
                )
                .await;
        } else if yes_filled || no_filled {
            // One side filled, other still open — mark as HalfFilled.
            // Only update if not already HalfFilled to avoid resetting updated_at
            // (which is used for the stop loss grace period timer).
            if pair.status != "HalfFilled" {
                let _ = self
                    .db
                    .update_mint_maker_pair_fill(
                        pair.id,
                        if yes_filled {
                            yes_price.as_deref()
                        } else {
                            None
                        },
                        if no_filled {
                            no_price.as_deref()
                        } else {
                            None
                        },
                        "HalfFilled",
                    )
                    .await;
            }
            debug!(
                "MintMaker: Pair {} half-filled (yes={:?}, no={:?})",
                pair.id, yes_status, no_status
            );
        } else if yes_cancelled && no_cancelled {
            // Both sides cancelled (e.g. by CLOB) — mark pair as cancelled
            let _ = self
                .db
                .update_mint_maker_pair_status(pair.id, "Cancelled")
                .await;
            let _ = self
                .db
                .log_mint_maker_action(
                    wallet_address,
                    "both_cancelled",
                    Some(&pair.market_id),
                    Some(&pair.question),
                    Some(&pair.asset),
                    None,
                    None,
                    None,
                    None,
                    Some(&pair.size),
                    Some("Both orders cancelled by CLOB"),
                )
                .await;
            info!("MintMaker: Pair {} both sides cancelled", pair.id);
        }
        // else: both still open or unknown — do nothing, check again next cycle
    }

    /// Build status update for broadcast
    async fn build_status_update(
        &self,
        eligible_markets: &[MintMakerMarket],
        wallet_address: &str,
    ) -> anyhow::Result<MintMakerStatusUpdate> {
        let settings = if !wallet_address.is_empty() {
            Some(self.db.get_mint_maker_settings(wallet_address).await?)
        } else {
            None
        };

        let enabled = settings.as_ref().map(|s| s.enabled).unwrap_or(false);

        // Build market statuses
        let strategy = MintMakerStrategy::new(self.config.clone());
        let mut market_statuses = Vec::new();
        for market in eligible_markets {
            let open_pairs = if !wallet_address.is_empty() {
                self.db
                    .count_mint_maker_open_pairs_for_market(wallet_address, &market.market_id)
                    .await
                    .unwrap_or(0)
            } else {
                0
            };

            let (yes_bid, no_bid, spread_profit) =
                match strategy.calculate_bids(market, Decimal::ONE) {
                    Some((y, n)) => (
                        Some(y.to_string()),
                        Some(n.to_string()),
                        Some((Decimal::ONE - y - n).to_string()),
                    ),
                    None => (None, None, None),
                };

            market_statuses.push(MintMakerMarketStatus {
                market_id: market.market_id.clone(),
                condition_id: market.condition_id.clone(),
                question: market.question.clone(),
                asset: market.asset.to_string(),
                yes_token_id: market.yes_token_id.clone(),
                no_token_id: market.no_token_id.clone(),
                yes_price: market.yes_price.to_string(),
                no_price: market.no_price.to_string(),
                yes_bid,
                no_bid,
                spread_profit,
                slug: market.slug.clone(),
                minutes_left: market.minutes_to_close,
                open_pairs,
            });
        }

        // Stats
        let stats = if !wallet_address.is_empty() {
            let (total, merged, cancelled, profit, cost, avg_spread) = self
                .db
                .get_mint_maker_stats(wallet_address)
                .await
                .unwrap_or((0, 0, 0, 0.0, 0.0, 0.0));
            let fill_rate = if total > 0 {
                merged as f64 / total as f64
            } else {
                0.0
            };
            MintMakerStatsSnapshot {
                total_pairs: total,
                merged_pairs: merged,
                cancelled_pairs: cancelled,
                total_profit: format!("{:.4}", profit),
                total_cost: format!("{:.4}", cost),
                avg_spread: format!("{:.4}", avg_spread),
                fill_rate: format!("{:.2}", fill_rate * 100.0),
            }
        } else {
            MintMakerStatsSnapshot::default()
        };

        // Open pairs
        let open_pairs = if !wallet_address.is_empty() {
            self.db
                .get_mint_maker_open_pairs(wallet_address)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        // Recent log
        let recent_log = if !wallet_address.is_empty() {
            self.db
                .get_mint_maker_log(wallet_address, 20)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(MintMakerStatusUpdate {
            enabled,
            active_markets: market_statuses,
            stats,
            open_pairs,
            recent_log,
            settings,
        })
    }

    /// Check for resolved markets and redeem winning tokens via CTF relay
    async fn check_auto_redeem(
        &self,
        wallet_address: &str,
    ) -> anyhow::Result<()> {
        let pairs = self.db.get_mint_maker_redeemable_pairs(wallet_address).await?;
        if pairs.is_empty() {
            return Ok(());
        }

        let private_key = match self.key_store.get_key(wallet_address).await {
            Some(pk) => pk,
            None => {
                debug!("MintMaker auto-redeem: no private key for {}, skipping", &wallet_address[..8]);
                return Ok(());
            }
        };

        let (bk, bs, bp) = match (
            std::env::var("POLY_BUILDER_API_KEY").ok(),
            std::env::var("POLY_BUILDER_SECRET").ok(),
            std::env::var("POLY_BUILDER_PASSPHRASE").ok(),
        ) {
            (Some(bk), Some(bs), Some(bp)) => (bk, bs, bp),
            _ => {
                debug!("MintMaker auto-redeem: POLY_BUILDER_* env vars not set, skipping");
                return Ok(());
            }
        };

        // Deduplicate by condition_id — multiple pairs can share the same market
        let mut checked_conditions: HashSet<String> = HashSet::new();

        for pair in &pairs {
            if checked_conditions.contains(&pair.condition_id) {
                continue;
            }
            checked_conditions.insert(pair.condition_id.clone());

            // Check if market is resolved via Gamma API
            let resolved = is_market_resolved(&self.client, &pair.condition_id).await;
            if !resolved {
                continue;
            }

            info!(
                "MintMaker auto-redeem: market {} resolved, redeeming for {}",
                &pair.condition_id[..10], &wallet_address[..8]
            );

            let ctf = crate::services::CtfService::new();
            match ctf.redeem(
                &pair.condition_id,
                &[1, 2], // both YES and NO index sets
                &private_key,
                &bk,
                &bs,
                &bp,
                pair.neg_risk,
            ).await {
                Ok(resp) if resp.success => {
                    let tx_id = resp.transaction_id.unwrap_or_else(|| "unknown".to_string());
                    info!(
                        "MintMaker auto-redeem: condition {} redeemed, tx: {}",
                        &pair.condition_id[..10], tx_id
                    );

                    // Update ALL pairs for this condition_id to Redeemed
                    for p in pairs.iter().filter(|p| p.condition_id == pair.condition_id) {
                        let _ = self.db.update_mint_maker_pair_status(p.id, "Redeemed").await;
                        let _ = self.db.log_mint_maker_action(
                            wallet_address,
                            "auto_redeem",
                            Some(&p.market_id),
                            Some(&p.question),
                            Some(&p.asset),
                            None,
                            None,
                            p.pair_cost.as_deref(),
                            None,
                            Some(&p.size),
                            Some(&format!("tx: {}", tx_id)),
                        ).await;
                    }
                }
                Ok(resp) => {
                    let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
                    warn!(
                        "MintMaker auto-redeem failed for condition {}: {}",
                        &pair.condition_id[..10], err
                    );
                }
                Err(e) => {
                    warn!(
                        "MintMaker auto-redeem error for condition {}: {}",
                        &pair.condition_id[..10], e
                    );
                }
            }
        }

        Ok(())
    }
}

/// Check if a market condition has been resolved on-chain.
///
/// Calls `payoutDenominator(bytes32)` on the CTF contract — returns > 0 if resolved.
/// This is authoritative (the Gamma API's `resolved` field is unreliable for 15-min markets).
async fn is_market_resolved(client: &reqwest::Client, condition_id: &str) -> bool {
    const CTF_ADDRESS: &str = "0x4d97dcd97ec945f40cf65f87097ace5ea0476045";
    // payoutDenominator(bytes32) selector = 0xdd34de67
    const SELECTOR: &str = "dd34de67";
    const RPC_URL: &str = "https://polygon-rpc.com";

    let cond_hex = condition_id.strip_prefix("0x").unwrap_or(condition_id);
    if cond_hex.len() != 64 {
        return false;
    }

    let calldata = format!("0x{}{}", SELECTOR, cond_hex);
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": CTF_ADDRESS, "data": calldata}, "latest"],
        "id": 1
    });

    match client.post(RPC_URL).json(&payload).send().await {
        Ok(resp) => {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                if let Some(result) = data.get("result").and_then(|v| v.as_str()) {
                    // payoutDenominator > 0 means condition is resolved
                    let val = u64::from_str_radix(result.trim_start_matches("0x"), 16).unwrap_or(0);
                    return val > 0;
                }
            }
            false
        }
        Err(e) => {
            debug!("payoutDenominator RPC error for {}: {}", &condition_id[..10], e);
            false
        }
    }
}
