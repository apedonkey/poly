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
  recommendation: string
  token_id: string | null
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

export interface PaperTradeRequest {
  market_id: string
  side: 'Yes' | 'No'
  size_usdc: string
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

export type WsMessage = WsConnectedMessage | WsOpportunitiesMessage | WsErrorMessage
