import { useState, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Crown, ChevronDown, ChevronUp, Eye, TrendingUp, TrendingDown, AlertTriangle, Pause, ExternalLink, Clock, Calendar } from 'lucide-react'
import { getMcStatus, getMcScoutLog, getMcTrades, getMcTierHistory } from '../../api/client'
import type { McStatus, McScoutResult, McTrade, McTierTransition } from '../../types'

const TIER_LABELS = [
  '', // 0 - unused
  '$40 / $5',
  '$100 / $12',
  '$300 / $35',
  '$1K / $100',
  '$3K / $250',
  '$7K / $500',
  '$10K / $750',
]

export function MillionairesClub() {
  const [collapsed, setCollapsed] = useState(false)
  const [showScoutDetails, setShowScoutDetails] = useState<string | null>(null)
  const [expandedTrade, setExpandedTrade] = useState<number | null>(null)
  const [showTierHistory, setShowTierHistory] = useState(false)
  const [realtimeStatus, setRealtimeStatus] = useState<McStatus | null>(null)

  // Listen for real-time WebSocket MC status updates
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail as McStatus
      setRealtimeStatus(detail)
    }
    window.addEventListener('mc-status', handler)
    return () => window.removeEventListener('mc-status', handler)
  }, [])

  // Poll status every 10 seconds (fallback for WS)
  const { data: statusData } = useQuery({
    queryKey: ['mc-status'],
    queryFn: getMcStatus,
    refetchInterval: 10000,
  })

  // Poll scout log every 30 seconds
  const { data: scoutData } = useQuery({
    queryKey: ['mc-scout-log'],
    queryFn: () => getMcScoutLog(20, 0),
    refetchInterval: 30000,
  })

  // Poll trades every 30 seconds
  const { data: tradesData } = useQuery({
    queryKey: ['mc-trades'],
    queryFn: () => getMcTrades(20, 0),
    refetchInterval: 30000,
  })

  // Tier history (infrequent)
  const { data: tierData } = useQuery({
    queryKey: ['mc-tier-history'],
    queryFn: getMcTierHistory,
    refetchInterval: 60000,
  })

  // Use real-time status if available, fallback to polled
  const status: McStatus | null = realtimeStatus || statusData?.status || null
  const scouts = scoutData?.logs || []
  const trades = tradesData?.trades || []
  const tierHistory = tierData?.history || []

  const isPaused = status?.pause_state !== 'active'
  const winRateColor = status && status.win_rate > 0
    ? status.win_rate >= 95 ? 'text-green-400' : status.win_rate >= 90 ? 'text-yellow-400' : 'text-red-400'
    : 'text-gray-400'

  return (
    <div className="bg-poly-card rounded-xl border border-poly-border">
      {/* Header */}
      <button
        onClick={() => setCollapsed(!collapsed)}
        className="w-full flex items-center justify-between p-4 hover:bg-white/5 transition rounded-xl"
      >
        <div className="flex items-center gap-3">
          <Crown className="w-5 h-5 text-yellow-500" />
          <h3 className="text-base font-semibold">Millionaires Club</h3>
          <span className="px-2 py-0.5 text-xs rounded-full bg-blue-500/20 text-blue-400 border border-blue-500/30">
            {status?.mode === 'live' ? 'LIVE' : 'OBSERVATION'}
          </span>
          {isPaused && (
            <span className="px-2 py-0.5 text-xs rounded-full bg-yellow-500/20 text-yellow-400 border border-yellow-500/30 flex items-center gap-1">
              <Pause className="w-3 h-3" /> PAUSED
            </span>
          )}
        </div>
        {collapsed ? <ChevronDown className="w-5 h-5 text-gray-400" /> : <ChevronUp className="w-5 h-5 text-gray-400" />}
      </button>

      {!collapsed && (
        <div className="px-4 pb-4 space-y-4">
          {/* Pause Banner */}
          {isPaused && status && (
            <div className={`px-3 py-2 rounded-lg text-sm flex items-center gap-2 ${
              status.pause_state === 'drawdown_paused' ? 'bg-red-500/10 text-red-400 border border-red-500/20' :
              'bg-yellow-500/10 text-yellow-400 border border-yellow-500/20'
            }`}>
              <AlertTriangle className="w-4 h-4" />
              <span>
                {status.pause_state === 'drawdown_paused' && 'Trading paused: 35%+ drawdown from peak'}
                {status.pause_state === 'drawdown_reduced' && 'Position size halved: 20%+ drawdown from peak'}
                {status.pause_state === 'weekly_loss_pause' && 'Trading paused: 2+ losses in 7 days'}
                {status.pause_state === 'dispute_pause' && 'Trading paused: active dispute detected'}
              </span>
              {status.pause_until && (
                <span className="ml-auto text-xs opacity-70">
                  Until {new Date(status.pause_until).toLocaleString()}
                </span>
              )}
            </div>
          )}

          {/* Dashboard Row */}
          {status && (
            <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-6 gap-3">
              <DashStat label="Tier" value={`${status.tier} — ${TIER_LABELS[status.tier] || '?'}`} />
              <DashStat label="Bankroll" value={`$${parseFloat(status.bankroll).toFixed(2)}`} />
              <DashStat label="Bet Size" value={`$${status.bet_size}`} />
              <DashStat
                label="Win Rate"
                value={`${status.win_rate.toFixed(1)}%`}
                className={winRateColor}
              />
              <DashStat
                label="P&L"
                value={`$${parseFloat(status.total_pnl).toFixed(2)}`}
                className={parseFloat(status.total_pnl) >= 0 ? 'text-green-400' : 'text-red-400'}
              />
              <DashStat
                label="Drawdown"
                value={`${status.drawdown_pct.toFixed(1)}%`}
                className={status.drawdown_pct > 20 ? 'text-red-400' : status.drawdown_pct > 10 ? 'text-yellow-400' : 'text-gray-300'}
              />
            </div>
          )}

          {/* Tier Progress Bar */}
          {status && (
            <div className="space-y-1">
              <div className="flex justify-between text-xs text-gray-500">
                <span>Tier Progress</span>
                <span>{status.tier}/7</span>
              </div>
              <div className="h-2 bg-gray-700 rounded-full overflow-hidden">
                <div
                  className="h-full bg-gradient-to-r from-yellow-600 to-yellow-400 rounded-full transition-all"
                  style={{ width: `${(status.tier / 7) * 100}%` }}
                />
              </div>
              <div className="flex justify-between text-xs text-gray-600">
                {[1,2,3,4,5,6,7].map(t => (
                  <span key={t} className={t <= status.tier ? 'text-yellow-500' : ''}>{t}</span>
                ))}
              </div>
            </div>
          )}

          {/* Scout Log */}
          <div>
            <h4 className="text-sm font-semibold text-gray-300 mb-2 flex items-center gap-2">
              <Eye className="w-4 h-4" /> Scout Log
              <span className="text-xs text-gray-500 font-normal">({scouts.length} recent)</span>
            </h4>
            {scouts.length === 0 ? (
              <p className="text-xs text-gray-500 italic">No markets evaluated yet. Waiting for scan...</p>
            ) : (
              <div className="space-y-1 max-h-64 overflow-y-auto">
                {scouts.map((scout, i) => (
                  <ScoutRow
                    key={`${scout.market_id}-${i}`}
                    scout={scout}
                    expanded={showScoutDetails === `${scout.market_id}-${i}`}
                    onToggle={() => setShowScoutDetails(
                      showScoutDetails === `${scout.market_id}-${i}` ? null : `${scout.market_id}-${i}`
                    )}
                  />
                ))}
              </div>
            )}
          </div>

          {/* Trade History */}
          <div>
            <h4 className="text-sm font-semibold text-gray-300 mb-2 flex items-center gap-2">
              <TrendingUp className="w-4 h-4" /> Simulated Trades
              <span className="text-xs text-gray-500 font-normal">({trades.length})</span>
            </h4>
            {trades.length === 0 ? (
              <p className="text-xs text-gray-500 italic">No simulated trades yet.</p>
            ) : (
              <div className="space-y-1 max-h-80 overflow-y-auto">
                {trades.map(trade => (
                  <TradeRow
                    key={trade.id}
                    trade={trade}
                    expanded={expandedTrade === trade.id}
                    onToggle={() => setExpandedTrade(expandedTrade === trade.id ? null : trade.id)}
                  />
                ))}
              </div>
            )}
          </div>

          {/* Tier History (collapsible) */}
          {tierHistory.length > 0 && (
            <div>
              <button
                onClick={() => setShowTierHistory(!showTierHistory)}
                className="text-sm font-semibold text-gray-300 mb-2 flex items-center gap-2 hover:text-white transition"
              >
                <TrendingDown className="w-4 h-4" /> Tier History
                <span className="text-xs text-gray-500 font-normal">({tierHistory.length})</span>
                {showTierHistory ? <ChevronUp className="w-3 h-3" /> : <ChevronDown className="w-3 h-3" />}
              </button>
              {showTierHistory && (
                <div className="space-y-1">
                  {tierHistory.map(th => (
                    <TierHistoryRow key={th.id} entry={th} />
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function DashStat({ label, value, className }: { label: string; value: string; className?: string }) {
  return (
    <div className="bg-gray-800/50 rounded-lg px-3 py-2">
      <p className="text-xs text-gray-500">{label}</p>
      <p className={`text-sm font-semibold ${className || 'text-white'}`}>{value}</p>
    </div>
  )
}

function ScoutRow({ scout, expanded, onToggle }: { scout: McScoutResult; expanded: boolean; onToggle: () => void }) {
  const priceDisplay = (parseFloat(scout.price) * 100).toFixed(1) + 'c'
  const truncQ = scout.question.length > 60 ? scout.question.slice(0, 57) + '...' : scout.question

  return (
    <div className="bg-gray-800/30 rounded-lg">
      <button
        onClick={onToggle}
        className="w-full flex items-center gap-2 px-3 py-1.5 text-xs hover:bg-white/5 transition rounded-lg"
      >
        <span className={`w-2 h-2 rounded-full flex-shrink-0 ${scout.passed ? 'bg-green-500' : 'bg-red-500'}`} />
        <span className="text-gray-300 text-left flex-1 truncate">{truncQ}</span>
        <span className="text-gray-400">{scout.side}</span>
        <span className="text-gray-300 font-mono">{priceDisplay}</span>
        <span className={`font-mono ${scout.certainty_score >= 60 ? 'text-green-400' : 'text-red-400'}`}>
          {scout.certainty_score}
        </span>
        {scout.slippage_pct !== null && (
          <span className="text-gray-500">{scout.slippage_pct.toFixed(2)}%</span>
        )}
        {scout.would_trade && (
          <span className="px-1.5 py-0.5 rounded bg-green-500/20 text-green-400 text-[10px]">TRADE</span>
        )}
      </button>
      {expanded && (
        <div className="px-3 pb-2 pt-1 border-t border-gray-700/50">
          <div className="space-y-0.5">
            {scout.reasons.map((reason, i) => (
              <p key={i} className={`text-[11px] font-mono ${
                reason.startsWith('+') ? 'text-green-400/80' :
                reason.startsWith('-FAIL') ? 'text-red-400/80' :
                reason.startsWith('-SKIP') ? 'text-yellow-400/80' :
                reason.startsWith('-WARN') ? 'text-orange-400/80' :
                reason.startsWith('-') ? 'text-red-400/80' :
                'text-gray-400'
              }`}>
                {reason}
              </p>
            ))}
          </div>
          <div className="mt-1 flex gap-3 text-[10px] text-gray-500">
            <span>Vol: ${parseFloat(scout.volume).toLocaleString()}</span>
            {scout.category && <span>Cat: {scout.category}</span>}
            <span>{new Date(scout.scanned_at).toLocaleTimeString()}</span>
          </div>
        </div>
      )}
    </div>
  )
}

function TradeRow({ trade, expanded, onToggle }: { trade: McTrade; expanded: boolean; onToggle: () => void }) {
  const truncQ = trade.question.length > 55 ? trade.question.slice(0, 52) + '...' : trade.question
  const pnl = trade.pnl ? parseFloat(trade.pnl) : null
  const entryDisplay = (parseFloat(trade.entry_price) * 100).toFixed(1) + 'c'
  const exitDisplay = trade.exit_price ? (parseFloat(trade.exit_price) * 100).toFixed(1) + 'c' : null
  const shares = parseFloat(trade.shares || '0')
  const size = parseFloat(trade.size || '0')

  // Calculate hold time
  const openedDate = new Date(trade.opened_at)
  const closedDate = trade.closed_at ? new Date(trade.closed_at) : null
  const holdMs = closedDate ? closedDate.getTime() - openedDate.getTime() : Date.now() - openedDate.getTime()
  const holdHours = holdMs / (1000 * 60 * 60)
  const holdDisplay = holdHours < 1 ? `${Math.round(holdHours * 60)}m` :
    holdHours < 24 ? `${holdHours.toFixed(1)}h` :
    holdHours < 168 ? `${(holdHours / 24).toFixed(1)}d` :
    `${(holdHours / 168).toFixed(1)}w`

  // Breakeven threshold for this entry price
  const entryPct = parseFloat(trade.entry_price) * 100
  const breakevenWinRate = entryPct // e.g., 95c entry = 95% breakeven

  // Return if won: (1 - entry) / entry * 100
  const entryF = parseFloat(trade.entry_price)
  const returnPct = entryF > 0 ? ((1 - entryF) / entryF * 100) : 0

  // Market end date
  const endDate = trade.end_date ? new Date(trade.end_date) : null
  const endDateDisplay = endDate ? endDate.toLocaleDateString() : null
  const timeUntilEnd = endDate ? endDate.getTime() - Date.now() : null
  const endingSoon = timeUntilEnd !== null && timeUntilEnd > 0 && timeUntilEnd < 3 * 24 * 60 * 60 * 1000 // < 3 days
  const endTimeDisplay = timeUntilEnd !== null && timeUntilEnd > 0
    ? timeUntilEnd < 60 * 60 * 1000 ? `${Math.round(timeUntilEnd / (60 * 1000))}m`
      : timeUntilEnd < 24 * 60 * 60 * 1000 ? `${(timeUntilEnd / (60 * 60 * 1000)).toFixed(1)}h`
      : `${(timeUntilEnd / (24 * 60 * 60 * 1000)).toFixed(1)}d`
    : timeUntilEnd !== null ? 'Ended' : null

  return (
    <div className="bg-gray-800/30 rounded-lg">
      <button
        onClick={onToggle}
        className="w-full flex items-center gap-2 px-3 py-1.5 text-xs hover:bg-white/5 transition rounded-lg"
      >
        <span className={`w-2 h-2 rounded-full flex-shrink-0 ${
          trade.status === 'open' ? 'bg-blue-500' :
          trade.status === 'won' ? 'bg-green-500' :
          'bg-red-500'
        }`} />
        <span className="text-gray-300 text-left flex-1 truncate">{truncQ}</span>
        <span className="text-gray-400">{trade.side}</span>
        <span className="text-gray-300 font-mono">{entryDisplay}</span>
        <span className={`px-1.5 py-0.5 rounded text-[10px] ${
          trade.status === 'open' ? 'bg-blue-500/20 text-blue-400' :
          trade.status === 'won' ? 'bg-green-500/20 text-green-400' :
          'bg-red-500/20 text-red-400'
        }`}>
          {trade.status.toUpperCase()}
        </span>
        <span className={`font-mono ${
          pnl === null ? 'text-gray-500' : pnl >= 0 ? 'text-green-400' : 'text-red-400'
        }`}>
          {pnl !== null ? `$${pnl.toFixed(2)}` : '—'}
        </span>
      </button>
      {expanded && (
        <div className="px-3 pb-3 pt-1 border-t border-gray-700/50 space-y-2">
          {/* Full question */}
          <p className="text-xs text-gray-300">{trade.question}</p>

          {/* Trade details grid */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
            <DetailCell label="Side" value={trade.side} />
            <DetailCell label="Entry Price" value={entryDisplay} />
            <DetailCell label="Exit Price" value={exitDisplay || 'Pending'} muted={!exitDisplay} />
            <DetailCell label="Bet Size" value={`$${size.toFixed(2)}`} />
            <DetailCell label="Shares" value={shares.toFixed(4)} />
            <DetailCell label="Tier at Entry" value={`Tier ${trade.tier_at_entry}`} />
            <DetailCell label="Certainty Score" value={`${trade.certainty_score}/100`}
              className={trade.certainty_score >= 80 ? 'text-green-400' : trade.certainty_score >= 60 ? 'text-yellow-400' : 'text-red-400'} />
            <DetailCell
              label="P&L"
              value={pnl !== null ? `$${pnl.toFixed(4)}` : 'Pending'}
              className={pnl === null ? undefined : pnl >= 0 ? 'text-green-400' : 'text-red-400'}
              muted={pnl === null}
            />
          </div>

          {/* Second row: timing and return info */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
            <DetailCell label="Return if Won" value={`${returnPct.toFixed(2)}%`} />
            <DetailCell label="Breakeven WR" value={`${breakevenWinRate.toFixed(1)}%`} />
            <DetailCell label={closedDate ? 'Hold Time' : 'Open For'} value={holdDisplay} icon={<Clock className="w-3 h-3 text-gray-500" />} />
            {endDateDisplay ? (
              <DetailCell
                label="Market Ends"
                value={endTimeDisplay ? `${endDateDisplay} (${endTimeDisplay})` : endDateDisplay}
                icon={<Calendar className="w-3 h-3 text-gray-500" />}
                className={endingSoon ? 'text-yellow-400' : timeUntilEnd !== null && timeUntilEnd <= 0 ? 'text-red-400' : undefined}
              />
            ) : trade.category ? (
              <DetailCell label="Category" value={trade.category} />
            ) : null}
          </div>
          {/* Category row (if end date takes the slot above) */}
          {endDateDisplay && trade.category && (
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
              <DetailCell label="Category" value={trade.category} />
            </div>
          )}

          {/* Timestamps */}
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-[10px] text-gray-500">
            <span>Opened: {openedDate.toLocaleString()}</span>
            {closedDate && <span>Closed: {closedDate.toLocaleString()}</span>}
            {trade.slug && (
              <a
                href={`https://polymarket.com/event/${trade.slug}`}
                target="_blank"
                rel="noopener noreferrer"
                className="text-blue-400 hover:text-blue-300 flex items-center gap-1"
              >
                View on Polymarket <ExternalLink className="w-3 h-3" />
              </a>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

function DetailCell({ label, value, className, muted, icon }: {
  label: string; value: string; className?: string; muted?: boolean; icon?: React.ReactNode
}) {
  return (
    <div className="bg-gray-900/40 rounded px-2 py-1">
      <p className="text-[10px] text-gray-500">{label}</p>
      <p className={`text-xs font-mono flex items-center gap-1 ${className || (muted ? 'text-gray-500' : 'text-gray-200')}`}>
        {icon}{value}
      </p>
    </div>
  )
}

function TierHistoryRow({ entry }: { entry: McTierTransition }) {
  const promoted = entry.to_tier > entry.from_tier

  return (
    <div className="flex items-center gap-2 text-xs px-3 py-1 bg-gray-800/30 rounded">
      <span className={promoted ? 'text-green-400' : 'text-red-400'}>
        {promoted ? '↑' : '↓'} Tier {entry.from_tier} → {entry.to_tier}
      </span>
      <span className="text-gray-500 flex-1">{entry.reason}</span>
      <span className="text-gray-600">{new Date(entry.timestamp).toLocaleDateString()}</span>
    </div>
  )
}
