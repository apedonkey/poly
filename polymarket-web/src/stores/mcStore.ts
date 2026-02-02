import { create } from 'zustand'

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

interface McState {
  status: McStatus | null
  setStatus: (status: McStatus) => void
}

export const useMcStore = create<McState>((set) => ({
  status: null,
  setStatus: (status) => set({ status }),
}))
