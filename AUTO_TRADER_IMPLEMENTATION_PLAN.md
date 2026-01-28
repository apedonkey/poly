# Auto-Trader Implementation Plan v2

## Overview

A fully automated trading system with a dedicated **Auto-Trade tab** in the web UI. Runs independently from manual trading. Supports multiple users with per-wallet settings.

---

## Final Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              WEB UI                                         │
├─────────────────────────────────────────────────────────────────────────────┤
│  [Opportunities]    [Positions]    [⚡ Auto-Trade]                          │
│                                          │                                  │
│                                          ▼                                  │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │  ⚡ AUTO-TRADING                                        [● ENABLED]   │  │
│  ├───────────────────────────────────────────────────────────────────────┤  │
│  │                                                                       │  │
│  │  ┌─────────────────────┐  ┌─────────────────────┐  ┌───────────────┐  │  │
│  │  │ 📈 AUTO-BUY         │  │ 💰 AUTO-SELL        │  │ 📊 STATS      │  │  │
│  │  │ ○ Disabled          │  │ ✓ Take Profit: 20% │  │ Trades: 47    │  │  │
│  │  │ Max: $50/trade      │  │ ✓ Stop Loss: 10%   │  │ Win Rate: 72% │  │  │
│  │  │ Strategies:         │  │ ○ Trailing Stop    │  │ P&L: +$234    │  │  │
│  │  │  ☑ Sniper           │  │ ○ Time Exit: 24h   │  │               │  │  │
│  │  │  ☐ NO Bias          │  │                    │  │               │  │  │
│  │  └─────────────────────┘  └─────────────────────┘  └───────────────┘  │  │
│  │                                                                       │  │
│  │  ┌───────────────────────────────────────────────────────────────────┐│  │
│  │  │ 📋 ACTIVITY LOG                                                   ││  │
│  │  │ 🟢 Auto-buy: "Trump wins?" YES @ 72¢ ($50)           2 min ago   ││  │
│  │  │ 💰 Take-profit: "BTC > 100k" @ 91¢ → +$15.20        15 min ago   ││  │
│  │  │ 🔴 Stop-loss: "ETH ETF approved" @ 38¢ → -$8.50      1 hour ago  ││  │
│  │  │ ⏱️ Time-exit: "Fed rate cut" @ 65¢ → +$3.20          3 hours ago ││  │
│  │  └───────────────────────────────────────────────────────────────────┘│  │
│  └───────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ REST API + WebSocket
                                    ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              BACKEND                                        │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    AUTO-TRADER SERVICE (NEW)                        │   │
│  ├─────────────────────────────────────────────────────────────────────┤   │
│  │                                                                     │   │
│  │  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐           │   │
│  │  │ POSITION      │  │ OPPORTUNITY   │  │ ORDER         │           │   │
│  │  │ MONITOR       │  │ WATCHER       │  │ EXECUTOR      │           │   │
│  │  ├───────────────┤  ├───────────────┤  ├───────────────┤           │   │
│  │  │ • Price feed  │  │ • New opps    │  │ • Sign orders │           │   │
│  │  │ • TP/SL check │  │ • Buy checks  │  │ • Submit CLOB │           │   │
│  │  │ • Trailing    │  │ • Filters     │  │ • Record DB   │           │   │
│  │  │ • Time exit   │  │ • Cooldowns   │  │ • Log trades  │           │   │
│  │  └───────────────┘  └───────────────┘  └───────────────┘           │   │
│  │                              │                                      │   │
│  │                              ▼                                      │   │
│  │                    ┌───────────────────┐                            │   │
│  │                    │    DATABASE       │                            │   │
│  │                    │ auto_trading_     │                            │   │
│  │                    │ settings + logs   │                            │   │
│  │                    └───────────────────┘                            │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                    EXISTING (UNTOUCHED)                             │   │
│  │  Scanner → Opportunities → WebSocket → Manual Trading               │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Phase 1: Database & Backend Foundation

### 1.1 New Database Tables

