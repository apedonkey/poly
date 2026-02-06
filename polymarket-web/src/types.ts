export interface MarketHolder {
  address: string
  amount: number
  name: string
  outcome_index: number
}

export interface MarketHolders {
  yes_holders: MarketHolder[]
  no_holders: MarketHolder[]
  yes_total_count: number
  no_total_count: number
}

// Opportunity from the backend
export interface Opportunity {
  market_id: string
  condition_id: string
  question: string
  slug: string
  strategy: 'ResolutionSniper' | 'Dispute'
  side: 'Yes' | 'No'
  entry_price: string
  expected_return: number
  confidence: number
  edge: number
  time_to_close_hours: number | null
  liquidity: string
  volume: string
  category: string | null
  resolution_source: string | null
  /** Full market description containing resolution rules */
  description: string | null
  recommendation: string
  token_id: string | null
  /** Whether this is a neg-risk market */
  neg_risk?: boolean
  /** Whether opportunity currently meets filter criteria. False = temporarily outside thresholds (price moved). */
  meets_criteria?: boolean
  /** Top holders on each side of the market */
  holders?: MarketHolders | null
}

// Wallet response
export interface GenerateWalletResponse {
  address: string
  private_key: string
  session_token: string
}

export interface ImportWalletResponse {
  address: string
  session_token: string
}

export interface UnlockWalletResponse {
  session_token: string
}

// Position
export interface Position {
  id: number
  market_id: string
  question: string
  slug: string | null
  side: 'Yes' | 'No'
  entry_price: string
  size: string
  strategy: 'ResolutionSniper' | 'Dispute'
  opened_at: string
  closed_at: string | null
  exit_price: string | null
  pnl: string | null
  status: 'Open' | 'PendingResolution' | 'Resolved' | 'Closed'
  is_paper: boolean
  end_date: string | null
  token_id: string | null
  order_id: string | null
  // Partial sell tracking fields
  remaining_size: string | null  // Shares remaining (null = full position for backward compat)
  realized_pnl: string | null    // Cumulative PnL from partial sells
  total_sold_size: string | null // Total shares sold so far
  avg_exit_price: string | null  // Weighted average exit price from partial sells
  neg_risk?: boolean             // Whether this is a neg-risk market
  fee_paid?: string              // Fee paid on this position
}

// Stats
export interface BotStats {
  total_trades: number
  winning_trades: number
  losing_trades: number
  total_pnl: string
  sniper_trades: number
  sniper_wins: number
  avg_hold_time_hours: number
}

// Trade request
export interface ExecuteTradeRequest {
  market_id: string
  side: 'Yes' | 'No'
  size_usdc: string
  password: string
  order_type?: 'market' | 'limit' | 'gtd' | 'fak'
  limit_price?: string  // Price in cents (e.g., "45" for 45c)
  take_profit_price?: string  // Sell limit price in cents
  post_only?: boolean  // Maker-only order (no taker fee, rejected if would cross spread)
  expiration?: number   // Unix timestamp for GTD order expiration
}

// WebSocket messages
export interface WsConnectedMessage {
  type: 'connected'
  data: { message: string }
}

export interface WsOpportunitiesMessage {
  type: 'opportunities'
  data: Opportunity[]
}

export interface WsErrorMessage {
  type: 'error'
  data: { message: string }
}

export interface WsPriceUpdateMessage {
  type: 'price_update'
  data: { token_id: string; price: string }
}

export interface WsScanStatusMessage {
  type: 'scan_status'
  data: { scan_interval_seconds: number; last_scan_at: number }
}

// UMA dispute alert
export interface DisputeAlert {
  assertion_id: string
  condition_id: string
  question: string
  slug: string
  dispute_status: 'Proposed' | 'Disputed' | 'DvmVote'
  proposed_outcome: string
  dispute_timestamp: number
  estimated_resolution: number
  current_yes_price: string
  current_no_price: string
  liquidity: string
  yes_token_id: string | null
  no_token_id: string | null
  edge: string | null
  /** Which round of the two-round dispute cycle (1 = first proposal, 2 = re-proposal after first dispute) */
  dispute_round?: number
  /** Proposer bond amount in USDC (typically $750) */
  proposer_bond?: string | null
  /** UmaCtfAdapter version ("v1", "v2", "v3") */
  adapter_version?: string | null
  /** Liveness/challenge window in seconds (typically 7200 = 2hr) */
  liveness_seconds?: number | null
  /** Expected value accounting for 50-50 outcome possibility */
  expected_value?: string | null
}

export interface WsDisputesMessage {
  type: 'disputes'
  data: DisputeAlert[]
}

export interface WsWalletBalanceMessage {
  type: 'wallet_balance'
  data: { address: string; usdc_balance: string }
}

// Order event from user channel WebSocket
export interface OrderEvent {
  order_id: string
  event_type: string
  status: string
  fill_price: string | null
  fill_size: string | null
  token_id: string | null
  timestamp: number
}

export interface WsOrderEventMessage {
  type: 'order_event'
  data: OrderEvent
}

// Millionaires Club status update
export interface McStatus {
  mode: string
  tier: number
  bankroll: string
  bet_size: string
  win_rate: number
  total_pnl: string
  total_trades: number
  open_trades: number
  drawdown_pct: number
  peak_bankroll: string
  pause_state: string
  pause_until: string | null
  recent_scouts: McScoutResult[]
  max_positions: number
}

