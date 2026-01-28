import { History, TrendingUp, TrendingDown, ShoppingCart, Clock, Activity } from 'lucide-react'
import type { AutoTradeLog } from '../../types'

interface Props {
  history: AutoTradeLog[]
}

function getActionIcon(action: string) {
  switch (action) {
    case 'auto_buy':
      return <ShoppingCart className="w-4 h-4 text-poly-green" />
    case 'take_profit':
      return <TrendingUp className="w-4 h-4 text-poly-green" />
    case 'stop_loss':
      return <TrendingDown className="w-4 h-4 text-poly-red" />
    case 'trailing_stop':
      return <Activity className="w-4 h-4 text-yellow-500" />
    case 'time_exit':
      return <Clock className="w-4 h-4 text-blue-400" />
    default:
      return <History className="w-4 h-4 text-gray-400" />
  }
}

function getActionLabel(action: string) {
  switch (action) {
    case 'auto_buy':
      return 'Auto-Buy'
    case 'take_profit':
      return 'Take Profit'
    case 'stop_loss':
      return 'Stop Loss'
    case 'trailing_stop':
      return 'Trailing Stop'
    case 'time_exit':
      return 'Time Exit'
    default:
      return action
  }
}

function formatTimeAgo(dateStr: string): string {
  const date = new Date(dateStr)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffMins = Math.floor(diffMs / 60000)
  const diffHours = Math.floor(diffMs / 3600000)
  const diffDays = Math.floor(diffMs / 86400000)

  if (diffMins < 1) return 'just now'
  if (diffMins < 60) return `${diffMins}m ago`
  if (diffHours < 24) return `${diffHours}h ago`
  return `${diffDays}d ago`
}

export function ActivityLog({ history }: Props) {
  if (history.length === 0) {
    return (
      <div className="bg-poly-card rounded-xl border border-poly-border p-6">
        <div className="flex items-center gap-2 mb-4">
          <History className="w-5 h-5 text-gray-400" />
          <h3 className="font-semibold">Activity Log</h3>
        </div>
        <p className="text-center text-gray-500 py-8">
          No auto-trades yet. Enable auto-trading to get started.
        </p>
      </div>
    )
  }

  return (
    <div className="bg-poly-card rounded-xl border border-poly-border p-4">
      <div className="flex items-center gap-2 mb-4">
        <History className="w-5 h-5 text-gray-400" />
        <h3 className="font-semibold">Activity Log</h3>
        <span className="text-sm text-gray-500">({history.length})</span>
      </div>

      <div className="space-y-2 max-h-96 overflow-y-auto">
        {history.map((log) => {
          const pnl = log.pnl ? parseFloat(log.pnl) : null
          const isProfit = pnl !== null && pnl > 0
          const isLoss = pnl !== null && pnl < 0

          return (
            <div
              key={log.id}
              className="flex items-start gap-3 p-3 bg-poly-dark/50 rounded-lg"
            >
              <div className="flex-shrink-0 mt-0.5">{getActionIcon(log.action)}</div>

              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="font-medium text-sm">{getActionLabel(log.action)}</span>
                  {log.side && (
                    <span
                      className={`text-xs px-1.5 py-0.5 rounded ${
                        log.side === 'Yes'
                          ? 'bg-poly-green/20 text-poly-green'
                          : 'bg-poly-red/20 text-poly-red'
                      }`}
                    >
                      {log.side}
                    </span>
                  )}
                  {pnl !== null && (
                    <span
                      className={`text-sm font-medium ${
                        isProfit ? 'text-poly-green' : isLoss ? 'text-poly-red' : 'text-gray-400'
                      }`}
                    >
                      {isProfit ? '+' : ''}${pnl.toFixed(2)}
                    </span>
                  )}
                </div>

                {log.market_question && (
                  <p className="text-sm text-gray-400 truncate mt-0.5">{log.market_question}</p>
                )}

                <div className="flex items-center gap-3 mt-1 text-xs text-gray-500">
                  {log.entry_price && (
                    <span>Entry: {(parseFloat(log.entry_price) * 100).toFixed(0)}¢</span>
                  )}
                  {log.exit_price && (
                    <span>Exit: {(parseFloat(log.exit_price) * 100).toFixed(0)}¢</span>
                  )}
                  {log.size && <span>${parseFloat(log.size).toFixed(2)}</span>}
                </div>
              </div>

              <div className="text-xs text-gray-500 flex-shrink-0">
                {formatTimeAgo(log.created_at)}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