```sql
-- Auto-trading settings per wallet
CREATE TABLE IF NOT EXISTS auto_trading_settings (
    wallet_address TEXT PRIMARY KEY,

    -- Master switch
    enabled INTEGER DEFAULT 0,

    -- Auto-buy settings
    auto_buy_enabled INTEGER DEFAULT 0,
    max_position_size TEXT DEFAULT '50',
    max_total_exposure TEXT DEFAULT '500',
    min_edge REAL DEFAULT 0.05,
    strategies TEXT DEFAULT '["sniper"]',  -- JSON array

    -- Take profit
    take_profit_enabled INTEGER DEFAULT 1,
    take_profit_percent REAL DEFAULT 0.20,

    -- Stop loss
    stop_loss_enabled INTEGER DEFAULT 1,
    stop_loss_percent REAL DEFAULT 0.10,

    -- Trailing stop
    trailing_stop_enabled INTEGER DEFAULT 0,
    trailing_stop_percent REAL DEFAULT 0.10,

    -- Time-based exit
    time_exit_enabled INTEGER DEFAULT 0,
    time_exit_hours REAL DEFAULT 24.0,

    -- Risk limits
    max_positions INTEGER DEFAULT 10,
    cooldown_minutes INTEGER DEFAULT 5,
    max_daily_loss TEXT DEFAULT '100',

    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,

    FOREIGN KEY (wallet_address) REFERENCES wallets(address)
);

-- Auto-trade activity log
CREATE TABLE IF NOT EXISTS auto_trade_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    wallet_address TEXT NOT NULL,
    position_id INTEGER,
    action TEXT NOT NULL,  -- 'auto_buy', 'take_profit', 'stop_loss', 'trailing_stop', 'time_exit'
    market_question TEXT,
    side TEXT,
    entry_price TEXT,
    exit_price TEXT,
    size TEXT,
    pnl TEXT,
    trigger_reason TEXT,
    created_at TEXT NOT NULL,

    FOREIGN KEY (wallet_address) REFERENCES wallets(address)
);

-- Price peaks for trailing stop
CREATE TABLE IF NOT EXISTS position_peaks (
    position_id INTEGER PRIMARY KEY,
    peak_price TEXT NOT NULL,
    peak_at TEXT NOT NULL,

    FOREIGN KEY (position_id) REFERENCES positions(id)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_auto_settings_enabled ON auto_trading_settings(enabled);
CREATE INDEX IF NOT EXISTS idx_auto_log_wallet ON auto_trade_log(wallet_address);
CREATE INDEX IF NOT EXISTS idx_auto_log_created ON auto_trade_log(created_at DESC);
```

### 1.2 New Backend Files

```
polymarket-bot/src/
├── services/
│   ├── mod.rs                      # Add: pub mod auto_trader;
│   ├── auto_trader/
│   │   ├── mod.rs                  # Module exports
│   │   ├── config.rs               # AutoTradingSettings struct
│   │   ├── monitor.rs              # Position price monitoring
│   │   ├── buyer.rs                # Auto-buy logic
│   │   ├── seller.rs               # Auto-sell (TP/SL) logic
│   │   ├── executor.rs             # CLOB order execution
│   │   └── types.rs                # ExitTrigger, TradeAction enums
├── api/routes/
│   ├── mod.rs                      # Add: pub mod auto_trading;
│   ├── auto_trading.rs             # NEW: API endpoints
```

### 1.3 API Endpoints

```
GET  /api/auto-trading/settings     → AutoTradingSettings
PUT  /api/auto-trading/settings     → Update settings
POST /api/auto-trading/enable       → Enable auto-trading
POST /api/auto-trading/disable      → Disable auto-trading
GET  /api/auto-trading/status       → Running status, active positions
GET  /api/auto-trading/history      → Activity log (paginated)
GET  /api/auto-trading/stats        → Performance statistics
```

### 1.4 WebSocket Messages (New)

```typescript
// Backend → Frontend
{ type: 'auto_trade_executed', data: AutoTradeLog }
{ type: 'auto_trading_status', data: { enabled, active_positions, daily_pnl } }
```

### 1.5 Deliverables
- [ ] Database migrations in `db.rs`
- [ ] `AutoTradingSettings` struct with defaults
- [ ] Database methods: `get_auto_trading_settings()`, `update_auto_trading_settings()`, `log_auto_trade()`, `get_auto_trade_history()`
- [ ] API routes in `auto_trading.rs`
- [ ] WebSocket message types

---

## Phase 2: Auto-Seller Service (Take-Profit / Stop-Loss)

### 2.1 Position Monitor

Watches prices for all positions where auto-trading is enabled.