export interface McScoutResult {
  market_id: string
  condition_id: string
  question: string
  slug: string
  side: string
  price: string
  volume: string
  category: string | null
  end_date: string | null
  passed: boolean
  certainty_score: number
  reasons: string[]
  slippage_pct: number | null
  would_trade: boolean
  token_id: string | null
  scanned_at: string
}

export interface McTrade {
  id: number
  market_id: string
  condition_id: string
  question: string
  slug: string
  side: string
  entry_price: string
  exit_price: string | null
  size: string
  shares: string
  pnl: string | null
  certainty_score: number
  category: string | null
  status: string
  tier_at_entry: number
  token_id: string | null
  end_date: string | null
  opened_at: string
  closed_at: string | null
}

export interface McTierTransition {
  id: number
  from_tier: number
  to_tier: number
  bankroll: string
  reason: string
  timestamp: string
}

export interface WsMcStatusMessage {
  type: 'mc_status'
  data: McStatus
}

// Mint Maker types
export interface MintMakerSettings {
  wallet_address: string
  enabled: boolean
  preset: string
  bid_offset_cents: number
  max_pair_cost: number
  min_spread_profit: number
  max_pairs_per_market: number
  max_total_pairs: number
  stale_order_seconds: number
  assets: string[]
  min_minutes_to_close: number
  max_minutes_to_close: number
  auto_place: boolean
  auto_place_size: string
  auto_max_markets: number
  auto_redeem: boolean
  stop_loss_pct: number
  stop_loss_delay_secs: number
  auto_place_delay_mins: number
  auto_size_pct: number
  auto_max_attempts: number
  balance_reserve: number
  smart_mode: boolean
  pre_place: boolean
  stop_after_profit: boolean
  momentum_threshold: number
  depth_check: boolean
}

export interface MintMakerMarketStatus {
  market_id: string
  condition_id: string
  question: string
  asset: string
  yes_token_id: string
  no_token_id: string
  yes_price: string
  no_price: string
  yes_bid: string | null
  no_bid: string | null
  spread_profit: string | null
  slug: string
  minutes_left: number
  open_pairs: number
}

export interface MintMakerPairSummary {
  id: number
  wallet_address: string
  market_id: string
  condition_id: string
  question: string
  asset: string
  yes_order_id: string
  no_order_id: string
  yes_bid_price: string
  no_bid_price: string
  yes_fill_price: string | null
  no_fill_price: string | null
  pair_cost: string | null
  profit: string | null
  size: string
  yes_size: string | null
  no_size: string | null
  slug: string | null
  status: string
  merge_tx_id: string | null
  stop_loss_order_id: string | null
  created_at: string
  updated_at: string
}

export interface MintMakerStatsSnapshot {
  total_pairs: number
  merged_pairs: number
  cancelled_pairs: number
  total_profit: string
  total_cost: string
  avg_spread: string
  fill_rate: string
}

export interface MintMakerLogEntry {
  id: number
  wallet_address: string
  action: string
  market_id: string | null
  question: string | null
  asset: string | null
  yes_price: string | null
  no_price: string | null
  pair_cost: string | null
  profit: string | null
  size: string | null
  details: string | null
  created_at: string
}

export interface MintMakerStatus {
  enabled: boolean
  active_markets: MintMakerMarketStatus[]
  stats: MintMakerStatsSnapshot
  open_pairs: MintMakerPairSummary[]
  recent_log: MintMakerLogEntry[]
  settings: MintMakerSettings | null
}

export interface WsMintMakerStatusMessage {
  type: 'mint_maker_status'
  data: MintMakerStatus
}

export type WsMessage = WsConnectedMessage | WsOpportunitiesMessage | WsErrorMessage | WsPriceUpdateMessage | WsScanStatusMessage | WsDisputesMessage | WsWalletBalanceMessage | WsOrderEventMessage | WsMcStatusMessage | WsMintMakerStatusMessage

// Auto-Trading Settings
export interface AutoTradingSettings {
  enabled: boolean
  auto_buy_enabled: boolean
  strategies: string[]
  position_size: string
  max_total_exposure: string
  min_edge: number
  max_positions: number
  take_profit_enabled: boolean
  take_profit_percent: number
  stop_loss_enabled: boolean
  stop_loss_percent: number
  trailing_stop_enabled: boolean
  trailing_stop_percent: number
  time_exit_enabled: boolean
  time_exit_hours: number
  dispute_sniper_enabled: boolean
  min_dispute_edge: number
  dispute_position_size: string
  dispute_exit_on_escalation: boolean
}

// Auto-Trade Log Entry
export interface AutoTradeLog {
  id: number
  wallet_address: string
  position_id: number | null
  action: string
  side: string | null
  entry_price: string | null
  exit_price: string | null
  size: string | null
  pnl: string | null
  market_question: string | null
  created_at: string
}

// Auto-Trading Stats
export interface AutoTradingStats {
  total_trades: number
  total_pnl: string
  win_rate: number
  take_profit_count: number
  take_profit_pnl: string
  stop_loss_count: number
  stop_loss_pnl: string
  trailing_stop_count: number
  trailing_stop_pnl: string
  time_exit_count: number
  time_exit_pnl: string
  auto_buy_count: number
}
