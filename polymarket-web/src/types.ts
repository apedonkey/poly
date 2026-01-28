// Opportunity from the backend
export interface Opportunity {
  market_id: string
  condition_id: string
  question: string
  slug: string
  strategy: 'ResolutionSniper' | 'NoBias'
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
  /** Whether opportunity currently meets filter criteria. False = temporarily outside thresholds (price moved). */
  meets_criteria?: boolean
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
  strategy: 'ResolutionSniper' | 'NoBias'
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
}

// Stats
export interface BotStats {
  total_trades: number
  winning_trades: number
  losing_trades: number
  total_pnl: string
  sniper_trades: number
  sniper_wins: number
  no_bias_trades: number
  no_bias_wins: number
  avg_hold_time_hours: number
}

// Trade request
export interface ExecuteTradeRequest {
  market_id: string
  side: 'Yes' | 'No'
  size_usdc: string
  password: string
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

// Clarification alert when market description changes
export interface ClarificationAlert {
  market_id: string
  condition_id: string
  question: string
  slug: string
  old_description_hash: string
  new_description_preview: string
  detected_at: number
  current_yes_price: string
  current_no_price: string
  liquidity: string
}

// UMA dispute alert
export interface DisputeAlert {
  market_id: string
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
}

export interface WsClarificationsMessage {
  type: 'clarifications'
  data: ClarificationAlert[]
}

export interface WsDisputesMessage {
  type: 'disputes'
  data: DisputeAlert[]
}

export type WsMessage = WsConnectedMessage | WsOpportunitiesMessage | WsErrorMessage | WsPriceUpdateMessage | WsScanStatusMessage | WsClarificationsMessage | WsDisputesMessage

// Auto-Trading Settings
export interface AutoTradingSettings {
  enabled: boolean
  auto_buy_enabled: boolean
  strategies: string[]
  max_position_size: string
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