```rust
// src/services/auto_trader/monitor.rs

pub struct PositionMonitor {
    db: Arc<Database>,
}

impl PositionMonitor {
    /// Run monitoring loop
    pub async fn run(
        &self,
        mut price_rx: broadcast::Receiver<PriceUpdate>,
        trade_tx: broadcast::Sender<AutoTradeLog>,
    ) {
        while let Ok(price_update) = price_rx.recv().await {
            self.handle_price_update(&price_update, &trade_tx).await;
        }
    }

    async fn handle_price_update(&self, update: &PriceUpdate, trade_tx: &broadcast::Sender<AutoTradeLog>) {
        // 1. Find positions with this token_id
        let positions = self.db.get_positions_by_token_id(&update.token_id).await?;

        for position in positions {
            // 2. Get wallet's auto-trading settings
            let settings = self.db.get_auto_trading_settings(&position.wallet_address).await?;

            if !settings.enabled {
                continue;
            }

            // 3. Update peak price for trailing stop
            if settings.trailing_stop_enabled {
                self.update_peak_price(position.id, &update.price).await?;
            }

            // 4. Check exit triggers
            if let Some(trigger) = self.check_triggers(&position, &update.price, &settings) {
                self.execute_sell(&position, trigger, trade_tx).await?;
            }
        }
    }

    fn check_triggers(&self, position: &Position, current_price: &Decimal, settings: &AutoTradingSettings) -> Option<ExitTrigger> {
        let entry = position.entry_price;
        let pnl_percent = (current_price - entry) / entry;

        // Take profit
        if settings.take_profit_enabled && pnl_percent >= settings.take_profit_percent {
            return Some(ExitTrigger::TakeProfit {
                price: *current_price,
                pnl_percent
            });
        }

        // Stop loss
        if settings.stop_loss_enabled && pnl_percent <= -settings.stop_loss_percent {
            return Some(ExitTrigger::StopLoss {
                price: *current_price,
                pnl_percent
            });
        }

        // Trailing stop
        if settings.trailing_stop_enabled {
            if let Some(peak) = self.get_peak_price(position.id) {
                let drop_percent = (peak - current_price) / peak;
                if drop_percent >= settings.trailing_stop_percent {
                    return Some(ExitTrigger::TrailingStop {
                        peak,
                        price: *current_price,
                        drop_percent
                    });
                }
            }
        }

        // Time exit
        if settings.time_exit_enabled {
            let hours_held = position.hours_since_open();
            if hours_held >= settings.time_exit_hours {
                return Some(ExitTrigger::TimeExit {
                    hours_held,
                    price: *current_price
                });
            }
        }

        None
    }
}
```

### 2.2 Auto-Sell Executor

```rust
// src/services/auto_trader/seller.rs

impl AutoSeller {
    pub async fn execute_sell(
        &self,
        position: &Position,
        trigger: ExitTrigger,
        trade_tx: &broadcast::Sender<AutoTradeLog>,
    ) -> Result<()> {
        let wallet = &position.wallet_address;

        // 1. Get API credentials
        let creds = self.db.get_api_credentials(wallet).await?
            .ok_or_else(|| anyhow!("No API credentials"))?;

        // 2. Get current best bid
        let best_bid = self.get_best_bid(&position.token_id).await?;

        // 3. Calculate shares to sell
        let shares = position.remaining_shares();

        // 4. Create sell order
        let order = self.create_sell_order(
            &position.token_id,
            shares,
            best_bid,
        ).await?;

        // 5. Sign and submit to CLOB
        let result = self.submit_order(&order, &creds).await?;

        // 6. Update position in DB
        let close_result = self.db.partial_close_position_for_wallet(
            wallet, position.id, shares, best_bid
        ).await?;

        // 7. Log the auto-trade
        let log = AutoTradeLog {
            wallet_address: wallet.clone(),
            position_id: Some(position.id),
            action: trigger.action_name(),
            market_question: Some(position.question.clone()),
            side: Some(position.side.to_string()),
            entry_price: Some(position.entry_price),
            exit_price: Some(best_bid),
            size: Some(position.size),
            pnl: Some(close_result.pnl_this_sell),
            trigger_reason: trigger.reason(),
            created_at: Utc::now(),
        };

        self.db.log_auto_trade(&log).await?;

        // 8. Broadcast to WebSocket
        let _ = trade_tx.send(log);

        Ok(())
    }
}
```

