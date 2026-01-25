import { Clock, TrendingUp, TrendingDown, FileText, Timer } from 'lucide-react'
import type { Position } from '../../types'

interface Props {
  position: Position
}

export function PositionCard({ position }: Props) {
  const entryPrice = parseFloat(position.entry_price)
  const size = parseFloat(position.size)
  const pnl = position.pnl ? parseFloat(position.pnl) : null
  const isProfitable = pnl !== null && pnl > 0

  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr)
    return date.toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  // Calculate time remaining until market ends
  const getTimeRemaining = () => {
    if (!position.end_date) return null
    const endDate = new Date(position.end_date)
    const now = new Date()
    const diffMs = endDate.getTime() - now.getTime()

    if (diffMs <= 0) return 'Ended'

    const hours = diffMs / (1000 * 60 * 60)
    if (hours < 1) return `${Math.round(hours * 60)}m`
    if (hours < 24) return `${hours.toFixed(1)}h`
    if (hours < 24 * 7) return `${(hours / 24).toFixed(1)}d`
    return `${(hours / (24 * 7)).toFixed(1)}w`
  }

  const timeRemaining = getTimeRemaining()

  const statusColors = {
    Open: 'bg-blue-500/20 text-blue-400',
    PendingResolution: 'bg-yellow-500/20 text-yellow-400',
    Resolved: pnl && pnl > 0 ? 'bg-poly-green/20 text-poly-green' : 'bg-poly-red/20 text-poly-red',
    Closed: 'bg-gray-500/20 text-gray-400',
  }

  return (
    <div className="bg-poly-card rounded-xl border border-poly-border p-4">
      <div className="flex items-start justify-between gap-3 mb-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1 flex-wrap">
            <span className={`text-xs font-medium px-2 py-0.5 rounded ${statusColors[position.status]}`}>
              {position.status}
            </span>
            <span className={`text-xs font-medium px-2 py-0.5 rounded ${
              position.strategy === 'ResolutionSniper'
                ? 'bg-yellow-500/20 text-yellow-400'
                : 'bg-blue-500/20 text-blue-400'
            }`}>
              {position.strategy === 'ResolutionSniper' ? 'Sniper' : 'NO Bias'}
            </span>
            {position.is_paper && (
              <span className="text-xs font-medium px-2 py-0.5 rounded bg-purple-500/20 text-purple-400 flex items-center gap-1">
                <FileText className="w-3 h-3" />
                Paper
              </span>
            )}
          </div>
          <h3 className="font-medium text-sm leading-tight line-clamp-2">
            {position.question}
          </h3>
        </div>
        {pnl !== null && (
          <div className={`flex items-center gap-1 ${isProfitable ? 'text-poly-green' : 'text-poly-red'}`}>
            {isProfitable ? (
              <TrendingUp className="w-4 h-4" />
            ) : (
              <TrendingDown className="w-4 h-4" />
            )}
            <span className="font-bold">
              {isProfitable ? '+' : ''}{pnl.toFixed(2)}
            </span>
          </div>
        )}
      </div>

      <div className="grid grid-cols-3 gap-3 text-center">
        <div>
          <div className={`text-lg font-bold ${
            position.side === 'Yes' ? 'text-poly-green' : 'text-poly-red'
          }`}>
            {position.side}
          </div>
          <div className="text-xs text-gray-500">Side</div>
        </div>
        <div>
          <div className="text-lg font-bold">{(entryPrice * 100).toFixed(0)}c</div>
          <div className="text-xs text-gray-500">Entry</div>
        </div>
        <div>
          <div className="text-lg font-bold">${size.toFixed(2)}</div>
          <div className="text-xs text-gray-500">Size</div>
        </div>
      </div>

      <div className="flex items-center justify-between text-xs text-gray-500 mt-3">
        <div className="flex items-center gap-1">
          <Clock className="w-3.5 h-3.5" />
          <span>Opened {formatDate(position.opened_at)}</span>
        </div>
        {timeRemaining && position.status === 'Open' && (
          <div className={`flex items-center gap-1 ${
            timeRemaining === 'Ended' ? 'text-poly-red' :
            timeRemaining.includes('h') && parseFloat(timeRemaining) < 4 ? 'text-yellow-400' :
            'text-gray-400'
          }`}>
            <Timer className="w-3.5 h-3.5" />
            <span>{timeRemaining === 'Ended' ? 'Market Ended' : `Ends in ${timeRemaining}`}</span>
          </div>
        )}
      </div>
    </div>
  )
}
