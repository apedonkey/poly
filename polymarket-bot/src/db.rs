//! SQLite database for tracking positions, orders, and statistics

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

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub wallet_address: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
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
                end_date TEXT
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
        pnl: Decimal,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE positions SET closed_at = ?, exit_price = ?, pnl = ?, status = 'Resolved' WHERE id = ?",
        )
        .bind(now)
        .bind(exit_price.to_string())
        .bind(pnl.to_string())
        .bind(id)
        .execute(&self.pool)
        .await?;
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
        let rows = sqlx::query(
            "SELECT * FROM positions WHERE status IN ('Open', 'PendingResolution')",
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
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM positions WHERE status = 'Resolved'")
            .fetch_one(&self.pool)
            .await?;

        let wins: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE status = 'Resolved' AND CAST(pnl AS REAL) > 0",
        )
        .fetch_one(&self.pool)
        .await?;

        let losses: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE status = 'Resolved' AND CAST(pnl AS REAL) <= 0",
        )
        .fetch_one(&self.pool)
        .await?;

        let total_pnl: Option<(String,)> = sqlx::query_as(
            "SELECT COALESCE(SUM(CAST(pnl AS REAL)), 0) FROM positions WHERE status = 'Resolved'",
        )
        .fetch_optional(&self.pool)
        .await?;

        let sniper_trades: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE strategy = 'ResolutionSniper' AND status = 'Resolved'",
        )
        .fetch_one(&self.pool)
        .await?;

        let sniper_wins: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE strategy = 'ResolutionSniper' AND status = 'Resolved' AND CAST(pnl AS REAL) > 0",
        )
        .fetch_one(&self.pool)
        .await?;

        let no_bias_trades: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE strategy = 'NoBias' AND status = 'Resolved'",
        )
        .fetch_one(&self.pool)
        .await?;

        let no_bias_wins: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM positions WHERE strategy = 'NoBias' AND status = 'Resolved' AND CAST(pnl AS REAL) > 0",
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

        Ok(Position {
            id: row.get("id"),
            market_id: row.get("market_id"),
            question: row.get("question"),
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
        side: Side,
        entry_price: Decimal,
        size: Decimal,
        strategy: StrategyType,
        is_paper: bool,
        end_date: Option<DateTime<Utc>>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let side_str = format!("{:?}", side);
        let strategy_str = format!("{:?}", strategy);
        let end_date_str = end_date.map(|d| d.to_rfc3339());

        let result = sqlx::query(
            r#"
            INSERT INTO positions (wallet_address, market_id, question, side, entry_price, size, strategy, opened_at, status, is_paper, end_date)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'Open', ?, ?)
            "#,
        )
        .bind(wallet_address)
        .bind(market_id)
        .bind(question)
        .bind(side_str)
        .bind(entry_price.to_string())
        .bind(size.to_string())
        .bind(strategy_str)
        .bind(now)
        .bind(is_paper)
        .bind(end_date_str)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get open positions for a specific wallet
    pub async fn get_positions_for_wallet(&self, wallet_address: &str) -> Result<Vec<Position>> {
        let rows = sqlx::query(
            "SELECT * FROM positions WHERE wallet_address = ? ORDER BY opened_at DESC",
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

    /// Get open positions for a specific wallet
    pub async fn get_open_positions_for_wallet(&self, wallet_address: &str) -> Result<Vec<Position>> {
        let rows = sqlx::query(
            "SELECT * FROM positions WHERE wallet_address = ? AND status IN ('Open', 'PendingResolution') ORDER BY opened_at DESC",
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
        // Single query to get all stats at once
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
            WHERE wallet_address = ? AND status = 'Resolved'
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
}