### 2.3 Deliverables
- [ ] `PositionMonitor` service with price subscription
- [ ] `AutoSeller` with CLOB sell order logic
- [ ] `ExitTrigger` enum (TakeProfit, StopLoss, TrailingStop, TimeExit)
- [ ] Peak price tracking for trailing stops
- [ ] Integration with existing `PriceWebSocket`
- [ ] WebSocket broadcast of auto-trades

---

## Phase 3: Auto-Buyer Service

### 3.1 Opportunity Watcher

```rust
// src/services/auto_trader/buyer.rs

pub struct AutoBuyer {
    db: Arc<Database>,
}

impl AutoBuyer {
    pub async fn run(
        &self,
        mut opp_rx: broadcast::Receiver<Vec<Opportunity>>,
        trade_tx: broadcast::Sender<AutoTradeLog>,
    ) {
        while let Ok(opportunities) = opp_rx.recv().await {
            self.process_opportunities(&opportunities, &trade_tx).await;
        }
    }

    async fn process_opportunities(
        &self,
        opportunities: &[Opportunity],
        trade_tx: &broadcast::Sender<AutoTradeLog>,
    ) {
        // Get all wallets with auto-buy enabled
        let wallets = self.db.get_auto_buy_enabled_wallets().await?;

        for wallet_address in wallets {
            let settings = self.db.get_auto_trading_settings(&wallet_address).await?;

            for opp in opportunities {
                if self.should_buy(&wallet_address, opp, &settings).await {
                    if let Err(e) = self.execute_buy(&wallet_address, opp, &settings, trade_tx).await {
                        tracing::warn!("Auto-buy failed: {}", e);
                    }
                }
            }
        }
    }

    async fn should_buy(&self, wallet: &str, opp: &Opportunity, settings: &AutoTradingSettings) -> bool {
        // 1. Strategy enabled?
        let strategy_ok = match opp.strategy {
            StrategyType::ResolutionSniper => settings.strategies.contains(&"sniper".to_string()),
            StrategyType::NoBias => settings.strategies.contains(&"no_bias".to_string()),
        };
        if !strategy_ok { return false; }

        // 2. Meets criteria? (price in range, has edge)
        if !opp.meets_criteria { return false; }

        // 3. Minimum edge?
        if opp.edge < settings.min_edge { return false; }

        // 4. Already have position?
        if self.db.has_open_position(wallet, &opp.market_id).await.unwrap_or(true) {
            return false;
        }

        // 5. Max positions?
        let count = self.db.count_open_positions(wallet).await.unwrap_or(999);
        if count >= settings.max_positions { return false; }

        // 6. Max exposure?
        let exposure = self.db.get_total_exposure(wallet).await.unwrap_or(Decimal::MAX);
        if exposure >= settings.max_total_exposure { return false; }

        // 7. Cooldown?
        if self.db.is_in_cooldown(wallet, &opp.market_id, settings.cooldown_minutes).await.unwrap_or(true) {
            return false;
        }

        // 8. Daily loss limit?
        let daily_pnl = self.db.get_daily_auto_pnl(wallet).await.unwrap_or(Decimal::ZERO);
        if daily_pnl <= -settings.max_daily_loss { return false; }

        // 9. Has USDC balance?
        // (Could check via RPC, or trust that we track it)

        true
    }

    async fn execute_buy(
        &self,
        wallet: &str,
        opp: &Opportunity,
        settings: &AutoTradingSettings,
        trade_tx: &broadcast::Sender<AutoTradeLog>,
    ) -> Result<()> {
        // 1. Get API credentials
        let creds = self.db.get_api_credentials(wallet).await?
            .ok_or_else(|| anyhow!("No API credentials"))?;

        // 2. Calculate size (scale by edge)
        let edge_factor = (opp.edge / 0.20).min(1.0);  // Scale up to 20% edge
        let size = settings.max_position_size * Decimal::try_from(edge_factor)?;

        // 3. Get best ask
        let best_ask = self.get_best_ask(&opp.token_id).await?;

        // 4. Create buy order
        let order = self.create_buy_order(&opp.token_id, size, best_ask).await?;

        // 5. Sign and submit
        let result = self.submit_order(&order, &creds).await?;

        // 6. Record position
        let position_id = self.db.create_position_for_wallet(
            wallet,
            &opp.market_id,
            &opp.question,
            Some(&opp.slug),
            opp.side,
            best_ask,
            size,
            opp.strategy,
            false,  // Live
            opp.time_to_close_hours.map(|h| Utc::now() + Duration::hours(h as i64)),
            opp.token_id.as_deref(),
            result.order_id.as_deref(),
        ).await?;

        // 7. Set cooldown
        self.db.set_cooldown(wallet, &opp.market_id).await?;

        // 8. Log
        let log = AutoTradeLog {
            wallet_address: wallet.to_string(),
            position_id: Some(position_id),
            action: "auto_buy".to_string(),
            market_question: Some(opp.question.clone()),
            side: Some(opp.side.to_string()),
            entry_price: Some(best_ask),
            exit_price: None,
            size: Some(size),
            pnl: None,
            trigger_reason: format!("Edge: {:.1}%, Strategy: {:?}", opp.edge * 100.0, opp.strategy),
            created_at: Utc::now(),
        };

        self.db.log_auto_trade(&log).await?;
        let _ = trade_tx.send(log);

        Ok(())
    }
}
```

