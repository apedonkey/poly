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
                avg_exit_price TEXT
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
        no_bias_opps: i64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO scan_history (scanned_at, markets_found, sniper_opportunities, no_bias_opportunities) VALUES (?, ?, ?, ?)",
        )
        .bind(now)
        .bind(markets_found)
        .bind(sniper_opps)
        .bind(no_bias_opps)
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

        let no_bias_trades: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE strategy = 'NoBias' AND status IN ('Resolved', 'Closed')",
        )
        .fetch_one(&self.pool)
        .await?;

        let no_bias_wins: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE strategy = 'NoBias' AND status IN ('Resolved', 'Closed') AND CAST(pnl AS REAL) > 0",
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
            no_bias_trades: no_bias_trades.0,
            no_bias_wins: no_bias_wins.0,
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
            "ResolutionSniper" => StrategyType::ResolutionSniper,
            _ => StrategyType::NoBias,
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
        .bind(address)
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
            "SELECT encrypted_private_key, salt, nonce FROM wallets WHERE address = ?",
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
        sqlx::query("UPDATE wallets SET last_active = ? WHERE address = ?")
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
        .bind(wallet_address)
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
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let side_str = format!("{:?}", side);
        let strategy_str = format!("{:?}", strategy);
        let end_date_str = end_date.map(|d| d.to_rfc3339());

        // Calculate initial remaining_size as shares (size / entry_price)
        let shares = size / entry_price;

        let result = sqlx::query(
            r#"
            INSERT INTO positions (wallet_address, market_id, question, slug, side, entry_price, size, strategy, opened_at, status, is_paper, end_date, token_id, order_id, remaining_size, realized_pnl, total_sold_size)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'Open', ?, ?, ?, ?, ?, '0', '0')
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
        let row: (i64, i64, i64, f64, i64, i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) as total,
                SUM(CASE WHEN CAST(pnl AS REAL) > 0 THEN 1 ELSE 0 END) as wins,
                SUM(CASE WHEN CAST(pnl AS REAL) <= 0 THEN 1 ELSE 0 END) as losses,
                COALESCE(SUM(CAST(pnl AS REAL)), 0) as total_pnl,
                SUM(CASE WHEN strategy = 'ResolutionSniper' THEN 1 ELSE 0 END) as sniper_trades,
                SUM(CASE WHEN strategy = 'ResolutionSniper' AND CAST(pnl AS REAL) > 0 THEN 1 ELSE 0 END) as sniper_wins,
                SUM(CASE WHEN strategy = 'NoBias' THEN 1 ELSE 0 END) as no_bias_trades,
                SUM(CASE WHEN strategy = 'NoBias' AND CAST(pnl AS REAL) > 0 THEN 1 ELSE 0 END) as no_bias_wins
            FROM positions
            WHERE wallet_address = ? AND status IN ('Resolved', 'Closed') AND is_paper = 0
            "#,
        )
        .bind(wallet_address)
        .fetch_one(&self.pool)
        .await
        .unwrap_or((0, 0, 0, 0.0, 0, 0, 0, 0));

        let pnl_decimal = Decimal::from_f64_retain(row.3).unwrap_or_default();

        Ok(BotStats {
            total_trades: row.0,
            winning_trades: row.1,
            losing_trades: row.2,
            total_pnl: pnl_decimal,
            sniper_trades: row.4,
            sniper_wins: row.5,
            no_bias_trades: row.6,
            no_bias_wins: row.7,
            avg_hold_time_hours: 0.0,
        })
    }

    /// Close a position and calculate PnL
    pub async fn close_position_for_wallet(
        &self,
        wallet_address: &str,
        position_id: i64,
        exit_price: Decimal,
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
        let side: String = row.get("side");

        let entry = Decimal::from_str(&entry_price)?;
        let size_dec = Decimal::from_str(&size)?;

        // Calculate PnL: (exit_price - entry_price) * shares
        // Shares = size / entry_price
        let shares = size_dec / entry;

        // For both YES and NO positions: profit if you sell at a higher price than you bought
        // (You bought tokens at entry_price, sold at exit_price)
        let pnl = (exit_price - entry) * shares;

        // Update the position
        sqlx::query(
            r#"
            UPDATE positions
            SET status = 'Closed', exit_price = ?, pnl = ?, closed_at = ?
            WHERE id = ? AND wallet_address = ?
            "#,
        )
        .bind(exit_price.to_string())
        .bind(pnl.to_string())
        .bind(Utc::now().to_rfc3339())
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
                    max_position_size: Decimal::from_str(r.get::<&str, _>("max_position_size")).unwrap_or(Decimal::from(50)),
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
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(settings.wallet_address.to_lowercase())
        .bind(settings.enabled as i32)
        .bind(settings.auto_buy_enabled as i32)
        .bind(settings.max_position_size.to_string())
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
                max_daily_loss = ?, updated_at = ?
            WHERE wallet_address = ?
            "#,
        )
        .bind(settings.enabled as i32)
        .bind(settings.auto_buy_enabled as i32)
        .bind(settings.max_position_size.to_string())
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
}
