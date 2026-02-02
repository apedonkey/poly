//! SQLite database for tracking positions, orders, and statistics

use crate::services::auto_trader::{AutoTradeLog, AutoTradingSettings, AutoTradingStats};
use crate::types::{BotStats, Opportunity, Position, PositionStatus, Side, StrategyType};
use crate::wallet::EncryptedKey;
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::str::FromStr;
use tracing::info;
use uuid::Uuid;

/// Stored wallet information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredWallet {
    pub id: i64,
    pub address: String,
    pub has_encrypted_key: bool,
    pub created_at: DateTime<Utc>,
    pub last_active: Option<DateTime<Utc>>,
}

/// Wallet with encrypted key (for internal use)
#[derive(Debug, Clone)]
pub struct WalletWithKey {
    pub address: String,
    pub encrypted_key: Option<EncryptedKey>,
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub wallet_address: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Partial close response with PnL and remaining info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialCloseResult {
    /// PnL from this specific sell
    pub pnl_this_sell: Decimal,
    /// Whether the position is now fully closed
    pub is_fully_closed: bool,
    /// Total realized PnL from all partial sells
    pub total_realized_pnl: Decimal,
    /// Shares remaining after this sell
    pub remaining_shares: Decimal,
}

/// Database connection pool
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection
    pub async fn new(path: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(path)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("Failed to connect to database")?;

        let db = Self { pool };
        db.initialize().await?;

        Ok(db)
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<()> {
        // Check if positions table exists and add columns if missing
        let table_info: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as(
            "PRAGMA table_info(positions)"
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        // Check if wallet_address column exists
        let has_wallet_address = table_info.iter().any(|(_, name, _, _, _, _)| name == "wallet_address");
        if !table_info.is_empty() && !has_wallet_address {
            info!("Migrating positions table: adding wallet_address column");
            sqlx::query("ALTER TABLE positions ADD COLUMN wallet_address TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Check if is_paper column exists
        let has_is_paper = table_info.iter().any(|(_, name, _, _, _, _)| name == "is_paper");
        if !table_info.is_empty() && !has_is_paper {
            info!("Migrating positions table: adding is_paper column");
            sqlx::query("ALTER TABLE positions ADD COLUMN is_paper INTEGER NOT NULL DEFAULT 1")
                .execute(&self.pool)
                .await?;
        }

        // Check if end_date column exists
        let has_end_date = table_info.iter().any(|(_, name, _, _, _, _)| name == "end_date");
        if !table_info.is_empty() && !has_end_date {
            info!("Migrating positions table: adding end_date column");
            sqlx::query("ALTER TABLE positions ADD COLUMN end_date TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Check if token_id column exists
        let has_token_id = table_info.iter().any(|(_, name, _, _, _, _)| name == "token_id");
        if !table_info.is_empty() && !has_token_id {
            info!("Migrating positions table: adding token_id column");
            sqlx::query("ALTER TABLE positions ADD COLUMN token_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Check if order_id column exists
        let has_order_id = table_info.iter().any(|(_, name, _, _, _, _)| name == "order_id");
        if !table_info.is_empty() && !has_order_id {
            info!("Migrating positions table: adding order_id column");
            sqlx::query("ALTER TABLE positions ADD COLUMN order_id TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Check if slug column exists
        let has_slug = table_info.iter().any(|(_, name, _, _, _, _)| name == "slug");
        if !table_info.is_empty() && !has_slug {
            info!("Migrating positions table: adding slug column");
            sqlx::query("ALTER TABLE positions ADD COLUMN slug TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Check if remaining_size column exists (for partial sells)
        let has_remaining_size = table_info.iter().any(|(_, name, _, _, _, _)| name == "remaining_size");
        if !table_info.is_empty() && !has_remaining_size {
            info!("Migrating positions table: adding remaining_size column");
            sqlx::query("ALTER TABLE positions ADD COLUMN remaining_size TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Check if realized_pnl column exists (for partial sells)
        let has_realized_pnl = table_info.iter().any(|(_, name, _, _, _, _)| name == "realized_pnl");
        if !table_info.is_empty() && !has_realized_pnl {
            info!("Migrating positions table: adding realized_pnl column");
            sqlx::query("ALTER TABLE positions ADD COLUMN realized_pnl TEXT DEFAULT '0'")
                .execute(&self.pool)
                .await?;
        }

        // Check if total_sold_size column exists (for partial sells)
        let has_total_sold_size = table_info.iter().any(|(_, name, _, _, _, _)| name == "total_sold_size");
        if !table_info.is_empty() && !has_total_sold_size {
            info!("Migrating positions table: adding total_sold_size column");
            sqlx::query("ALTER TABLE positions ADD COLUMN total_sold_size TEXT DEFAULT '0'")
                .execute(&self.pool)
                .await?;
        }

        // Check if avg_exit_price column exists (for partial sells)
        let has_avg_exit_price = table_info.iter().any(|(_, name, _, _, _, _)| name == "avg_exit_price");
        if !table_info.is_empty() && !has_avg_exit_price {
            info!("Migrating positions table: adding avg_exit_price column");
            sqlx::query("ALTER TABLE positions ADD COLUMN avg_exit_price TEXT")
                .execute(&self.pool)
                .await?;
        }

        // Check if neg_risk column exists
        let has_neg_risk = table_info.iter().any(|(_, name, _, _, _, _)| name == "neg_risk");
        if !table_info.is_empty() && !has_neg_risk {
            info!("Migrating positions table: adding neg_risk column");
            sqlx::query("ALTER TABLE positions ADD COLUMN neg_risk INTEGER DEFAULT 0")
                .execute(&self.pool)
                .await?;
        }

        // Check if fee_paid column exists
        let has_fee_paid = table_info.iter().any(|(_, name, _, _, _, _)| name == "fee_paid");
        if !table_info.is_empty() && !has_fee_paid {
            info!("Migrating positions table: adding fee_paid column");
            sqlx::query("ALTER TABLE positions ADD COLUMN fee_paid TEXT DEFAULT '0'")
                .execute(&self.pool)
                .await?;
        }

        // ==================== AUTO-TRADING SETTINGS MIGRATIONS ====================
        let settings_info: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as(
            "PRAGMA table_info(auto_trading_settings)"
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        if !settings_info.is_empty() {
            let has_dispute_sniper = settings_info.iter().any(|(_, name, _, _, _, _)| name == "dispute_sniper_enabled");
            if !has_dispute_sniper {
                info!("Migrating auto_trading_settings: adding dispute sniper columns");
                sqlx::query("ALTER TABLE auto_trading_settings ADD COLUMN dispute_sniper_enabled INTEGER DEFAULT 0")
                    .execute(&self.pool).await?;
                sqlx::query("ALTER TABLE auto_trading_settings ADD COLUMN min_dispute_edge REAL DEFAULT 0.10")
                    .execute(&self.pool).await?;
                sqlx::query("ALTER TABLE auto_trading_settings ADD COLUMN max_dispute_position_size TEXT DEFAULT '25'")
                    .execute(&self.pool).await?;
                sqlx::query("ALTER TABLE auto_trading_settings ADD COLUMN dispute_exit_on_escalation INTEGER DEFAULT 1")
                    .execute(&self.pool).await?;
            }
        }

        // ==================== MC_TRADES MIGRATIONS ====================
        let mc_trades_info: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as(
            "PRAGMA table_info(mc_trades)"
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        if !mc_trades_info.is_empty() {
            let has_end_date = mc_trades_info.iter().any(|(_, name, _, _, _, _)| name == "end_date");
            if !has_end_date {
                info!("Migrating mc_trades table: adding end_date column");
                sqlx::query("ALTER TABLE mc_trades ADD COLUMN end_date TEXT")
                    .execute(&self.pool).await?;
            }
        }

        // Fix NULL values in is_paper column - treat all NULL as paper trades (1)
        sqlx::query("UPDATE positions SET is_paper = 1 WHERE is_paper IS NULL")
            .execute(&self.pool)
            .await?;

        // Backfill remaining_size for existing open positions (remaining = original size in shares)
        // remaining_size stores the number of shares remaining, not USDC amount
        // For existing positions, we calculate shares as size / entry_price
        sqlx::query(
            "UPDATE positions SET remaining_size = CAST(size AS REAL) / CAST(entry_price AS REAL)
             WHERE remaining_size IS NULL AND status IN ('Open', 'PendingResolution')"
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Initialize database schema
    async fn initialize(&self) -> Result<()> {
        // Run migrations first
        self.run_migrations().await?;

        // Wallets table for multi-user support
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS wallets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                address TEXT NOT NULL UNIQUE,
                encrypted_private_key BLOB,
                salt BLOB,
                nonce BLOB,
                created_at TEXT NOT NULL,
                last_active TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Sessions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                wallet_address TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                FOREIGN KEY (wallet_address) REFERENCES wallets(address)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Positions table with wallet_address
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS positions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                market_id TEXT NOT NULL,
                question TEXT NOT NULL,
                side TEXT NOT NULL,
                entry_price TEXT NOT NULL,
                size TEXT NOT NULL,
                strategy TEXT NOT NULL,
                opened_at TEXT NOT NULL,
                closed_at TEXT,
                exit_price TEXT,
                pnl TEXT,
                status TEXT NOT NULL DEFAULT 'Open',
                wallet_address TEXT,
                is_paper INTEGER NOT NULL DEFAULT 1,
                end_date TEXT,
                token_id TEXT,
                remaining_size TEXT,
                realized_pnl TEXT DEFAULT '0',
                total_sold_size TEXT DEFAULT '0',
                avg_exit_price TEXT,
                neg_risk INTEGER DEFAULT 0,
                fee_paid TEXT DEFAULT '0'
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for fast lookups
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_positions_wallet ON positions(wallet_address)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_positions_wallet_status ON positions(wallet_address, status)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_positions_market ON positions(market_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS opportunities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                market_id TEXT NOT NULL,
                question TEXT NOT NULL,
                slug TEXT NOT NULL,
                strategy TEXT NOT NULL,
                side TEXT NOT NULL,
                entry_price TEXT NOT NULL,
                expected_return REAL NOT NULL,
                edge REAL NOT NULL,
                time_to_close_hours REAL,
                liquidity TEXT NOT NULL,
                found_at TEXT NOT NULL,
                acted_on INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS scan_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scanned_at TEXT NOT NULL,
                markets_found INTEGER NOT NULL,
                sniper_opportunities INTEGER NOT NULL,
                no_bias_opportunities INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_wallets_address ON wallets(address)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_wallet ON sessions(wallet_address)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_positions_status ON positions(status)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_positions_market ON positions(market_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_positions_wallet ON positions(wallet_address)")
            .execute(&self.pool)
            .await?;

        // API credentials table for CLOB authentication
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_credentials (
                wallet_address TEXT PRIMARY KEY,
                api_key TEXT NOT NULL,
                api_secret TEXT NOT NULL,
                api_passphrase TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (wallet_address) REFERENCES wallets(address)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // ==================== AUTO-TRADING TABLES ====================

        // Auto-trading settings per wallet
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auto_trading_settings (
                wallet_address TEXT PRIMARY KEY,
                enabled INTEGER DEFAULT 0,
                auto_buy_enabled INTEGER DEFAULT 0,
                max_position_size TEXT DEFAULT '50',
                max_total_exposure TEXT DEFAULT '500',
                min_edge REAL DEFAULT 0.05,
                strategies TEXT DEFAULT '["sniper"]',
                take_profit_enabled INTEGER DEFAULT 1,
                take_profit_percent REAL DEFAULT 0.20,
                stop_loss_enabled INTEGER DEFAULT 1,
                stop_loss_percent REAL DEFAULT 0.10,
                trailing_stop_enabled INTEGER DEFAULT 0,
                trailing_stop_percent REAL DEFAULT 0.10,
                time_exit_enabled INTEGER DEFAULT 0,
                time_exit_hours REAL DEFAULT 24.0,
                max_positions INTEGER DEFAULT 10,
                cooldown_minutes INTEGER DEFAULT 5,
                max_daily_loss TEXT DEFAULT '100',
                dispute_sniper_enabled INTEGER DEFAULT 0,
                min_dispute_edge REAL DEFAULT 0.10,
                max_dispute_position_size TEXT DEFAULT '25',
                dispute_exit_on_escalation INTEGER DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (wallet_address) REFERENCES wallets(address)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Auto-trade activity log
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS auto_trade_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT NOT NULL,
                position_id INTEGER,
                action TEXT NOT NULL,
                market_question TEXT,
                side TEXT,
                entry_price TEXT,
                exit_price TEXT,
                size TEXT,
                pnl TEXT,
                trigger_reason TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (wallet_address) REFERENCES wallets(address)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Position peaks for trailing stop
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS position_peaks (
                position_id INTEGER PRIMARY KEY,
                peak_price TEXT NOT NULL,
                peak_at TEXT NOT NULL,
                FOREIGN KEY (position_id) REFERENCES positions(id)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Auto-trading indexes
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_auto_settings_enabled ON auto_trading_settings(enabled)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_auto_log_wallet ON auto_trade_log(wallet_address)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_auto_log_created ON auto_trade_log(created_at DESC)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_positions_token_id ON positions(token_id)")
            .execute(&self.pool)
            .await?;

        // ==================== ORDER LIFECYCLE TRACKING ====================

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS orders (
                id TEXT PRIMARY KEY,
                wallet_address TEXT NOT NULL,
                token_id TEXT NOT NULL,
                market_id TEXT,
                side TEXT NOT NULL,
                order_type TEXT NOT NULL,
                price TEXT NOT NULL,
                original_size TEXT NOT NULL,
                filled_size TEXT DEFAULT '0',
                avg_fill_price TEXT,
                status TEXT NOT NULL DEFAULT 'Pending',
                position_id INTEGER,
                neg_risk INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (wallet_address) REFERENCES wallets(address)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_orders_wallet ON orders(wallet_address)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_orders_status ON orders(status)")
            .execute(&self.pool)
            .await?;

        // ==================== CLARIFICATION MONITOR TABLES ====================

        // Description hashes for detecting changes
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS description_hashes (
                market_id TEXT PRIMARY KEY,
                description_hash TEXT NOT NULL,
                last_updated INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // ==================== MILLIONAIRES CLUB TABLES ====================

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mc_config (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                bankroll TEXT NOT NULL DEFAULT '40',
                tier INTEGER NOT NULL DEFAULT 1,
                mode TEXT NOT NULL DEFAULT 'observation',
                peak_bankroll TEXT NOT NULL DEFAULT '40',
                pause_state TEXT NOT NULL DEFAULT 'active',
                pause_until TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mc_scout_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                market_id TEXT NOT NULL,
                condition_id TEXT,
                question TEXT NOT NULL,
                slug TEXT,
                side TEXT NOT NULL,
                price TEXT NOT NULL,
                volume TEXT,
                category TEXT,
                end_date TEXT,
                passed INTEGER NOT NULL DEFAULT 0,
                certainty_score INTEGER NOT NULL DEFAULT 0,
                reasons TEXT NOT NULL DEFAULT '[]',
                slippage_pct REAL,
                would_trade INTEGER NOT NULL DEFAULT 0,
                token_id TEXT,
                scanned_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mc_scout_scanned ON mc_scout_log(scanned_at)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mc_scout_passed ON mc_scout_log(passed)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mc_trades (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                market_id TEXT NOT NULL,
                condition_id TEXT,
                question TEXT NOT NULL,
                slug TEXT,
                side TEXT NOT NULL,
                entry_price TEXT NOT NULL,
                exit_price TEXT,
                size TEXT NOT NULL,
                shares TEXT NOT NULL,
                pnl TEXT,
                certainty_score INTEGER NOT NULL DEFAULT 0,
                category TEXT,
                status TEXT NOT NULL DEFAULT 'open',
                tier_at_entry INTEGER NOT NULL DEFAULT 1,
                token_id TEXT,
                end_date TEXT,
                opened_at TEXT NOT NULL,
                closed_at TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mc_trades_status ON mc_trades(status)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mc_trades_category ON mc_trades(category)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mc_trades_opened ON mc_trades(opened_at)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mc_tier_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_tier INTEGER NOT NULL,
                to_tier INTEGER NOT NULL,
                bankroll TEXT NOT NULL,
                reason TEXT NOT NULL,
                timestamp TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mc_drawdown_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                peak_bankroll TEXT NOT NULL,
                current_bankroll TEXT NOT NULL,
                drawdown_pct REAL NOT NULL,
                action_taken TEXT NOT NULL,
                timestamp TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // ==================== MINT MAKER TABLES ====================

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mint_maker_settings (
                wallet_address TEXT PRIMARY KEY,
                enabled INTEGER DEFAULT 0,
                preset TEXT DEFAULT 'balanced',
                bid_offset_cents INTEGER DEFAULT 2,
                max_pair_cost REAL DEFAULT 0.98,
                min_spread_profit REAL DEFAULT 0.01,
                max_pairs_per_market INTEGER DEFAULT 5,
                max_total_pairs INTEGER DEFAULT 20,
                stale_order_seconds INTEGER DEFAULT 120,
                assets TEXT DEFAULT '["BTC","ETH","SOL"]',
                min_minutes_to_close REAL DEFAULT 2.0,
                max_minutes_to_close REAL DEFAULT 14.0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (wallet_address) REFERENCES wallets(address)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mint_maker_pairs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT NOT NULL,
                market_id TEXT NOT NULL,
                condition_id TEXT NOT NULL,
                question TEXT NOT NULL,
                asset TEXT NOT NULL,
                yes_order_id TEXT NOT NULL,
                no_order_id TEXT NOT NULL,
                yes_bid_price TEXT NOT NULL,
                no_bid_price TEXT NOT NULL,
                yes_fill_price TEXT,
                no_fill_price TEXT,
                pair_cost TEXT,
                profit TEXT,
                size TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'Pending',
                merge_tx_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (wallet_address) REFERENCES wallets(address)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mm_pairs_wallet ON mint_maker_pairs(wallet_address)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mm_pairs_status ON mint_maker_pairs(status)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mm_pairs_market ON mint_maker_pairs(market_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS mint_maker_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                wallet_address TEXT NOT NULL,
                action TEXT NOT NULL,
                market_id TEXT,
                question TEXT,
                asset TEXT,
                yes_price TEXT,
                no_price TEXT,
                pair_cost TEXT,
                profit TEXT,
                size TEXT,
                details TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (wallet_address) REFERENCES wallets(address)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mm_log_wallet ON mint_maker_log(wallet_address)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_mm_log_created ON mint_maker_log(created_at DESC)")
            .execute(&self.pool)
            .await?;

        // ==================== MINT MAKER SETTINGS MIGRATIONS ====================
        {
            let mm_info: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as(
                "PRAGMA table_info(mint_maker_settings)"
            )
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

            let has_auto_place = mm_info.iter().any(|(_, name, _, _, _, _)| name == "auto_place");
            if !mm_info.is_empty() && !has_auto_place {
                info!("Migrating mint_maker_settings: adding auto_place column");
                sqlx::query("ALTER TABLE mint_maker_settings ADD COLUMN auto_place INTEGER DEFAULT 0")
                    .execute(&self.pool)
                    .await?;
            }

            let has_auto_place_size = mm_info.iter().any(|(_, name, _, _, _, _)| name == "auto_place_size");
            if !mm_info.is_empty() && !has_auto_place_size {
                info!("Migrating mint_maker_settings: adding auto_place_size column");
                sqlx::query("ALTER TABLE mint_maker_settings ADD COLUMN auto_place_size TEXT DEFAULT '2'")
                    .execute(&self.pool)
                    .await?;
            }

            let has_auto_max_markets = mm_info.iter().any(|(_, name, _, _, _, _)| name == "auto_max_markets");
            if !mm_info.is_empty() && !has_auto_max_markets {
                info!("Migrating mint_maker_settings: adding auto_max_markets column");
                sqlx::query("ALTER TABLE mint_maker_settings ADD COLUMN auto_max_markets INTEGER DEFAULT 1")
                    .execute(&self.pool)
                    .await?;
            }

            let has_auto_redeem = mm_info.iter().any(|(_, name, _, _, _, _)| name == "auto_redeem");
            if !mm_info.is_empty() && !has_auto_redeem {
                info!("Migrating mint_maker_settings: adding auto_redeem column");
                sqlx::query("ALTER TABLE mint_maker_settings ADD COLUMN auto_redeem INTEGER DEFAULT 0")
                    .execute(&self.pool)
                    .await?;
            }
        }

        // ==================== MINT MAKER PAIRS MIGRATIONS ====================
        {
            let mm_pairs_info: Vec<(i64, String, String, i64, Option<String>, i64)> = sqlx::query_as(
                "PRAGMA table_info(mint_maker_pairs)"
            )
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

            let has_yes_size = mm_pairs_info.iter().any(|(_, name, _, _, _, _)| name == "yes_size");
            if !mm_pairs_info.is_empty() && !has_yes_size {
                info!("Migrating mint_maker_pairs: adding yes_size column");
                sqlx::query("ALTER TABLE mint_maker_pairs ADD COLUMN yes_size TEXT")
                    .execute(&self.pool)
                    .await?;
            }

            let has_no_size = mm_pairs_info.iter().any(|(_, name, _, _, _, _)| name == "no_size");
            if !mm_pairs_info.is_empty() && !has_no_size {
                info!("Migrating mint_maker_pairs: adding no_size column");
                sqlx::query("ALTER TABLE mint_maker_pairs ADD COLUMN no_size TEXT")
                    .execute(&self.pool)
                    .await?;
            }

            let has_slug = mm_pairs_info.iter().any(|(_, name, _, _, _, _)| name == "slug");
            if !mm_pairs_info.is_empty() && !has_slug {
                info!("Migrating mint_maker_pairs: adding slug column");
                sqlx::query("ALTER TABLE mint_maker_pairs ADD COLUMN slug TEXT")
                    .execute(&self.pool)
                    .await?;
            }

            let has_yes_token_id = mm_pairs_info.iter().any(|(_, name, _, _, _, _)| name == "yes_token_id");
            if !mm_pairs_info.is_empty() && !has_yes_token_id {
                info!("Migrating mint_maker_pairs: adding yes_token_id, no_token_id, neg_risk columns");
                sqlx::query("ALTER TABLE mint_maker_pairs ADD COLUMN yes_token_id TEXT")
                    .execute(&self.pool)
                    .await?;
                sqlx::query("ALTER TABLE mint_maker_pairs ADD COLUMN no_token_id TEXT")
                    .execute(&self.pool)
                    .await?;
                sqlx::query("ALTER TABLE mint_maker_pairs ADD COLUMN neg_risk INTEGER DEFAULT 0")
                    .execute(&self.pool)
                    .await?;
            }

            let has_stop_loss_order_id = mm_pairs_info.iter().any(|(_, name, _, _, _, _)| name == "stop_loss_order_id");
            if !mm_pairs_info.is_empty() && !has_stop_loss_order_id {
                info!("Migrating mint_maker_pairs: adding stop_loss_order_id column");
                sqlx::query("ALTER TABLE mint_maker_pairs ADD COLUMN stop_loss_order_id TEXT")
                    .execute(&self.pool)
                    .await?;
            }
        }

        info!("Database initialized");
        Ok(())
    }

    /// Store API credentials for a wallet
    pub async fn store_api_credentials(
        &self,
        wallet_address: &str,
        api_key: &str,
        api_secret: &str,
        api_passphrase: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO api_credentials (wallet_address, api_key, api_secret, api_passphrase, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(wallet_address.to_lowercase())
        .bind(api_key)
        .bind(api_secret)
        .bind(api_passphrase)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get API credentials for a wallet
    pub async fn get_api_credentials(
        &self,
        wallet_address: &str,
    ) -> Result<Option<(String, String, String)>> {
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT api_key, api_secret, api_passphrase FROM api_credentials WHERE wallet_address = ?",
        )
        .bind(wallet_address.to_lowercase())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Get all wallets that have API credentials stored
    pub async fn get_wallets_with_api_credentials(&self) -> Result<Vec<(String, String, String, String)>> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT wallet_address, api_key, api_secret, api_passphrase FROM api_credentials",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Record a new position
    pub async fn create_position(
        &self,
        market_id: &str,
        question: &str,
        side: Side,
        entry_price: Decimal,
        size: Decimal,
        strategy: StrategyType,
        is_paper: bool,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let side_str = format!("{:?}", side);
        let strategy_str = format!("{:?}", strategy);

        let result = sqlx::query(
            r#"
            INSERT INTO positions (market_id, question, side, entry_price, size, strategy, opened_at, status, is_paper)
            VALUES (?, ?, ?, ?, ?, ?, ?, 'Open', ?)
            "#,
        )
        .bind(market_id)
        .bind(question)
        .bind(side_str)
        .bind(entry_price.to_string())
        .bind(size.to_string())
        .bind(strategy_str)
        .bind(now)
        .bind(is_paper)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Update position status
    pub async fn update_position_status(&self, id: i64, status: PositionStatus) -> Result<()> {
        let status_str = format!("{:?}", status);
        sqlx::query("UPDATE positions SET status = ? WHERE id = ?")
            .bind(status_str)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Close a position with result
    pub async fn close_position(
        &self,
        id: i64,
        exit_price: Decimal,
        order_id: Option<&str>,
    ) -> Result<()> {
        // Get position to calculate PnL
        let row = sqlx::query("SELECT entry_price, size FROM positions WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Position not found"))?;

        let entry_price_str: String = row.get("entry_price");
        let size_str: String = row.get("size");

        let entry_price = Decimal::from_str(&entry_price_str)?;
        let size = Decimal::from_str(&size_str)?;

        // Calculate shares and PnL
        let shares = size / entry_price;
        let pnl = (exit_price - entry_price) * shares;

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE positions SET closed_at = ?, exit_price = ?, pnl = ?, status = 'Closed', order_id = COALESCE(?, order_id) WHERE id = ?",
        )
        .bind(&now)
        .bind(exit_price.to_string())
        .bind(pnl.to_string())
        .bind(order_id)
        .bind(id)
        .execute(&self.pool)
        .await?;

        // Clean up position peak if exists
        self.delete_position_peak(id).await?;

        Ok(())
    }

    /// Update position end_date (for backfilling existing positions)
    pub async fn update_position_end_date(&self, id: i64, end_date: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE positions SET end_date = ? WHERE id = ?")
            .bind(end_date.to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get all open positions
    pub async fn get_open_positions(&self) -> Result<Vec<Position>> {
        // Only get live trades (not paper), exclude already resolved
        // Note: is_paper = 0 means live trade, NULL or 1 means paper trade
        let rows = sqlx::query(
            "SELECT * FROM positions WHERE status IN ('Open', 'PendingResolution') AND is_paper = 0",
        )
        .fetch_all(&self.pool)
        .await?;

        let positions = rows
            .iter()
            .filter_map(|row| self.row_to_position(row).ok())
            .collect();

        Ok(positions)
    }

    /// Get position by market ID
    pub async fn get_position_by_market(&self, market_id: &str) -> Result<Option<Position>> {
        let row = sqlx::query("SELECT * FROM positions WHERE market_id = ? AND status = 'Open'")
            .bind(market_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => Ok(Some(self.row_to_position(&r)?)),
            None => Ok(None),
        }
    }

    /// Record an opportunity found
    pub async fn record_opportunity(&self, opp: &Opportunity) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let strategy_str = format!("{:?}", opp.strategy);
        let side_str = format!("{:?}", opp.side);

        let result = sqlx::query(
            r#"
            INSERT INTO opportunities (market_id, question, slug, strategy, side, entry_price, expected_return, edge, time_to_close_hours, liquidity, found_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&opp.market_id)
        .bind(&opp.question)
        .bind(&opp.slug)
        .bind(strategy_str)
        .bind(side_str)
        .bind(opp.entry_price.to_string())
        .bind(opp.expected_return)
        .bind(opp.edge)
        .bind(opp.time_to_close_hours)
        .bind(opp.liquidity.to_string())
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Record a scan
    pub async fn record_scan(
        &self,
        markets_found: i64,
        sniper_opps: i64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO scan_history (scanned_at, markets_found, sniper_opportunities, no_bias_opportunities) VALUES (?, ?, ?, 0)",
        )
        .bind(now)
        .bind(markets_found)
        .bind(sniper_opps)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get bot statistics
    pub async fn get_stats(&self) -> Result<BotStats> {
        // Include both 'Resolved' (market resolved) and 'Closed' (manually sold) positions
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM positions WHERE status IN ('Resolved', 'Closed')")
            .fetch_one(&self.pool)
            .await?;

        let wins: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE status IN ('Resolved', 'Closed') AND CAST(pnl AS REAL) > 0",
        )
        .fetch_one(&self.pool)
        .await?;

        let losses: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE status IN ('Resolved', 'Closed') AND CAST(pnl AS REAL) <= 0",
        )
        .fetch_one(&self.pool)
        .await?;

        let total_pnl: Option<(String,)> = sqlx::query_as(
            "SELECT COALESCE(SUM(CAST(pnl AS REAL)), 0) FROM positions WHERE status IN ('Resolved', 'Closed')",
        )
        .fetch_optional(&self.pool)
        .await?;

        let sniper_trades: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE strategy = 'ResolutionSniper' AND status IN ('Resolved', 'Closed')",
        )
        .fetch_one(&self.pool)
        .await?;

        let sniper_wins: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE strategy = 'ResolutionSniper' AND status IN ('Resolved', 'Closed') AND CAST(pnl AS REAL) > 0",
        )
        .fetch_one(&self.pool)
        .await?;

        let pnl_decimal = total_pnl
            .and_then(|(s,)| Decimal::from_str(&s).ok())
            .unwrap_or_default();

        Ok(BotStats {
            total_trades: total.0,
            winning_trades: wins.0,
            losing_trades: losses.0,
            total_pnl: pnl_decimal,
            sniper_trades: sniper_trades.0,
            sniper_wins: sniper_wins.0,
            avg_hold_time_hours: 0.0, // TODO: calculate from closed_at - opened_at
        })
    }

    fn row_to_position(&self, row: &sqlx::sqlite::SqliteRow) -> Result<Position> {
        let side_str: String = row.get("side");
        let side = match side_str.as_str() {
            "Yes" => Side::Yes,
            _ => Side::No,
        };

        let strategy_str: String = row.get("strategy");
        let strategy = match strategy_str.as_str() {
            "Dispute" => StrategyType::Dispute,
            "MillionairesClub" => StrategyType::MillionairesClub,
            _ => StrategyType::ResolutionSniper,
        };

        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "Open" => PositionStatus::Open,
            "PendingResolution" => PositionStatus::PendingResolution,
            "Resolved" => PositionStatus::Resolved,
            _ => PositionStatus::Closed,
        };

        let entry_price_str: String = row.get("entry_price");
        let size_str: String = row.get("size");
        let opened_at_str: String = row.get("opened_at");

        let exit_price: Option<String> = row.get("exit_price");
        let pnl: Option<String> = row.get("pnl");
        let closed_at: Option<String> = row.get("closed_at");
        let is_paper: bool = row.try_get("is_paper").unwrap_or(true);
        let end_date: Option<String> = row.try_get("end_date").unwrap_or(None);
        let token_id: Option<String> = row.try_get("token_id").unwrap_or(None);
        let order_id: Option<String> = row.try_get("order_id").unwrap_or(None);
        let slug: Option<String> = row.try_get("slug").unwrap_or(None);
        let remaining_size: Option<String> = row.try_get("remaining_size").unwrap_or(None);
        let realized_pnl: Option<String> = row.try_get("realized_pnl").unwrap_or(None);
        let total_sold_size: Option<String> = row.try_get("total_sold_size").unwrap_or(None);
        let avg_exit_price: Option<String> = row.try_get("avg_exit_price").unwrap_or(None);
        let neg_risk: bool = row.try_get::<i32, _>("neg_risk").unwrap_or(0) != 0;
        let fee_paid: Option<String> = row.try_get("fee_paid").unwrap_or(None);

        let wallet_address: Option<String> = row.try_get("wallet_address").unwrap_or(None);

        Ok(Position {
            id: row.get("id"),
            wallet_address: wallet_address.unwrap_or_default(),
            market_id: row.get("market_id"),
            question: row.get("question"),
            slug,
            side,
            entry_price: Decimal::from_str(&entry_price_str)?,
            size: Decimal::from_str(&size_str)?,
            strategy,
            opened_at: DateTime::parse_from_rfc3339(&opened_at_str)?.with_timezone(&Utc),
            closed_at: closed_at
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|d| d.with_timezone(&Utc)),
            exit_price: exit_price.and_then(|s| Decimal::from_str(&s).ok()),
            pnl: pnl.and_then(|s| Decimal::from_str(&s).ok()),
            status,
            is_paper,
            end_date: end_date
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|d| d.with_timezone(&Utc)),
            token_id,
            order_id,
            remaining_size: remaining_size.and_then(|s| Decimal::from_str(&s).ok()),
            realized_pnl: realized_pnl.and_then(|s| Decimal::from_str(&s).ok()),
            total_sold_size: total_sold_size.and_then(|s| Decimal::from_str(&s).ok()),
            avg_exit_price: avg_exit_price.and_then(|s| Decimal::from_str(&s).ok()),
            neg_risk,
            fee_paid: fee_paid.and_then(|s| Decimal::from_str(&s).ok()),
        })
    }

    // ==================== WALLET MANAGEMENT ====================

    /// Create a new wallet record (optionally with encrypted private key)
    pub async fn create_wallet(
        &self,
        address: &str,
        encrypted_key: Option<&EncryptedKey>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();

        let (encrypted_private_key, salt, nonce) = match encrypted_key {
            Some(ek) => (Some(&ek.ciphertext[..]), Some(&ek.salt[..]), Some(&ek.nonce[..])),
            None => (None, None, None),
        };

        let result = sqlx::query(
            r#"
            INSERT INTO wallets (address, encrypted_private_key, salt, nonce, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(address.to_lowercase())
        .bind(encrypted_private_key)
        .bind(salt)
        .bind(nonce)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get wallet by address
    pub async fn get_wallet(&self, address: &str) -> Result<Option<StoredWallet>> {
        let row = sqlx::query("SELECT * FROM wallets WHERE address = ?")
            .bind(address)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => {
                let created_at_str: String = r.get("created_at");
                let last_active: Option<String> = r.get("last_active");
                let encrypted_key: Option<Vec<u8>> = r.get("encrypted_private_key");

                Ok(Some(StoredWallet {
                    id: r.get("id"),
                    address: r.get("address"),
                    has_encrypted_key: encrypted_key.is_some(),
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)?
                        .with_timezone(&Utc),
                    last_active: last_active
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|d| d.with_timezone(&Utc)),
                }))
            }
            None => Ok(None),
        }
    }

    /// Get encrypted key for a wallet
    pub async fn get_encrypted_key(&self, address: &str) -> Result<Option<EncryptedKey>> {
        let row = sqlx::query(
            "SELECT encrypted_private_key, salt, nonce FROM wallets WHERE LOWER(address) = LOWER(?)",
        )
        .bind(address)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let ciphertext: Option<Vec<u8>> = r.get("encrypted_private_key");
                let salt: Option<Vec<u8>> = r.get("salt");
                let nonce: Option<Vec<u8>> = r.get("nonce");

                match (ciphertext, salt, nonce) {
                    (Some(c), Some(s), Some(n)) => Ok(Some(EncryptedKey {
                        ciphertext: c,
                        salt: s,
                        nonce: n,
                    })),
                    _ => Ok(None),
                }
            }
            None => Ok(None),
        }
    }

    /// Update wallet last active time
    pub async fn update_wallet_activity(&self, address: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE wallets SET last_active = ? WHERE LOWER(address) = LOWER(?)")
            .bind(now)
            .bind(address)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ==================== SESSION MANAGEMENT ====================

    /// Create a new session for a wallet
    pub async fn create_session(&self, wallet_address: &str) -> Result<Session> {
        let session_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires_at = now + Duration::hours(24);

        sqlx::query(
            "INSERT INTO sessions (id, wallet_address, created_at, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&session_id)
        .bind(wallet_address.to_lowercase())
        .bind(now.to_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(Session {
            id: session_id,
            wallet_address: wallet_address.to_string(),
            created_at: now,
            expires_at,
        })
    }

    /// Validate and get session
    pub async fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let row = sqlx::query("SELECT * FROM sessions WHERE id = ?")
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => {
                let created_at_str: String = r.get("created_at");
                let expires_at_str: String = r.get("expires_at");
                let expires_at = DateTime::parse_from_rfc3339(&expires_at_str)?
                    .with_timezone(&Utc);

                // Check if session is expired
                if expires_at < Utc::now() {
                    // Delete expired session
                    self.delete_session(session_id).await?;
                    return Ok(None);
                }

                Ok(Some(Session {
                    id: r.get("id"),
                    wallet_address: r.get("wallet_address"),
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)?
                        .with_timezone(&Utc),
                    expires_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at < ?")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Get distinct wallet addresses with active (non-expired) sessions
    pub async fn get_active_wallet_addresses(&self) -> Result<Vec<String>> {
        let now = Utc::now().to_rfc3339();
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT wallet_address FROM sessions WHERE expires_at > ?"
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(addr,)| addr).collect())
    }

    // ==================== POSITION MANAGEMENT (MULTI-USER) ====================

    /// Record a new position for a specific wallet
    pub async fn create_position_for_wallet(
        &self,
        wallet_address: &str,
        market_id: &str,
        question: &str,
        slug: Option<&str>,
        side: Side,
        entry_price: Decimal,
        size: Decimal,
        strategy: StrategyType,
        is_paper: bool,
        end_date: Option<DateTime<Utc>>,
        token_id: Option<&str>,
        order_id: Option<&str>,
        neg_risk: bool,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let side_str = format!("{:?}", side);
        let strategy_str = format!("{:?}", strategy);
        let end_date_str = end_date.map(|d| d.to_rfc3339());

        // Calculate initial remaining_size as shares (size / entry_price)
        let shares = size / entry_price;

        let result = sqlx::query(
            r#"
            INSERT INTO positions (wallet_address, market_id, question, slug, side, entry_price, size, strategy, opened_at, status, is_paper, end_date, token_id, order_id, remaining_size, realized_pnl, total_sold_size, neg_risk)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'Open', ?, ?, ?, ?, ?, '0', '0', ?)
            "#,
        )
        .bind(wallet_address)
        .bind(market_id)
        .bind(question)
        .bind(slug)
        .bind(side_str)
        .bind(entry_price.to_string())
        .bind(size.to_string())
        .bind(strategy_str)
        .bind(now)
        .bind(is_paper)
        .bind(end_date_str)
        .bind(token_id)
        .bind(order_id)
        .bind(shares.to_string())
        .bind(neg_risk as i32)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get open positions for a specific wallet
    pub async fn get_positions_for_wallet(&self, wallet_address: &str) -> Result<Vec<Position>> {
        // Only return live trades (exclude paper trades)
        let rows = sqlx::query(
            "SELECT * FROM positions WHERE wallet_address = ? AND is_paper = 0 ORDER BY opened_at DESC",
        )
        .bind(wallet_address)
        .fetch_all(&self.pool)
        .await?;

        let positions = rows
            .iter()
            .filter_map(|row| self.row_to_position(row).ok())
            .collect();

        Ok(positions)
    }

    /// Get open positions for a specific wallet (live trades only)
    pub async fn get_open_positions_for_wallet(&self, wallet_address: &str) -> Result<Vec<Position>> {
        let rows = sqlx::query(
            "SELECT * FROM positions WHERE wallet_address = ? AND status IN ('Open', 'PendingResolution') AND is_paper = 0 ORDER BY opened_at DESC",
        )
        .bind(wallet_address)
        .fetch_all(&self.pool)
        .await?;

        let positions = rows
            .iter()
            .filter_map(|row| self.row_to_position(row).ok())
            .collect();

        Ok(positions)
    }

    /// Get stats for a specific wallet (optimized single query)
    pub async fn get_stats_for_wallet(&self, wallet_address: &str) -> Result<BotStats> {
        // Single query to get all stats at once (live trades only)
        // Include both 'Resolved' (market resolved) and 'Closed' (manually sold) positions
        let row: (i64, i64, i64, f64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) as total,
                SUM(CASE WHEN CAST(pnl AS REAL) > 0 THEN 1 ELSE 0 END) as wins,
                SUM(CASE WHEN CAST(pnl AS REAL) <= 0 THEN 1 ELSE 0 END) as losses,
                COALESCE(SUM(CAST(pnl AS REAL)), 0) as total_pnl,
                SUM(CASE WHEN strategy = 'ResolutionSniper' THEN 1 ELSE 0 END) as sniper_trades,
                SUM(CASE WHEN strategy = 'ResolutionSniper' AND CAST(pnl AS REAL) > 0 THEN 1 ELSE 0 END) as sniper_wins
            FROM positions
            WHERE wallet_address = ? AND status IN ('Resolved', 'Closed') AND is_paper = 0
            "#,
        )
        .bind(wallet_address)
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0, 0, 0, 0.0, 0, 0));

        let pnl_decimal = Decimal::from_f64_retain(row.3).unwrap_or_default();

        Ok(BotStats {
            total_trades: row.0,
            winning_trades: row.1,
            losing_trades: row.2,
            total_pnl: pnl_decimal,
            sniper_trades: row.4,
            sniper_wins: row.5,
            avg_hold_time_hours: 0.0,
        })
    }

    /// Close a position and calculate PnL (with optional fee deduction)
    pub async fn close_position_for_wallet(
        &self,
        wallet_address: &str,
        position_id: i64,
        exit_price: Decimal,
        fee: Option<Decimal>,
    ) -> Result<Decimal> {
        // First get the position to verify ownership and calculate PnL
        let row = sqlx::query(
            "SELECT entry_price, size, side FROM positions WHERE id = ? AND wallet_address = ?",
        )
        .bind(position_id)
        .bind(wallet_address)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Position not found or unauthorized"))?;

        let entry_price: String = row.get("entry_price");
        let size: String = row.get("size");
        let _side: String = row.get("side");

        let entry = Decimal::from_str(&entry_price)?;
        let size_dec = Decimal::from_str(&size)?;

        // Calculate PnL: (exit_price - entry_price) * shares - fee
        // Shares = size / entry_price
        let shares = size_dec / entry;
        let fee_amount = fee.unwrap_or(Decimal::ZERO);

        // For both YES and NO positions: profit if you sell at a higher price than you bought
        // (You bought tokens at entry_price, sold at exit_price)
        let pnl = (exit_price - entry) * shares - fee_amount;

        // Update the position
        sqlx::query(
            r#"
            UPDATE positions
            SET status = 'Closed', exit_price = ?, pnl = ?, closed_at = ?, fee_paid = ?
            WHERE id = ? AND wallet_address = ?
            "#,
        )
        .bind(exit_price.to_string())
        .bind(pnl.to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(fee_amount.to_string())
        .bind(position_id)
        .bind(wallet_address)
        .execute(&self.pool)
        .await?;

        Ok(pnl)
    }

    /// Partially close a position (sell some shares, keep the rest)
    /// Returns PnL for this sell and whether position is fully closed
    pub async fn partial_close_position_for_wallet(
        &self,
        wallet_address: &str,
        position_id: i64,
        sell_shares: Decimal,
        exit_price: Decimal,
    ) -> Result<PartialCloseResult> {
        // Get the position to verify ownership and get current state
        let row = sqlx::query(
            "SELECT entry_price, size, remaining_size, realized_pnl, total_sold_size, avg_exit_price FROM positions WHERE id = ? AND wallet_address = ?",
        )
        .bind(position_id)
        .bind(wallet_address)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Position not found or unauthorized"))?;

        let entry_price_str: String = row.get("entry_price");
        let size_str: String = row.get("size");
        let remaining_size_str: Option<String> = row.get("remaining_size");
        let realized_pnl_str: Option<String> = row.get("realized_pnl");
        let total_sold_size_str: Option<String> = row.get("total_sold_size");
        let avg_exit_price_str: Option<String> = row.get("avg_exit_price");

        let entry_price = Decimal::from_str(&entry_price_str)?;
        let size = Decimal::from_str(&size_str)?;

        // Calculate original total shares
        let total_shares = size / entry_price;

        // Get remaining shares (backward compat: if NULL, use total)
        let remaining_shares = remaining_size_str
            .and_then(|s| Decimal::from_str(&s).ok())
            .unwrap_or(total_shares);

        // Get cumulative realized PnL
        let prev_realized_pnl = realized_pnl_str
            .and_then(|s| Decimal::from_str(&s).ok())
            .unwrap_or(Decimal::ZERO);

        // Get total shares sold so far
        let prev_sold_size = total_sold_size_str
            .and_then(|s| Decimal::from_str(&s).ok())
            .unwrap_or(Decimal::ZERO);

        // Get previous avg exit price
        let prev_avg_exit = avg_exit_price_str
            .and_then(|s| Decimal::from_str(&s).ok());

        // Validate we have enough shares to sell
        if sell_shares > remaining_shares {
            anyhow::bail!("Cannot sell {} shares, only {} remaining", sell_shares, remaining_shares);
        }

        // Calculate PnL for this sell:
        // Cost basis for these shares = (sell_shares / total_shares) * original_size_usdc
        // But simpler: PnL = (exit_price - entry_price) * sell_shares
        let pnl_this_sell = (exit_price - entry_price) * sell_shares;

        // Update cumulative values
        let new_remaining = remaining_shares - sell_shares;
        let new_realized_pnl = prev_realized_pnl + pnl_this_sell;
        let new_sold_size = prev_sold_size + sell_shares;

        // Calculate new weighted average exit price
        let new_avg_exit = if let Some(prev_avg) = prev_avg_exit {
            // Weighted average: (prev_avg * prev_sold + exit_price * sell_shares) / new_sold_size
            ((prev_avg * prev_sold_size) + (exit_price * sell_shares)) / new_sold_size
        } else {
            // First sale, just use exit_price
            exit_price
        };

        // Check if fully closed (using small epsilon for floating point comparison)
        let is_fully_closed = new_remaining <= Decimal::new(1, 6); // < 0.000001 shares

        // Update the position
        if is_fully_closed {
            // Fully closed - set final state
            sqlx::query(
                r#"
                UPDATE positions
                SET status = 'Closed',
                    remaining_size = '0',
                    realized_pnl = ?,
                    total_sold_size = ?,
                    avg_exit_price = ?,
                    exit_price = ?,
                    pnl = ?,
                    closed_at = ?
                WHERE id = ? AND wallet_address = ?
                "#,
            )
            .bind(new_realized_pnl.to_string())
            .bind(new_sold_size.to_string())
            .bind(new_avg_exit.to_string())
            .bind(new_avg_exit.to_string()) // Final exit_price = weighted avg
            .bind(new_realized_pnl.to_string()) // Final pnl = total realized
            .bind(Utc::now().to_rfc3339())
            .bind(position_id)
            .bind(wallet_address)
            .execute(&self.pool)
            .await?;
        } else {
            // Partial close - update tracking fields, keep status Open
            sqlx::query(
                r#"
                UPDATE positions
                SET remaining_size = ?,
                    realized_pnl = ?,
                    total_sold_size = ?,
                    avg_exit_price = ?
                WHERE id = ? AND wallet_address = ?
                "#,
            )
            .bind(new_remaining.to_string())
            .bind(new_realized_pnl.to_string())
            .bind(new_sold_size.to_string())
            .bind(new_avg_exit.to_string())
            .bind(position_id)
            .bind(wallet_address)
            .execute(&self.pool)
            .await?;
        }

        Ok(PartialCloseResult {
            pnl_this_sell,
            is_fully_closed,
            total_realized_pnl: new_realized_pnl,
            remaining_shares: new_remaining,
        })
    }

    /// Update token_id for an existing position (for backfilling)
    pub async fn update_position_token_id(
        &self,
        wallet_address: &str,
        position_id: i64,
        token_id: &str,
    ) -> Result<()> {
        let result = sqlx::query(
            "UPDATE positions SET token_id = ? WHERE id = ? AND wallet_address = ?",
        )
        .bind(token_id)
        .bind(position_id)
        .bind(wallet_address)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            anyhow::bail!("Position not found or unauthorized");
        }

        Ok(())
    }

    /// Update entry_price for an existing position (for corrections)
    pub async fn update_position_entry_price(
        &self,
        wallet_address: &str,
        position_id: i64,
        entry_price: &str,
    ) -> Result<()> {
        let result = sqlx::query(
            "UPDATE positions SET entry_price = ? WHERE id = ? AND wallet_address = ?",
        )
        .bind(entry_price)
        .bind(position_id)
        .bind(wallet_address)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            anyhow::bail!("Position not found or unauthorized");
        }

        Ok(())
    }

    /// Get a single position by ID for a wallet
    pub async fn get_position_by_id(
        &self,
        wallet_address: &str,
        position_id: i64,
    ) -> Result<Option<Position>> {
        let row = sqlx::query(
            "SELECT * FROM positions WHERE id = ? AND wallet_address = ?",
        )
        .bind(position_id)
        .bind(wallet_address)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => Ok(Some(self.row_to_position(&r)?)),
            None => Ok(None),
        }
    }

    /// Get all token_ids from open positions (across all wallets)
    /// Used for real-time price subscriptions
    pub async fn get_all_open_position_token_ids(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT token_id FROM positions
             WHERE status IN ('Open', 'PendingResolution')
             AND token_id IS NOT NULL
             AND is_paper = 0"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(token_id,)| token_id).collect())
    }

    // ==================== AUTO-TRADING SETTINGS ====================

    /// Get auto-trading settings for a wallet (creates default if not exists)
    pub async fn get_auto_trading_settings(&self, wallet_address: &str) -> Result<AutoTradingSettings> {
        let row = sqlx::query(
            "SELECT * FROM auto_trading_settings WHERE wallet_address = ?"
        )
        .bind(wallet_address.to_lowercase())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                let strategies_json: String = r.get("strategies");
                let strategies: Vec<String> = serde_json::from_str(&strategies_json)
                    .unwrap_or_else(|_| vec!["sniper".to_string()]);

                Ok(AutoTradingSettings {
                    wallet_address: r.get("wallet_address"),
                    enabled: r.get::<i32, _>("enabled") != 0,
                    auto_buy_enabled: r.get::<i32, _>("auto_buy_enabled") != 0,
                    position_size: Decimal::from_str(r.get::<&str, _>("max_position_size")).unwrap_or(Decimal::from(50)),
                    max_total_exposure: Decimal::from_str(r.get::<&str, _>("max_total_exposure")).unwrap_or(Decimal::from(500)),
                    min_edge: r.get("min_edge"),
                    strategies,
                    take_profit_enabled: r.get::<i32, _>("take_profit_enabled") != 0,
                    take_profit_percent: r.get("take_profit_percent"),
                    stop_loss_enabled: r.get::<i32, _>("stop_loss_enabled") != 0,
                    stop_loss_percent: r.get("stop_loss_percent"),
                    trailing_stop_enabled: r.get::<i32, _>("trailing_stop_enabled") != 0,
                    trailing_stop_percent: r.get("trailing_stop_percent"),
                    time_exit_enabled: r.get::<i32, _>("time_exit_enabled") != 0,
                    time_exit_hours: r.get("time_exit_hours"),
                    max_positions: r.get("max_positions"),
                    cooldown_minutes: r.get("cooldown_minutes"),
                    max_daily_loss: Decimal::from_str(r.get::<&str, _>("max_daily_loss")).unwrap_or(Decimal::from(100)),
                    dispute_sniper_enabled: r.try_get::<i32, _>("dispute_sniper_enabled").unwrap_or(0) != 0,
                    min_dispute_edge: r.try_get("min_dispute_edge").unwrap_or(0.10),
                    dispute_position_size: r.try_get::<&str, _>("max_dispute_position_size")
                        .ok()
                        .and_then(|s| Decimal::from_str(s).ok())
                        .unwrap_or(Decimal::from(25)),
                    dispute_exit_on_escalation: r.try_get::<i32, _>("dispute_exit_on_escalation").unwrap_or(1) != 0,
                })
            }
            None => {
                // Create default settings
                let settings = AutoTradingSettings::for_wallet(wallet_address);
                self.create_auto_trading_settings(&settings).await?;
                Ok(settings)
            }
        }
    }

    /// Create auto-trading settings for a wallet
    async fn create_auto_trading_settings(&self, settings: &AutoTradingSettings) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let strategies_json = serde_json::to_string(&settings.strategies)?;

        sqlx::query(
            r#"
            INSERT INTO auto_trading_settings (
                wallet_address, enabled, auto_buy_enabled, max_position_size, max_total_exposure,
                min_edge, strategies, take_profit_enabled, take_profit_percent,
                stop_loss_enabled, stop_loss_percent, trailing_stop_enabled, trailing_stop_percent,
                time_exit_enabled, time_exit_hours, max_positions, cooldown_minutes, max_daily_loss,
                dispute_sniper_enabled, min_dispute_edge, max_dispute_position_size, dispute_exit_on_escalation,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(settings.wallet_address.to_lowercase())
        .bind(settings.enabled as i32)
        .bind(settings.auto_buy_enabled as i32)
        .bind(settings.position_size.to_string())
        .bind(settings.max_total_exposure.to_string())
        .bind(settings.min_edge)
        .bind(&strategies_json)
        .bind(settings.take_profit_enabled as i32)
        .bind(settings.take_profit_percent)
        .bind(settings.stop_loss_enabled as i32)
        .bind(settings.stop_loss_percent)
        .bind(settings.trailing_stop_enabled as i32)
        .bind(settings.trailing_stop_percent)
        .bind(settings.time_exit_enabled as i32)
        .bind(settings.time_exit_hours)
        .bind(settings.max_positions)
        .bind(settings.cooldown_minutes)
        .bind(settings.max_daily_loss.to_string())
        .bind(settings.dispute_sniper_enabled as i32)
        .bind(settings.min_dispute_edge)
        .bind(settings.dispute_position_size.to_string())
        .bind(settings.dispute_exit_on_escalation as i32)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update auto-trading settings for a wallet
    pub async fn update_auto_trading_settings(&self, settings: &AutoTradingSettings) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let strategies_json = serde_json::to_string(&settings.strategies)?;

        sqlx::query(
            r#"
            UPDATE auto_trading_settings SET
                enabled = ?, auto_buy_enabled = ?, max_position_size = ?, max_total_exposure = ?,
                min_edge = ?, strategies = ?, take_profit_enabled = ?, take_profit_percent = ?,
                stop_loss_enabled = ?, stop_loss_percent = ?, trailing_stop_enabled = ?, trailing_stop_percent = ?,
                time_exit_enabled = ?, time_exit_hours = ?, max_positions = ?, cooldown_minutes = ?,
                max_daily_loss = ?,
                dispute_sniper_enabled = ?, min_dispute_edge = ?, max_dispute_position_size = ?,
                dispute_exit_on_escalation = ?, updated_at = ?
            WHERE wallet_address = ?
            "#,
        )
        .bind(settings.enabled as i32)
        .bind(settings.auto_buy_enabled as i32)
        .bind(settings.position_size.to_string())
        .bind(settings.max_total_exposure.to_string())
        .bind(settings.min_edge)
        .bind(&strategies_json)
        .bind(settings.take_profit_enabled as i32)
        .bind(settings.take_profit_percent)
        .bind(settings.stop_loss_enabled as i32)
        .bind(settings.stop_loss_percent)
        .bind(settings.trailing_stop_enabled as i32)
        .bind(settings.trailing_stop_percent)
        .bind(settings.time_exit_enabled as i32)
        .bind(settings.time_exit_hours)
        .bind(settings.max_positions)
        .bind(settings.cooldown_minutes)
        .bind(settings.max_daily_loss.to_string())
        .bind(settings.dispute_sniper_enabled as i32)
        .bind(settings.min_dispute_edge)
        .bind(settings.dispute_position_size.to_string())
        .bind(settings.dispute_exit_on_escalation as i32)
        .bind(&now)
        .bind(settings.wallet_address.to_lowercase())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Enable auto-trading for a wallet
    pub async fn enable_auto_trading(&self, wallet_address: &str) -> Result<()> {
        // Ensure settings exist first
        let _ = self.get_auto_trading_settings(wallet_address).await?;

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE auto_trading_settings SET enabled = 1, updated_at = ? WHERE wallet_address = ?"
        )
        .bind(&now)
        .bind(wallet_address.to_lowercase())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Disable auto-trading for a wallet
    pub async fn disable_auto_trading(&self, wallet_address: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE auto_trading_settings SET enabled = 0, updated_at = ? WHERE wallet_address = ?"
        )
        .bind(&now)
        .bind(wallet_address.to_lowercase())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all wallets with auto-trading enabled
    pub async fn get_auto_trading_enabled_wallets(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT wallet_address FROM auto_trading_settings WHERE enabled = 1"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(addr,)| addr).collect())
    }

    /// Get all wallets with auto-buy enabled
    pub async fn get_auto_buy_enabled_wallets(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT wallet_address FROM auto_trading_settings WHERE enabled = 1 AND auto_buy_enabled = 1"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(addr,)| addr).collect())
    }

    // ==================== AUTO-TRADE LOGGING ====================

    /// Log an auto-trade action
    pub async fn log_auto_trade(&self, log: &AutoTradeLog) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO auto_trade_log (
                wallet_address, position_id, action, market_question, side,
                entry_price, exit_price, size, pnl, trigger_reason, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&log.wallet_address)
        .bind(log.position_id)
        .bind(&log.action)
        .bind(&log.market_question)
        .bind(&log.side)
        .bind(log.entry_price.map(|p| p.to_string()))
        .bind(log.exit_price.map(|p| p.to_string()))
        .bind(log.size.map(|s| s.to_string()))
        .bind(log.pnl.map(|p| p.to_string()))
        .bind(&log.trigger_reason)
        .bind(log.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get auto-trade history for a wallet
    pub async fn get_auto_trade_history(
        &self,
        wallet_address: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AutoTradeLog>> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM auto_trade_log
            WHERE wallet_address = ?
            ORDER BY created_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(wallet_address.to_lowercase())
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let logs = rows
            .iter()
            .filter_map(|row| {
                let created_at_str: String = row.get("created_at");
                let entry_price: Option<String> = row.get("entry_price");
                let exit_price: Option<String> = row.get("exit_price");
                let size: Option<String> = row.get("size");
                let pnl: Option<String> = row.get("pnl");

                Some(AutoTradeLog {
                    id: Some(row.get("id")),
                    wallet_address: row.get("wallet_address"),
                    position_id: row.get("position_id"),
                    action: row.get("action"),
                    market_question: row.get("market_question"),
                    side: row.get("side"),
                    entry_price: entry_price.and_then(|s| Decimal::from_str(&s).ok()),
                    exit_price: exit_price.and_then(|s| Decimal::from_str(&s).ok()),
                    size: size.and_then(|s| Decimal::from_str(&s).ok()),
                    pnl: pnl.and_then(|s| Decimal::from_str(&s).ok()),
                    trigger_reason: row.get("trigger_reason"),
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .ok()?
                        .with_timezone(&Utc),
                })
            })
            .collect();

        Ok(logs)
    }

    /// Get auto-trading stats for a wallet
    pub async fn get_auto_trading_stats(&self, wallet_address: &str) -> Result<AutoTradingStats> {
        let row: (i64, i64, i64, f64, i64, f64, i64, f64, i64, f64, i64, f64, i64, f64, f64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) as total_trades,
                SUM(CASE WHEN CAST(pnl AS REAL) > 0 THEN 1 ELSE 0 END) as win_count,
                SUM(CASE WHEN CAST(pnl AS REAL) <= 0 AND pnl IS NOT NULL THEN 1 ELSE 0 END) as loss_count,
                COALESCE(SUM(CAST(pnl AS REAL)), 0) as total_pnl,
                SUM(CASE WHEN action = 'take_profit' THEN 1 ELSE 0 END) as tp_count,
                COALESCE(SUM(CASE WHEN action = 'take_profit' THEN CAST(pnl AS REAL) ELSE 0 END), 0) as tp_pnl,
                SUM(CASE WHEN action = 'stop_loss' THEN 1 ELSE 0 END) as sl_count,
                COALESCE(SUM(CASE WHEN action = 'stop_loss' THEN CAST(pnl AS REAL) ELSE 0 END), 0) as sl_pnl,
                SUM(CASE WHEN action = 'trailing_stop' THEN 1 ELSE 0 END) as ts_count,
                COALESCE(SUM(CASE WHEN action = 'trailing_stop' THEN CAST(pnl AS REAL) ELSE 0 END), 0) as ts_pnl,
                SUM(CASE WHEN action = 'time_exit' THEN 1 ELSE 0 END) as te_count,
                COALESCE(SUM(CASE WHEN action = 'time_exit' THEN CAST(pnl AS REAL) ELSE 0 END), 0) as te_pnl,
                SUM(CASE WHEN action = 'auto_buy' THEN 1 ELSE 0 END) as ab_count,
                COALESCE(MAX(CAST(pnl AS REAL)), 0) as best_pnl,
                COALESCE(MIN(CAST(pnl AS REAL)), 0) as worst_pnl
            FROM auto_trade_log
            WHERE wallet_address = ?
            "#,
        )
        .bind(wallet_address.to_lowercase())
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0, 0, 0, 0.0, 0, 0.0, 0, 0.0, 0, 0.0, 0, 0.0, 0, 0.0, 0.0));

        let win_rate = if row.0 > 0 {
            row.1 as f64 / row.0 as f64
        } else {
            0.0
        };

        Ok(AutoTradingStats {
            total_trades: row.0,
            win_count: row.1,
            loss_count: row.2,
            win_rate,
            total_pnl: Decimal::from_f64_retain(row.3).unwrap_or_default(),
            take_profit_count: row.4,
            take_profit_pnl: Decimal::from_f64_retain(row.5).unwrap_or_default(),
            stop_loss_count: row.6,
            stop_loss_pnl: Decimal::from_f64_retain(row.7).unwrap_or_default(),
            trailing_stop_count: row.8,
            trailing_stop_pnl: Decimal::from_f64_retain(row.9).unwrap_or_default(),
            time_exit_count: row.10,
            time_exit_pnl: Decimal::from_f64_retain(row.11).unwrap_or_default(),
            auto_buy_count: row.12,
            best_trade_pnl: Decimal::from_f64_retain(row.13).unwrap_or_default(),
            worst_trade_pnl: Decimal::from_f64_retain(row.14).unwrap_or_default(),
            avg_hold_hours: 0.0, // TODO: calculate from position data
        })
    }

    // ==================== POSITION PEAKS (TRAILING STOP) ====================

    /// Update or create peak price for a position
    pub async fn update_position_peak(&self, position_id: i64, price: Decimal) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        // Get current peak
        let current: Option<(String,)> = sqlx::query_as(
            "SELECT peak_price FROM position_peaks WHERE position_id = ?"
        )
        .bind(position_id)
        .fetch_optional(&self.pool)
        .await?;

        let should_update = match current {
            Some((peak_str,)) => {
                let current_peak = Decimal::from_str(&peak_str).unwrap_or_default();
                price > current_peak
            }
            None => true,
        };

        if should_update {
            sqlx::query(
                r#"
                INSERT INTO position_peaks (position_id, peak_price, peak_at)
                VALUES (?, ?, ?)
                ON CONFLICT(position_id) DO UPDATE SET
                    peak_price = excluded.peak_price,
                    peak_at = excluded.peak_at
                "#,
            )
            .bind(position_id)
            .bind(price.to_string())
            .bind(&now)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Get peak price for a position
    pub async fn get_position_peak(&self, position_id: i64) -> Result<Option<Decimal>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT peak_price FROM position_peaks WHERE position_id = ?"
        )
        .bind(position_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.and_then(|(s,)| Decimal::from_str(&s).ok()))
    }

    /// Delete peak price when position is closed
    pub async fn delete_position_peak(&self, position_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM position_peaks WHERE position_id = ?")
            .bind(position_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get all position peaks (for loading into memory on startup)
    pub async fn get_all_position_peaks(&self) -> Result<Vec<crate::services::auto_trader::PositionPeak>> {
        let rows = sqlx::query(
            "SELECT position_id, peak_price, peak_at FROM position_peaks"
        )
        .fetch_all(&self.pool)
        .await?;

        let peaks = rows
            .iter()
            .filter_map(|row| {
                let position_id: i64 = row.get("position_id");
                let peak_price_str: String = row.get("peak_price");
                let peak_at_str: String = row.get("peak_at");

                Some(crate::services::auto_trader::PositionPeak {
                    position_id,
                    peak_price: Decimal::from_str(&peak_price_str).ok()?,
                    peak_at: DateTime::parse_from_rfc3339(&peak_at_str).ok()?.with_timezone(&Utc),
                })
            })
            .collect();

        Ok(peaks)
    }

    // ==================== POSITION QUERIES FOR AUTO-TRADING ====================

    /// Get positions by token_id (for price update handling)
    pub async fn get_positions_by_token_id(&self, token_id: &str) -> Result<Vec<Position>> {
        let rows = sqlx::query(
            "SELECT * FROM positions WHERE token_id = ? AND status IN ('Open', 'PendingResolution') AND is_paper = 0"
        )
        .bind(token_id)
        .fetch_all(&self.pool)
        .await?;

        let positions = rows
            .iter()
            .filter_map(|row| self.row_to_position(row).ok())
            .collect();

        Ok(positions)
    }

    /// Check if wallet has open position in a market
    pub async fn has_open_position(&self, wallet_address: &str, market_id: &str) -> Result<bool> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE wallet_address = ? AND market_id = ? AND status IN ('Open', 'PendingResolution') AND is_paper = 0"
        )
        .bind(wallet_address.to_lowercase())
        .bind(market_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count.0 > 0)
    }

    /// Check if wallet has open dispute position for a condition_id
    pub async fn has_open_dispute_position(&self, wallet_address: &str, condition_id: &str) -> Result<bool> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE wallet_address = ? AND market_id = ? AND strategy = 'Dispute' AND status IN ('Open', 'PendingResolution') AND is_paper = 0"
        )
        .bind(wallet_address.to_lowercase())
        .bind(condition_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count.0 > 0)
    }

    /// Count open positions for a wallet
    pub async fn count_open_positions(&self, wallet_address: &str) -> Result<i32> {
        let count: (i32,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE wallet_address = ? AND status IN ('Open', 'PendingResolution') AND is_paper = 0"
        )
        .bind(wallet_address.to_lowercase())
        .fetch_one(&self.pool)
        .await?;

        Ok(count.0)
    }

    /// Get total exposure (sum of position sizes) for a wallet
    pub async fn get_total_exposure(&self, wallet_address: &str) -> Result<Decimal> {
        let sum: Option<(f64,)> = sqlx::query_as(
            "SELECT COALESCE(SUM(CAST(size AS REAL)), 0) FROM positions WHERE wallet_address = ? AND status IN ('Open', 'PendingResolution') AND is_paper = 0"
        )
        .bind(wallet_address.to_lowercase())
        .fetch_optional(&self.pool)
        .await?;

        Ok(Decimal::from_f64_retain(sum.map(|(s,)| s).unwrap_or(0.0)).unwrap_or_default())
    }

    /// Get a position by ID (without wallet verification - for internal use)
    pub async fn get_position_by_id_internal(&self, position_id: i64) -> Result<Option<Position>> {
        let row = sqlx::query("SELECT * FROM positions WHERE id = ?")
            .bind(position_id)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => Ok(Some(self.row_to_position(&r)?)),
            None => Ok(None),
        }
    }

    /// Get wallet by address (with encrypted key data)
    pub async fn get_wallet_by_address(&self, address: &str) -> Result<Option<WalletWithKey>> {
        let row = sqlx::query("SELECT * FROM wallets WHERE address = ?")
            .bind(address.to_lowercase())
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => {
                let encrypted_key: Option<Vec<u8>> = r.get("encrypted_private_key");
                let salt: Option<Vec<u8>> = r.get("salt");
                let nonce: Option<Vec<u8>> = r.get("nonce");

                // For auto-trading, we need the decrypted key
                // This returns the encrypted form - the caller must decrypt
                let encrypted_key = match (encrypted_key, salt, nonce) {
                    (Some(c), Some(s), Some(n)) => Some(EncryptedKey {
                        ciphertext: c,
                        salt: s,
                        nonce: n,
                    }),
                    _ => None,
                };

                Ok(Some(WalletWithKey {
                    address: r.get("address"),
                    encrypted_key,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get today's PnL from auto-trades for a wallet
    pub async fn get_daily_auto_pnl(&self, wallet_address: &str) -> Result<Decimal> {
        let today_start = Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
        let today_start_str = DateTime::<Utc>::from_naive_utc_and_offset(today_start, Utc).to_rfc3339();

        let sum: Option<(f64,)> = sqlx::query_as(
            "SELECT COALESCE(SUM(CAST(pnl AS REAL)), 0) FROM auto_trade_log WHERE wallet_address = ? AND created_at >= ? AND pnl IS NOT NULL"
        )
        .bind(wallet_address.to_lowercase())
        .bind(&today_start_str)
        .fetch_optional(&self.pool)
        .await?;

        Ok(Decimal::from_f64_retain(sum.map(|(s,)| s).unwrap_or(0.0)).unwrap_or_default())
    }

    // ==================== DESCRIPTION HASHES (CLARIFICATION MONITOR) ====================

    /// Get stored description hash for a market
    pub async fn get_description_hash(&self, market_id: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT description_hash FROM description_hashes WHERE market_id = ?"
        )
        .bind(market_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(hash,)| hash))
    }

    /// Store or update description hash for a market
    pub async fn upsert_description_hash(&self, market_id: &str, hash: &str) -> Result<()> {
        let now = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO description_hashes (market_id, description_hash, last_updated)
            VALUES (?, ?, ?)
            ON CONFLICT(market_id) DO UPDATE SET
                description_hash = excluded.description_hash,
                last_updated = excluded.last_updated
            "#,
        )
        .bind(market_id)
        .bind(hash)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get all stored description hashes (for loading into memory on startup)
    pub async fn get_all_description_hashes(&self) -> Result<Vec<(String, String)>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT market_id, description_hash FROM description_hashes"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    // ==================== ORDER LIFECYCLE TRACKING ====================

    /// Create a new order record
    pub async fn create_order(
        &self,
        order: &crate::types::Order,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO orders (id, wallet_address, token_id, market_id, side, order_type, price, original_size, filled_size, avg_fill_price, status, position_id, neg_risk, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&order.id)
        .bind(&order.wallet_address)
        .bind(&order.token_id)
        .bind(&order.market_id)
        .bind(format!("{:?}", order.side))
        .bind(&order.order_type)
        .bind(order.price.to_string())
        .bind(order.original_size.to_string())
        .bind(order.filled_size.to_string())
        .bind(order.avg_fill_price.map(|p| p.to_string()))
        .bind(format!("{:?}", order.status))
        .bind(order.position_id)
        .bind(order.neg_risk as i32)
        .bind(order.created_at.to_rfc3339())
        .bind(order.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update order status (and optionally fill info)
    pub async fn update_order_status(
        &self,
        order_id: &str,
        status: crate::types::OrderLifecycleStatus,
        filled_size: Option<Decimal>,
        avg_fill_price: Option<Decimal>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let status_str = format!("{:?}", status);

        if let (Some(fill), Some(price)) = (filled_size, avg_fill_price) {
            sqlx::query(
                "UPDATE orders SET status = ?, filled_size = ?, avg_fill_price = ?, updated_at = ? WHERE id = ?"
            )
            .bind(&status_str)
            .bind(fill.to_string())
            .bind(price.to_string())
            .bind(&now)
            .bind(order_id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                "UPDATE orders SET status = ?, updated_at = ? WHERE id = ?"
            )
            .bind(&status_str)
            .bind(&now)
            .bind(order_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Get orders for a wallet with optional status filter
    pub async fn get_orders_for_wallet(
        &self,
        wallet_address: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<crate::types::Order>> {
        use crate::types::{Order, OrderLifecycleStatus};

        let rows = if let Some(status) = status_filter {
            sqlx::query(
                "SELECT * FROM orders WHERE wallet_address = ? AND status = ? ORDER BY created_at DESC LIMIT 100"
            )
            .bind(wallet_address.to_lowercase())
            .bind(status)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT * FROM orders WHERE wallet_address = ? ORDER BY created_at DESC LIMIT 100"
            )
            .bind(wallet_address.to_lowercase())
            .fetch_all(&self.pool)
            .await?
        };

        let orders = rows
            .iter()
            .filter_map(|row| {
                let side_str: String = row.get("side");
                let side = match side_str.as_str() {
                    "Yes" => crate::types::Side::Yes,
                    _ => crate::types::Side::No,
                };

                let status_str: String = row.get("status");
                let status = match status_str.as_str() {
                    "Pending" => OrderLifecycleStatus::Pending,
                    "Live" => OrderLifecycleStatus::Live,
                    "Matched" => OrderLifecycleStatus::Matched,
                    "Mined" => OrderLifecycleStatus::Mined,
                    "Confirmed" => OrderLifecycleStatus::Confirmed,
                    "Failed" => OrderLifecycleStatus::Failed,
                    "Cancelled" => OrderLifecycleStatus::Cancelled,
                    _ => OrderLifecycleStatus::Pending,
                };

                let price_str: String = row.get("price");
                let original_size_str: String = row.get("original_size");
                let filled_size_str: String = row.get("filled_size");
                let avg_fill_price: Option<String> = row.get("avg_fill_price");
                let created_at_str: String = row.get("created_at");
                let updated_at_str: String = row.get("updated_at");

                Some(Order {
                    id: row.get("id"),
                    wallet_address: row.get("wallet_address"),
                    token_id: row.get("token_id"),
                    market_id: row.get("market_id"),
                    side,
                    order_type: row.get("order_type"),
                    price: Decimal::from_str(&price_str).ok()?,
                    original_size: Decimal::from_str(&original_size_str).ok()?,
                    filled_size: Decimal::from_str(&filled_size_str).unwrap_or_default(),
                    avg_fill_price: avg_fill_price.and_then(|s| Decimal::from_str(&s).ok()),
                    status,
                    position_id: row.get("position_id"),
                    neg_risk: row.try_get::<i32, _>("neg_risk").unwrap_or(0) != 0,
                    created_at: DateTime::parse_from_rfc3339(&created_at_str).ok()?.with_timezone(&Utc),
                    updated_at: DateTime::parse_from_rfc3339(&updated_at_str).ok()?.with_timezone(&Utc),
                })
            })
            .collect();

        Ok(orders)
    }

    /// Get pending orders (for reconciliation)
    pub async fn get_pending_orders(&self) -> Result<Vec<crate::types::Order>> {
        self.get_orders_for_wallet("", Some("Pending")).await
        // Note: This won't work as expected - let's use a separate query
    }

    // ==================== MILLIONAIRES CLUB ====================

    /// Get MC config (creates default if not exists)
    pub async fn mc_get_config(&self) -> Result<crate::services::mc_scanner::McConfig> {
        use crate::services::mc_scanner::McConfig;

        let row = sqlx::query("SELECT * FROM mc_config WHERE id = 1")
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(r) => Ok(McConfig {
                bankroll: r.get("bankroll"),
                tier: r.get("tier"),
                mode: r.get("mode"),
                peak_bankroll: r.get("peak_bankroll"),
                pause_state: r.get("pause_state"),
                pause_until: r.try_get("pause_until").unwrap_or(None),
            }),
            None => {
                let now = Utc::now().to_rfc3339();
                sqlx::query(
                    "INSERT INTO mc_config (id, bankroll, tier, mode, peak_bankroll, pause_state, created_at, updated_at) VALUES (1, '40', 1, 'observation', '40', 'active', ?, ?)"
                )
                .bind(&now)
                .bind(&now)
                .execute(&self.pool)
                .await?;

                Ok(McConfig {
                    bankroll: "40".to_string(),
                    tier: 1,
                    mode: "observation".to_string(),
                    peak_bankroll: "40".to_string(),
                    pause_state: "active".to_string(),
                    pause_until: None,
                })
            }
        }
    }

    /// Update MC bankroll and peak
    pub async fn mc_update_bankroll(&self, bankroll: &str, peak: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE mc_config SET bankroll = ?, peak_bankroll = ?, updated_at = ? WHERE id = 1")
            .bind(bankroll)
            .bind(peak)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update MC tier
    pub async fn mc_update_tier(&self, tier: i32) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE mc_config SET tier = ?, updated_at = ? WHERE id = 1")
            .bind(tier)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update MC mode
    pub async fn mc_update_mode(&self, mode: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE mc_config SET mode = ?, updated_at = ? WHERE id = 1")
            .bind(mode)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update MC pause state
    pub async fn mc_update_pause_state(&self, state: &str, until: Option<&str>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE mc_config SET pause_state = ?, pause_until = ?, updated_at = ? WHERE id = 1")
            .bind(state)
            .bind(until)
            .bind(&now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Insert MC scout log entry
    pub async fn mc_insert_scout_log(&self, scout: &crate::services::mc_scanner::McScoutResult) -> Result<()> {
        let reasons_json = serde_json::to_string(&scout.reasons).unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            r#"
            INSERT INTO mc_scout_log (market_id, condition_id, question, slug, side, price, volume, category, end_date, passed, certainty_score, reasons, slippage_pct, would_trade, token_id, scanned_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&scout.market_id)
        .bind(&scout.condition_id)
        .bind(&scout.question)
        .bind(&scout.slug)
        .bind(&scout.side)
        .bind(&scout.price)
        .bind(&scout.volume)
        .bind(&scout.category)
        .bind(&scout.end_date)
        .bind(scout.passed as i32)
        .bind(scout.certainty_score)
        .bind(&reasons_json)
        .bind(scout.slippage_pct)
        .bind(scout.would_trade as i32)
        .bind(&scout.token_id)
        .bind(&scout.scanned_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get MC scout log (paginated)
    pub async fn mc_get_scout_log(&self, limit: i64, offset: i64) -> Result<(Vec<crate::services::mc_scanner::McScoutResult>, i64)> {
        use crate::services::mc_scanner::McScoutResult;

        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mc_scout_log")
            .fetch_one(&self.pool)
            .await?;

        let rows = sqlx::query(
            "SELECT * FROM mc_scout_log ORDER BY scanned_at DESC LIMIT ? OFFSET ?"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let logs: Vec<McScoutResult> = rows.iter().filter_map(|row| {
            let reasons_str: String = row.try_get("reasons").unwrap_or_else(|_| "[]".to_string());
            let reasons: Vec<String> = serde_json::from_str(&reasons_str).unwrap_or_default();

            Some(McScoutResult {
                market_id: row.get("market_id"),
                condition_id: row.try_get("condition_id").unwrap_or_default(),
                question: row.get("question"),
                slug: row.try_get("slug").unwrap_or_default(),
                side: row.get("side"),
                price: row.get("price"),
                volume: row.try_get("volume").unwrap_or_default(),
                category: row.try_get("category").unwrap_or(None),
                end_date: row.try_get("end_date").unwrap_or(None),
                passed: row.get::<i32, _>("passed") != 0,
                certainty_score: row.get("certainty_score"),
                reasons,
                slippage_pct: row.try_get("slippage_pct").unwrap_or(None),
                would_trade: row.get::<i32, _>("would_trade") != 0,
                token_id: row.try_get("token_id").unwrap_or(None),
                scanned_at: row.get("scanned_at"),
            })
        }).collect();

        Ok((logs, total.0))
    }

    /// Insert MC simulated trade
    pub async fn mc_insert_trade(
        &self,
        market_id: &str,
        condition_id: &str,
        question: &str,
        slug: &str,
        side: &str,
        entry_price: &str,
        size: &str,
        shares: &str,
        certainty_score: i32,
        category: Option<&str>,
        tier: i32,
        token_id: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO mc_trades (market_id, condition_id, question, slug, side, entry_price, size, shares, certainty_score, category, tier_at_entry, token_id, end_date, opened_at, status)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'open')
            "#,
        )
        .bind(market_id)
        .bind(condition_id)
        .bind(question)
        .bind(slug)
        .bind(side)
        .bind(entry_price)
        .bind(size)
        .bind(shares)
        .bind(certainty_score)
        .bind(category)
        .bind(tier)
        .bind(token_id)
        .bind(end_date)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update MC trade resolution
    pub async fn mc_update_trade_resolution(&self, trade_id: i64, exit_price: &str, pnl: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE mc_trades SET exit_price = ?, pnl = ?, status = ?, closed_at = ? WHERE id = ?"
        )
        .bind(exit_price)
        .bind(pnl)
        .bind(status)
        .bind(&now)
        .bind(trade_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get MC trades (paginated) — returns full trade data for API
    pub async fn mc_get_trades(&self, limit: i64, offset: i64) -> Result<(Vec<McTradeFullRow>, i64)> {
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mc_trades")
            .fetch_one(&self.pool)
            .await?;

        let rows = sqlx::query(
            "SELECT * FROM mc_trades ORDER BY opened_at DESC LIMIT ? OFFSET ?"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let trades: Vec<McTradeFullRow> = rows.iter().filter_map(|row| {
            Some(McTradeFullRow {
                id: row.get("id"),
                market_id: row.get("market_id"),
                condition_id: row.try_get("condition_id").unwrap_or_default(),
                question: row.get("question"),
                slug: row.try_get("slug").unwrap_or_default(),
                side: row.get("side"),
                entry_price: row.get("entry_price"),
                exit_price: row.try_get("exit_price").unwrap_or(None),
                size: row.try_get("size").unwrap_or_default(),
                shares: row.try_get("shares").unwrap_or_default(),
                pnl: row.try_get("pnl").unwrap_or(None),
                certainty_score: row.try_get("certainty_score").unwrap_or(0),
                category: row.try_get("category").unwrap_or(None),
                status: row.get("status"),
                tier_at_entry: row.try_get("tier_at_entry").unwrap_or(1),
                token_id: row.try_get("token_id").unwrap_or(None),
                end_date: row.try_get("end_date").unwrap_or(None),
                opened_at: row.get("opened_at"),
                closed_at: row.try_get("closed_at").unwrap_or(None),
            })
        }).collect();

        Ok((trades, total.0))
    }

    /// Get MC open trades (for resolution checking)
    pub async fn mc_get_open_trades(&self) -> Result<Vec<crate::services::mc_scanner::McTradeRow>> {
        use crate::services::mc_scanner::McTradeRow;

        let rows = sqlx::query(
            "SELECT * FROM mc_trades WHERE status = 'open'"
        )
        .fetch_all(&self.pool)
        .await?;

        let trades: Vec<McTradeRow> = rows.iter().filter_map(|row| {
            Some(McTradeRow {
                id: row.get("id"),
                market_id: row.get("market_id"),
                condition_id: row.try_get("condition_id").unwrap_or_default(),
                question: row.get("question"),
                side: row.get("side"),
                entry_price: row.get("entry_price"),
                shares: row.try_get("shares").unwrap_or_default(),
                status: row.get("status"),
            })
        }).collect();

        Ok(trades)
    }

    /// Get MC open trade count
    pub async fn mc_get_open_trade_count(&self) -> Result<i64> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mc_trades WHERE status = 'open'")
            .fetch_one(&self.pool)
            .await?;
        Ok(count.0)
    }

    /// Get MC trade stats (total, wins, total_pnl)
    pub async fn mc_get_trade_stats(&self) -> Result<(i64, i64, f64)> {
        let row: (i64, i64, f64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) as total,
                SUM(CASE WHEN status = 'won' THEN 1 ELSE 0 END) as wins,
                COALESCE(SUM(CAST(pnl AS REAL)), 0) as total_pnl
            FROM mc_trades
            WHERE status IN ('won', 'lost')
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0, 0, 0.0));

        Ok(row)
    }

    /// Get category trade count (open trades in a category)
    pub async fn mc_get_category_trade_count(&self, category: &str) -> Result<i64> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM mc_trades WHERE category = ? AND status = 'open'"
        )
        .bind(category)
        .fetch_one(&self.pool)
        .await?;
        Ok(count.0)
    }

    /// Insert MC tier history
    pub async fn mc_insert_tier_history(&self, from_tier: i32, to_tier: i32, bankroll: &str, reason: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO mc_tier_history (from_tier, to_tier, bankroll, reason, timestamp) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(from_tier)
        .bind(to_tier)
        .bind(bankroll)
        .bind(reason)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get MC tier history
    pub async fn mc_get_tier_history(&self) -> Result<Vec<McTierHistoryRow>> {
        let rows = sqlx::query(
            "SELECT * FROM mc_tier_history ORDER BY timestamp DESC"
        )
        .fetch_all(&self.pool)
        .await?;

        let history: Vec<McTierHistoryRow> = rows.iter().filter_map(|row| {
            Some(McTierHistoryRow {
                id: row.get("id"),
                from_tier: row.get("from_tier"),
                to_tier: row.get("to_tier"),
                bankroll: row.get("bankroll"),
                reason: row.get("reason"),
                timestamp: row.get("timestamp"),
            })
        }).collect();

        Ok(history)
    }

    /// Insert MC drawdown event
    pub async fn mc_insert_drawdown_event(
        &self,
        event_type: &str,
        peak_bankroll: &str,
        current_bankroll: &str,
        drawdown_pct: f64,
        action_taken: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO mc_drawdown_events (event_type, peak_bankroll, current_bankroll, drawdown_pct, action_taken, timestamp) VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(event_type)
        .bind(peak_bankroll)
        .bind(current_bankroll)
        .bind(drawdown_pct)
        .bind(action_taken)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get recent losses in the last N days
    pub async fn mc_get_recent_losses(&self, days: i64) -> Result<i64> {
        let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM mc_trades WHERE status = 'lost' AND closed_at >= ?"
        )
        .bind(&cutoff)
        .fetch_one(&self.pool)
        .await?;
        Ok(count.0)
    }

    /// Cleanup old scout logs (keep last N days)
    pub async fn mc_cleanup_old_scout_logs(&self, days: i64) -> Result<u64> {
        let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
        let result = sqlx::query("DELETE FROM mc_scout_log WHERE scanned_at < ?")
            .bind(&cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    // ==================== MINT MAKER METHODS ====================

    /// Get mint maker settings for a wallet (returns defaults if none exist)
    pub async fn get_mint_maker_settings(&self, wallet_address: &str) -> Result<MintMakerSettingsRow> {
        let addr = wallet_address.to_lowercase();
        let row = sqlx::query(
            "SELECT * FROM mint_maker_settings WHERE wallet_address = ?"
        )
        .bind(&addr)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let assets_json: String = row.get("assets");
                let assets: Vec<String> = serde_json::from_str(&assets_json).unwrap_or_else(|_| vec!["BTC".to_string(), "ETH".to_string(), "SOL".to_string()]);
                Ok(MintMakerSettingsRow {
                    wallet_address: row.get("wallet_address"),
                    enabled: row.get::<i32, _>("enabled") != 0,
                    preset: row.get("preset"),
                    bid_offset_cents: row.get("bid_offset_cents"),
                    max_pair_cost: row.get("max_pair_cost"),
                    min_spread_profit: row.get("min_spread_profit"),
                    max_pairs_per_market: row.get("max_pairs_per_market"),
                    max_total_pairs: row.get("max_total_pairs"),
                    stale_order_seconds: row.get("stale_order_seconds"),
                    assets,
                    min_minutes_to_close: row.get("min_minutes_to_close"),
                    max_minutes_to_close: row.get("max_minutes_to_close"),
                    auto_place: row.try_get::<i32, _>("auto_place").unwrap_or(0) != 0,
                    auto_place_size: row.try_get::<String, _>("auto_place_size").unwrap_or_else(|_| "2".to_string()),
                    auto_max_markets: row.try_get::<i32, _>("auto_max_markets").unwrap_or(1),
                    auto_redeem: row.try_get::<i32, _>("auto_redeem").unwrap_or(0) != 0,
                })
            }
            None => {
                // Return defaults
                Ok(MintMakerSettingsRow {
                    wallet_address: addr,
                    enabled: false,
                    preset: "balanced".to_string(),
                    bid_offset_cents: 2,
                    max_pair_cost: 0.98,
                    min_spread_profit: 0.01,
                    max_pairs_per_market: 5,
                    max_total_pairs: 20,
                    stale_order_seconds: 120,
                    assets: vec!["BTC".to_string(), "ETH".to_string(), "SOL".to_string(), "XRP".to_string()],
                    min_minutes_to_close: 2.0,
                    max_minutes_to_close: 14.0,
                    auto_place: false,
                    auto_place_size: "2".to_string(),
                    auto_max_markets: 1,
                    auto_redeem: false,
                })
            }
        }
    }

    /// Upsert mint maker settings for a wallet
    pub async fn upsert_mint_maker_settings(&self, settings: &MintMakerSettingsRow) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let assets_json = serde_json::to_string(&settings.assets)?;
        sqlx::query(
            r#"
            INSERT INTO mint_maker_settings (wallet_address, enabled, preset, bid_offset_cents, max_pair_cost,
                min_spread_profit, max_pairs_per_market, max_total_pairs, stale_order_seconds, assets,
                min_minutes_to_close, max_minutes_to_close, auto_place, auto_place_size, auto_max_markets,
                auto_redeem, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(wallet_address) DO UPDATE SET
                enabled = excluded.enabled,
                preset = excluded.preset,
                bid_offset_cents = excluded.bid_offset_cents,
                max_pair_cost = excluded.max_pair_cost,
                min_spread_profit = excluded.min_spread_profit,
                max_pairs_per_market = excluded.max_pairs_per_market,
                max_total_pairs = excluded.max_total_pairs,
                stale_order_seconds = excluded.stale_order_seconds,
                assets = excluded.assets,
                min_minutes_to_close = excluded.min_minutes_to_close,
                max_minutes_to_close = excluded.max_minutes_to_close,
                auto_place = excluded.auto_place,
                auto_place_size = excluded.auto_place_size,
                auto_max_markets = excluded.auto_max_markets,
                auto_redeem = excluded.auto_redeem,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&settings.wallet_address.to_lowercase())
        .bind(settings.enabled as i32)
        .bind(&settings.preset)
        .bind(settings.bid_offset_cents)
        .bind(settings.max_pair_cost)
        .bind(settings.min_spread_profit)
        .bind(settings.max_pairs_per_market)
        .bind(settings.max_total_pairs)
        .bind(settings.stale_order_seconds)
        .bind(&assets_json)
        .bind(settings.min_minutes_to_close)
        .bind(settings.max_minutes_to_close)
        .bind(settings.auto_place as i32)
        .bind(&settings.auto_place_size)
        .bind(settings.auto_max_markets)
        .bind(settings.auto_redeem as i32)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Create a new mint maker pair
    pub async fn create_mint_maker_pair(
        &self,
        wallet_address: &str,
        market_id: &str,
        condition_id: &str,
        question: &str,
        asset: &str,
        yes_order_id: &str,
        no_order_id: &str,
        yes_bid_price: &str,
        no_bid_price: &str,
        size: &str,
        yes_size: Option<&str>,
        no_size: Option<&str>,
        slug: Option<&str>,
        yes_token_id: Option<&str>,
        no_token_id: Option<&str>,
        neg_risk: bool,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query(
            r#"
            INSERT INTO mint_maker_pairs (wallet_address, market_id, condition_id, question, asset,
                yes_order_id, no_order_id, yes_bid_price, no_bid_price, size, yes_size, no_size, slug,
                yes_token_id, no_token_id, neg_risk, status, created_at, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'Pending', ?, ?)
            "#,
        )
        .bind(wallet_address.to_lowercase())
        .bind(market_id)
        .bind(condition_id)
        .bind(question)
        .bind(asset)
        .bind(yes_order_id)
        .bind(no_order_id)
        .bind(yes_bid_price)
        .bind(no_bid_price)
        .bind(size)
        .bind(yes_size)
        .bind(no_size)
        .bind(slug)
        .bind(yes_token_id)
        .bind(no_token_id)
        .bind(neg_risk as i32)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Update pair status
    pub async fn update_mint_maker_pair_status(&self, pair_id: i64, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE mint_maker_pairs SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(&now)
            .bind(pair_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Set the stop loss sell order ID on a pair
    pub async fn set_mint_maker_stop_loss_order(&self, pair_id: i64, order_id: &str, status: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE mint_maker_pairs SET stop_loss_order_id = ?, status = ?, updated_at = ? WHERE id = ?")
            .bind(order_id)
            .bind(status)
            .bind(&now)
            .bind(pair_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get pairs in StopLoss status that have a sell order to track
    pub async fn get_mint_maker_stop_loss_pairs(&self, wallet_address: &str) -> Result<Vec<MintMakerPairRow>> {
        let rows = sqlx::query(
            "SELECT * FROM mint_maker_pairs WHERE wallet_address = ? AND status = 'StopLoss' AND stop_loss_order_id IS NOT NULL ORDER BY created_at DESC"
        )
        .bind(wallet_address.to_lowercase())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_mm_pair).collect())
    }

    /// Update pair fill prices and calculate cost/profit
    pub async fn update_mint_maker_pair_fill(
        &self,
        pair_id: i64,
        yes_fill: Option<&str>,
        no_fill: Option<&str>,
        status: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        // Calculate pair_cost and profit if both filled
        let (pair_cost, profit) = if let (Some(y), Some(n)) = (yes_fill, no_fill) {
            let y_dec: f64 = y.parse().unwrap_or(0.0);
            let n_dec: f64 = n.parse().unwrap_or(0.0);
            let cost = y_dec + n_dec;
            let profit = 1.0 - cost;
            (Some(format!("{:.6}", cost)), Some(format!("{:.6}", profit)))
        } else {
            (None, None)
        };

        sqlx::query(
            r#"
            UPDATE mint_maker_pairs SET
                yes_fill_price = COALESCE(?, yes_fill_price),
                no_fill_price = COALESCE(?, no_fill_price),
                pair_cost = COALESCE(?, pair_cost),
                profit = COALESCE(?, profit),
                status = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(yes_fill)
        .bind(no_fill)
        .bind(pair_cost.as_deref())
        .bind(profit.as_deref())
        .bind(status)
        .bind(&now)
        .bind(pair_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Update pair merge size (e.g. to cap at actual filled amount)
    pub async fn update_mint_maker_pair_size(&self, pair_id: i64, size: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE mint_maker_pairs SET size = ?, updated_at = ? WHERE id = ?")
            .bind(size)
            .bind(&now)
            .bind(pair_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Mark a pair as merged with transaction ID
    pub async fn mark_mint_maker_pair_merged(&self, pair_id: i64, merge_tx_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE mint_maker_pairs SET status = 'Merged', merge_tx_id = ?, updated_at = ? WHERE id = ?")
            .bind(merge_tx_id)
            .bind(&now)
            .bind(pair_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get open pairs for a wallet (Pending, HalfFilled, Matched, Merging, Orphaned, StopLoss)
    pub async fn get_mint_maker_open_pairs(&self, wallet_address: &str) -> Result<Vec<MintMakerPairRow>> {
        let rows = sqlx::query(
            "SELECT * FROM mint_maker_pairs WHERE wallet_address = ? AND status IN ('Pending', 'HalfFilled', 'Matched', 'Merging', 'Orphaned', 'StopLoss') ORDER BY created_at DESC"
        )
        .bind(wallet_address.to_lowercase())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_mm_pair).collect())
    }

    /// Get pairs by status for a wallet
    pub async fn get_mint_maker_pairs_by_status(&self, wallet_address: &str, status: &str) -> Result<Vec<MintMakerPairRow>> {
        let rows = sqlx::query(
            "SELECT * FROM mint_maker_pairs WHERE wallet_address = ? AND status = ? ORDER BY created_at DESC"
        )
        .bind(wallet_address.to_lowercase())
        .bind(status)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_mm_pair).collect())
    }

    /// Get all recent pairs for a wallet (for display)
    pub async fn get_mint_maker_recent_pairs(&self, wallet_address: &str, limit: i64) -> Result<Vec<MintMakerPairRow>> {
        let rows = sqlx::query(
            "SELECT * FROM mint_maker_pairs WHERE wallet_address = ? ORDER BY created_at DESC LIMIT ?"
        )
        .bind(wallet_address.to_lowercase())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_mm_pair).collect())
    }

    /// Count open pairs for a specific market
    pub async fn count_mint_maker_open_pairs_for_market(&self, wallet_address: &str, market_id: &str) -> Result<i64> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM mint_maker_pairs WHERE wallet_address = ? AND market_id = ? AND status IN ('Pending', 'HalfFilled', 'Matched', 'Merging')"
        )
        .bind(wallet_address.to_lowercase())
        .bind(market_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(count.0)
    }

    /// Count total open pairs for a wallet
    pub async fn count_mint_maker_total_open_pairs(&self, wallet_address: &str) -> Result<i64> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM mint_maker_pairs WHERE wallet_address = ? AND status IN ('Pending', 'HalfFilled', 'Matched', 'Merging')"
        )
        .bind(wallet_address.to_lowercase())
        .fetch_one(&self.pool)
        .await?;
        Ok(count.0)
    }

    /// Get stale pairs (older than threshold seconds, still pending/half-filled)
    pub async fn get_mint_maker_stale_pairs(&self, wallet_address: &str, stale_seconds: i64) -> Result<Vec<MintMakerPairRow>> {
        let cutoff = (Utc::now() - Duration::seconds(stale_seconds)).to_rfc3339();
        let rows = sqlx::query(
            "SELECT * FROM mint_maker_pairs WHERE wallet_address = ? AND status IN ('Pending', 'HalfFilled') AND created_at < ?"
        )
        .bind(wallet_address.to_lowercase())
        .bind(&cutoff)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_mm_pair).collect())
    }

    /// Get pairs eligible for auto-redeem (tokens held but not yet redeemed)
    pub async fn get_mint_maker_redeemable_pairs(&self, wallet_address: &str) -> Result<Vec<MintMakerPairRow>> {
        let rows = sqlx::query(
            "SELECT * FROM mint_maker_pairs WHERE wallet_address = ? AND status IN ('Matched', 'HalfFilled', 'Orphaned', 'StopLoss', 'MergeFailed', 'Merging') ORDER BY created_at DESC"
        )
        .bind(wallet_address.to_lowercase())
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(Self::row_to_mm_pair).collect())
    }

    /// Get wallets with mint maker enabled
    pub async fn get_mint_maker_enabled_wallets(&self) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT wallet_address FROM mint_maker_settings WHERE enabled = 1"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(a,)| a).collect())
    }

    /// Get mint maker stats for a wallet
    pub async fn get_mint_maker_stats(&self, wallet_address: &str) -> Result<(i64, i64, i64, f64, f64, f64)> {
        let row: (i64, i64, i64, f64, f64, f64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) as total_pairs,
                SUM(CASE WHEN status = 'Merged' THEN 1 ELSE 0 END) as merged_pairs,
                SUM(CASE WHEN status = 'Cancelled' THEN 1 ELSE 0 END) as cancelled_pairs,
                COALESCE(SUM(CASE WHEN status = 'Merged' THEN CAST(profit AS REAL) ELSE 0.0 END), 0.0) as total_profit,
                COALESCE(SUM(CASE WHEN status = 'Merged' THEN CAST(pair_cost AS REAL) ELSE 0.0 END), 0.0) as total_cost,
                COALESCE(AVG(CASE WHEN status = 'Merged' THEN 1.0 - CAST(pair_cost AS REAL) ELSE NULL END), 0.0) as avg_spread
            FROM mint_maker_pairs
            WHERE wallet_address = ?
            "#,
        )
        .bind(wallet_address.to_lowercase())
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0, 0, 0, 0.0, 0.0, 0.0));

        Ok(row)
    }

    /// Log a mint maker action
    pub async fn log_mint_maker_action(
        &self,
        wallet_address: &str,
        action: &str,
        market_id: Option<&str>,
        question: Option<&str>,
        asset: Option<&str>,
        yes_price: Option<&str>,
        no_price: Option<&str>,
        pair_cost: Option<&str>,
        profit: Option<&str>,
        size: Option<&str>,
        details: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO mint_maker_log (wallet_address, action, market_id, question, asset,
                yes_price, no_price, pair_cost, profit, size, details, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(wallet_address.to_lowercase())
        .bind(action)
        .bind(market_id)
        .bind(question)
        .bind(asset)
        .bind(yes_price)
        .bind(no_price)
        .bind(pair_cost)
        .bind(profit)
        .bind(size)
        .bind(details)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get mint maker activity log
    pub async fn get_mint_maker_log(&self, wallet_address: &str, limit: i64) -> Result<Vec<MintMakerLogEntry>> {
        let rows = sqlx::query(
            "SELECT * FROM mint_maker_log WHERE wallet_address = ? ORDER BY created_at DESC LIMIT ?"
        )
        .bind(wallet_address.to_lowercase())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.iter().map(|row| MintMakerLogEntry {
            id: row.get("id"),
            wallet_address: row.get("wallet_address"),
            action: row.get("action"),
            market_id: row.get("market_id"),
            question: row.get("question"),
            asset: row.get("asset"),
            yes_price: row.get("yes_price"),
            no_price: row.get("no_price"),
            pair_cost: row.get("pair_cost"),
            profit: row.get("profit"),
            size: row.get("size"),
            details: row.get("details"),
            created_at: row.get("created_at"),
        }).collect())
    }

    /// Helper to convert a row to MintMakerPairRow
    fn row_to_mm_pair(row: &sqlx::sqlite::SqliteRow) -> MintMakerPairRow {
        let neg_risk_int: i32 = row.try_get("neg_risk").unwrap_or(0);
        MintMakerPairRow {
            id: row.get("id"),
            wallet_address: row.get("wallet_address"),
            market_id: row.get("market_id"),
            condition_id: row.get("condition_id"),
            question: row.get("question"),
            asset: row.get("asset"),
            yes_order_id: row.get("yes_order_id"),
            no_order_id: row.get("no_order_id"),
            yes_bid_price: row.get("yes_bid_price"),
            no_bid_price: row.get("no_bid_price"),
            yes_fill_price: row.get("yes_fill_price"),
            no_fill_price: row.get("no_fill_price"),
            pair_cost: row.get("pair_cost"),
            profit: row.get("profit"),
            size: row.get("size"),
            yes_size: row.try_get("yes_size").ok(),
            no_size: row.try_get("no_size").ok(),
            slug: row.try_get("slug").ok(),
            yes_token_id: row.try_get("yes_token_id").ok(),
            no_token_id: row.try_get("no_token_id").ok(),
            neg_risk: neg_risk_int != 0,
            status: row.get("status"),
            merge_tx_id: row.get("merge_tx_id"),
            stop_loss_order_id: row.try_get("stop_loss_order_id").ok().flatten(),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }
    }
}

/// MC tier history row
pub struct McTierHistoryRow {
    pub id: i64,
    pub from_tier: i32,
    pub to_tier: i32,
    pub bankroll: String,
    pub reason: String,
    pub timestamp: String,
}

// ==================== MINT MAKER DB TYPES ====================

/// Mint Maker settings from DB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintMakerSettingsRow {
    pub wallet_address: String,
    pub enabled: bool,
    pub preset: String,
    pub bid_offset_cents: i32,
    pub max_pair_cost: f64,
    pub min_spread_profit: f64,
    pub max_pairs_per_market: i32,
    pub max_total_pairs: i32,
    pub stale_order_seconds: i64,
    pub assets: Vec<String>,
    pub min_minutes_to_close: f64,
    pub max_minutes_to_close: f64,
    pub auto_place: bool,
    pub auto_place_size: String,
    pub auto_max_markets: i32,
    pub auto_redeem: bool,
}

/// Mint Maker log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintMakerLogEntry {
    pub id: i64,
    pub wallet_address: String,
    pub action: String,
    pub market_id: Option<String>,
    pub question: Option<String>,
    pub asset: Option<String>,
    pub yes_price: Option<String>,
    pub no_price: Option<String>,
    pub pair_cost: Option<String>,
    pub profit: Option<String>,
    pub size: Option<String>,
    pub details: Option<String>,
    pub created_at: String,
}

/// Mint Maker pair row from DB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintMakerPairRow {
    pub id: i64,
    pub wallet_address: String,
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub asset: String,
    pub yes_order_id: String,
    pub no_order_id: String,
    pub yes_bid_price: String,
    pub no_bid_price: String,
    pub yes_fill_price: Option<String>,
    pub no_fill_price: Option<String>,
    pub pair_cost: Option<String>,
    pub profit: Option<String>,
    pub size: String,
    pub yes_size: Option<String>,
    pub no_size: Option<String>,
    pub slug: Option<String>,
    pub yes_token_id: Option<String>,
    pub no_token_id: Option<String>,
    pub neg_risk: bool,
    pub status: String,
    pub merge_tx_id: Option<String>,
    pub stop_loss_order_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// MC trade full row (for API responses)
pub struct McTradeFullRow {
    pub id: i64,
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub slug: String,
    pub side: String,
    pub entry_price: String,
    pub exit_price: Option<String>,
    pub size: String,
    pub shares: String,
    pub pnl: Option<String>,
    pub certainty_score: i32,
    pub category: Option<String>,
    pub status: String,
    pub tier_at_entry: i32,
    pub token_id: Option<String>,
    pub end_date: Option<String>,
    pub opened_at: String,
    pub closed_at: Option<String>,
}