### 3.2 Deliverables
- [ ] `AutoBuyer` service
- [ ] Buy criteria checks (10 conditions)
- [ ] Position sizing (edge-scaled)
- [ ] Cooldown tracking
- [ ] Daily loss limit enforcement
- [ ] Integration with opportunity broadcast

---

## Phase 4: Frontend - Auto-Trade Tab

### 4.1 New Files

```
polymarket-web/src/
├── components/
│   ├── auto-trading/
│   │   ├── AutoTradingPanel.tsx      # Main tab component
│   │   ├── AutoBuySettings.tsx       # Auto-buy config card
│   │   ├── AutoSellSettings.tsx      # TP/SL config card
│   │   ├── AutoTradingStats.tsx      # Performance stats card
│   │   ├── ActivityLog.tsx           # Trade history list
│   │   ├── ActivityLogItem.tsx       # Single log entry
│   │   └── SettingsToggle.tsx        # Reusable toggle component
├── stores/
│   └── autoTradingStore.ts           # Zustand store for settings
├── api/
│   └── client.ts                     # Add auto-trading endpoints
├── types.ts                          # Add AutoTradingSettings, AutoTradeLog
```

### 4.2 Types

```typescript
// Add to types.ts

export interface AutoTradingSettings {
  enabled: boolean

  // Auto-buy
  auto_buy_enabled: boolean
  max_position_size: string
  max_total_exposure: string
  min_edge: number
  strategies: string[]  // ['sniper', 'no_bias']

  // Take profit
  take_profit_enabled: boolean
  take_profit_percent: number

  // Stop loss
  stop_loss_enabled: boolean
  stop_loss_percent: number

  // Trailing stop
  trailing_stop_enabled: boolean
  trailing_stop_percent: number

  // Time exit
  time_exit_enabled: boolean
  time_exit_hours: number

  // Risk
  max_positions: number
  cooldown_minutes: number
  max_daily_loss: string
}

export interface AutoTradeLog {
  id: number
  wallet_address: string
  position_id: number | null
  action: 'auto_buy' | 'take_profit' | 'stop_loss' | 'trailing_stop' | 'time_exit'
  market_question: string | null
  side: 'Yes' | 'No' | null
  entry_price: string | null
  exit_price: string | null
  size: string | null
  pnl: string | null
  trigger_reason: string | null
  created_at: string
}

export interface AutoTradingStats {
  total_trades: number
  win_rate: number
  total_pnl: string
  take_profit_count: number
  stop_loss_count: number
  trailing_stop_count: number
  auto_buy_count: number
  best_trade_pnl: string
  worst_trade_pnl: string
  avg_hold_hours: number
}
```

### 4.3 API Client Functions

```typescript
// Add to api/client.ts

export async function getAutoTradingSettings(sessionToken: string): Promise<AutoTradingSettings> {
  return fetchJson(`${API_BASE}/auto-trading/settings`, {
    headers: { Authorization: `Bearer ${sessionToken}` },
  })
}

export async function updateAutoTradingSettings(
  sessionToken: string,
  settings: Partial<AutoTradingSettings>
): Promise<AutoTradingSettings> {
  return fetchJson(`${API_BASE}/auto-trading/settings`, {
    method: 'PUT',
    headers: { Authorization: `Bearer ${sessionToken}` },
    body: JSON.stringify(settings),
  })
}

export async function enableAutoTrading(sessionToken: string): Promise<void> {
  return fetchJson(`${API_BASE}/auto-trading/enable`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${sessionToken}` },
  })
}

export async function disableAutoTrading(sessionToken: string): Promise<void> {
  return fetchJson(`${API_BASE}/auto-trading/disable`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${sessionToken}` },
  })
}

export async function getAutoTradingHistory(
  sessionToken: string,
  limit = 50,
  offset = 0
): Promise<AutoTradeLog[]> {
  return fetchJson(`${API_BASE}/auto-trading/history?limit=${limit}&offset=${offset}`, {
    headers: { Authorization: `Bearer ${sessionToken}` },
  })
}

export async function getAutoTradingStats(sessionToken: string): Promise<AutoTradingStats> {
  return fetchJson(`${API_BASE}/auto-trading/stats`, {
    headers: { Authorization: `Bearer ${sessionToken}` },
  })
}
```

### 4.4 Main Panel Component

```tsx
// components/auto-trading/AutoTradingPanel.tsx

import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Zap, Settings, TrendingUp, TrendingDown, Clock, Activity } from 'lucide-react'
import { useWalletStore } from '../../stores/walletStore'
import { getAutoTradingSettings, updateAutoTradingSettings, enableAutoTrading, disableAutoTrading, getAutoTradingHistory, getAutoTradingStats } from '../../api/client'
import { AutoBuySettings } from './AutoBuySettings'
import { AutoSellSettings } from './AutoSellSettings'
import { AutoTradingStats } from './AutoTradingStats'
import { ActivityLog } from './ActivityLog'

export function AutoTradingPanel() {
  const { sessionToken, isConnected } = useWalletStore()
  const queryClient = useQueryClient()

  // Fetch settings
  const { data: settings, isLoading } = useQuery({
    queryKey: ['auto-trading-settings', sessionToken],
    queryFn: () => getAutoTradingSettings(sessionToken!),
    enabled: isConnected(),
  })

  // Fetch history
  const { data: history } = useQuery({
    queryKey: ['auto-trading-history', sessionToken],
    queryFn: () => getAutoTradingHistory(sessionToken!),
    enabled: isConnected(),
    refetchInterval: 10000,  // Refresh every 10s
  })

  // Fetch stats
  const { data: stats } = useQuery({
    queryKey: ['auto-trading-stats', sessionToken],
    queryFn: () => getAutoTradingStats(sessionToken!),
    enabled: isConnected(),
    refetchInterval: 30000,
  })

  // Toggle mutation
  const toggleMutation = useMutation({
    mutationFn: async (enabled: boolean) => {
      if (enabled) {
        await enableAutoTrading(sessionToken!)
      } else {
        await disableAutoTrading(sessionToken!)
      }
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['auto-trading-settings'] })
    },
  })

  // Update settings mutation
  const updateMutation = useMutation({
    mutationFn: (newSettings: Partial<AutoTradingSettings>) =>
      updateAutoTradingSettings(sessionToken!, newSettings),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['auto-trading-settings'] })
    },
  })

  if (!isConnected()) {
    return (
      <div className="text-center py-12">
        <Zap className="w-12 h-12 text-gray-600 mx-auto mb-4" />
        <h3 className="text-lg font-semibold mb-2">Connect Your Wallet</h3>
        <p className="text-gray-400">Connect your wallet to configure auto-trading.</p>
      </div>
    )
  }

  if (isLoading) {
    return <div className="text-center py-12 text-gray-400">Loading settings...</div>
  }

  return (
    <div className="space-y-6">
      {/* Header with master toggle */}
      <div className="flex items-center justify-between bg-poly-card rounded-xl border border-poly-border p-4">
        <div className="flex items-center gap-3">
          <Zap className={`w-6 h-6 ${settings?.enabled ? 'text-poly-green' : 'text-gray-500'}`} />
          <div>
            <h2 className="text-lg font-semibold">Auto-Trading</h2>
            <p className="text-sm text-gray-400">
              {settings?.enabled ? 'Monitoring positions for auto-sell triggers' : 'Disabled'}
            </p>
          </div>
        </div>

        {/* Master toggle */}
        <button
          onClick={() => toggleMutation.mutate(!settings?.enabled)}
          disabled={toggleMutation.isPending}
          className={`relative w-14 h-8 rounded-full transition-colors ${
            settings?.enabled ? 'bg-poly-green' : 'bg-gray-600'
          }`}
        >
          <span className={`absolute top-1 w-6 h-6 bg-white rounded-full transition-transform ${
            settings?.enabled ? 'translate-x-7' : 'translate-x-1'
          }`} />
        </button>
      </div>

      {/* Settings cards */}
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
        <AutoBuySettings
          settings={settings}
          onUpdate={(s) => updateMutation.mutate(s)}
          disabled={!settings?.enabled}
        />
        <AutoSellSettings
          settings={settings}
          onUpdate={(s) => updateMutation.mutate(s)}
          disabled={!settings?.enabled}
        />
        <AutoTradingStats stats={stats} />
      </div>

      {/* Activity log */}
      <ActivityLog history={history || []} />
    </div>
  )
}
```

### 4.5 Update App.tsx

```tsx
// App.tsx - Add the new tab

import { AutoTradingPanel } from './components/auto-trading/AutoTradingPanel'
import { Zap } from 'lucide-react'

type Tab = 'opportunities' | 'positions' | 'auto-trading'

// In nav section, add:
<button
  onClick={() => setActiveTab('auto-trading')}
  className={`flex items-center justify-center gap-1.5 sm:gap-2 px-3 sm:px-4 py-3 border-b-2 transition flex-1 sm:flex-none touch-target ${
    activeTab === 'auto-trading'
      ? 'border-poly-green text-poly-green'
      : 'border-transparent text-gray-400 hover:text-white'
  }`}
>
  <Zap className="w-4 h-4" />
  <span className="text-sm sm:text-base">Auto-Trade</span>
</button>

// In main section, add:
{activeTab === 'auto-trading' && <AutoTradingPanel />}
```

### 4.6 WebSocket Integration

```typescript
// In useWebSocket.ts, add handler for auto-trade events

case 'auto_trade_executed':
  // Dispatch event for ActivityLog to catch
  window.dispatchEvent(new CustomEvent('auto-trade', { detail: msg.data }))
  // Invalidate queries to refresh data
  queryClient.invalidateQueries({ queryKey: ['auto-trading-history'] })
  queryClient.invalidateQueries({ queryKey: ['auto-trading-stats'] })
  queryClient.invalidateQueries({ queryKey: ['positions'] })
  break
```

### 4.7 Deliverables
- [ ] `AutoTradingPanel.tsx` main component
- [ ] `AutoBuySettings.tsx` card
- [ ] `AutoSellSettings.tsx` card
- [ ] `AutoTradingStats.tsx` card
- [ ] `ActivityLog.tsx` with real-time updates
- [ ] `autoTradingStore.ts` (if needed for local state)
- [ ] API client functions
- [ ] WebSocket handler for auto-trade events
- [ ] Update `App.tsx` with new tab

---

## Phase 5: Integration & Testing

### 5.1 Backend Service Startup

```rust
// In bin/server.rs, start auto-trader alongside other services

// Start auto-trader services
let auto_trader_db = Arc::clone(&db);
let auto_trade_tx = broadcast::channel(100).0;

// Position monitor (watches prices, triggers sells)
let monitor_db = Arc::clone(&auto_trader_db);
let monitor_price_rx = price_tx.subscribe();
let monitor_trade_tx = auto_trade_tx.clone();
tokio::spawn(async move {
    let monitor = PositionMonitor::new(monitor_db);
    monitor.run(monitor_price_rx, monitor_trade_tx).await;
});

// Auto-buyer (watches opportunities, executes buys)
let buyer_db = Arc::clone(&auto_trader_db);
let buyer_opp_rx = opportunity_tx.subscribe();
let buyer_trade_tx = auto_trade_tx.clone();
tokio::spawn(async move {
    let buyer = AutoBuyer::new(buyer_db);
    buyer.run(buyer_opp_rx, buyer_trade_tx).await;
});

// Broadcast auto-trades to WebSocket clients
let ws_trade_rx = auto_trade_tx.subscribe();
// ... wire into WebSocket handler
```

### 5.2 Testing Checklist

- [ ] **Unit tests**: Exit trigger calculations
- [ ] **Integration tests**: Full buy/sell flow with mock CLOB
- [ ] **Paper trading mode**: Test without real orders
- [ ] **Multi-user**: Verify wallet isolation
- [ ] **Edge cases**: No credentials, insufficient balance, CLOB errors
- [ ] **Rate limiting**: Don't spam CLOB API

### 5.3 Deliverables
- [ ] Service startup in `server.rs`
- [ ] Error handling and logging
- [ ] Paper trading mode for auto-trader
- [ ] Test coverage

---

## Phase 6: Advanced Features (Future)

### 6.1 Discord Notifications

```rust
// Notify on auto-trades
async fn notify_trade(&self, log: &AutoTradeLog) {
    let emoji = match log.action.as_str() {
        "auto_buy" => "🟢",
        "take_profit" => "💰",
        "stop_loss" => "🔴",
        "trailing_stop" => "📉",
        "time_exit" => "⏱️",
        _ => "📊",
    };

    let message = format!(
        "{} **{}** | {} {} @ {}¢ | {}",
        emoji, log.action.to_uppercase(),
        log.side, log.market_question,
        log.exit_price.or(log.entry_price),
        log.pnl.map(|p| format!("P&L: ${}", p)).unwrap_or_default()
    );

    self.send_discord(&message).await;
}
```

### 6.2 Advanced Risk Management

- Daily loss limit (pause auto-trading if exceeded)
- Consecutive loss limit
- Market correlation checks
- Volatility-adjusted position sizing

### 6.3 Backtesting

- Historical opportunity replay
- Simulated P&L calculation
- Parameter optimization

---

## Implementation Order

| Week | Phase | Tasks |
|------|-------|-------|
| 1 | Phase 1 | Database schema, API routes, types |
| 2 | Phase 2 | Position monitor, auto-sell logic |
| 3 | Phase 3 | Auto-buyer, opportunity watcher |
| 4 | Phase 4 | Frontend tab, settings UI |
| 5 | Phase 5 | Integration, testing, fixes |
| 6 | Phase 6 | Notifications, polish |

---

## Default Settings

```typescript
const DEFAULT_SETTINGS: AutoTradingSettings = {
  enabled: false,

  // Auto-buy (OFF by default - user must opt-in)
  auto_buy_enabled: false,
  max_position_size: '50',
  max_total_exposure: '500',
  min_edge: 0.05,
  strategies: ['sniper'],

  // Take profit (ON by default)
  take_profit_enabled: true,
  take_profit_percent: 0.20,

  // Stop loss (ON by default)
  stop_loss_enabled: true,
  stop_loss_percent: 0.10,

  // Trailing stop (OFF by default)
  trailing_stop_enabled: false,
  trailing_stop_percent: 0.10,

  // Time exit (OFF by default)
  time_exit_enabled: false,
  time_exit_hours: 24,

  // Risk
  max_positions: 10,
  cooldown_minutes: 5,
  max_daily_loss: '100',
}
```

---

## Multi-User Support

The system is **multi-user by design**:

1. **Settings stored per wallet** - Each wallet has its own row in `auto_trading_settings`
2. **Positions isolated** - Queries always filter by `wallet_address`
3. **Credentials separate** - Each wallet has its own CLOB API keys
4. **Activity log per wallet** - `auto_trade_log` has `wallet_address` column
5. **Independent monitoring** - All enabled wallets monitored in parallel

```
User A                          User B
┌─────────────────┐            ┌─────────────────┐
│ enabled: true   │            │ enabled: true   │
│ TP: 20%         │            │ TP: 15%         │
│ SL: 10%         │            │ SL: 5%          │
│ auto_buy: ON    │            │ auto_buy: OFF   │
│ positions: 3    │            │ positions: 5    │
└────────┬────────┘            └────────┬────────┘
         │                              │
         └──────────┬───────────────────┘
                    │
                    ▼
           ┌───────────────┐
           │ AUTO-TRADER   │
           │ (monitors all │
           │  in parallel) │
           └───────────────┘
```

---

## Security Notes

1. **API credentials never leave backend** - Frontend only sends session token
2. **No private keys in auto-trader** - Uses CLOB L2 auth (API keys)
3. **Rate limiting** - Prevent abuse
4. **Audit logging** - All auto-trades recorded
5. **Circuit breaker** - Pause on repeated failures

---

## Sources

- [Polymarket CLOB Orders](https://docs.polymarket.com/developers/CLOB/orders/create-order)
- [Polymarket L2 Auth](https://docs.polymarket.com/developers/CLOB/clients/methods-l2)
- [Polymarket Trading Guide](https://docs.polymarket.com/developers/market-makers/trading)
