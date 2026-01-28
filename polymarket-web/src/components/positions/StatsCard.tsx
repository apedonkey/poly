import { memo } from 'react'
import { TrendingUp, Target, Crosshair, Clock } from 'lucide-react'
import type { BotStats } from '../../types'

interface Props {
  stats: BotStats
}

export const StatsCard = memo(function StatsCard({ stats }: Props) {
  const totalPnl = parseFloat(stats.total_pnl)
  const winRate = stats.total_trades > 0
    ? (stats.winning_trades / stats.total_trades * 100).toFixed(1)
    : '0.0'
  const sniperWinRate = stats.sniper_trades > 0
    ? (stats.sniper_wins / stats.sniper_trades * 100).toFixed(1)
    : '0.0'
  const noBiasWinRate = stats.no_bias_trades > 0
    ? (stats.no_bias_wins / stats.no_bias_trades * 100).toFixed(1)
    : '0.0'

  return (
    <div className="bg-poly-card rounded-xl border border-poly-border p-3 sm:p-4">
      <h2 className="text-base sm:text-lg font-semibold mb-3 sm:mb-4 flex items-center gap-2">
        <TrendingUp className="w-4 h-4 sm:w-5 sm:h-5 text-poly-green" />
        Performance
      </h2>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-2 sm:gap-4">
        <div className="text-center p-2.5 sm:p-3 bg-poly-dark rounded-lg">
          <div className={`text-xl sm:text-2xl font-bold ${totalPnl >= 0 ? 'text-poly-green' : 'text-poly-red'}`}>
            {totalPnl >= 0 ? '+' : ''}{totalPnl.toFixed(2)}
          </div>
          <div className="text-xs text-gray-500">P&L</div>
        </div>

        <div className="text-center p-2.5 sm:p-3 bg-poly-dark rounded-lg">
          <div className="text-xl sm:text-2xl font-bold">{winRate}%</div>
          <div className="text-xs text-gray-500">Win Rate</div>
          <div className="text-xs text-gray-600">{stats.winning_trades}/{stats.total_trades}</div>
        </div>

        <div className="text-center p-2.5 sm:p-3 bg-poly-dark rounded-lg">
          <div className="flex items-center justify-center gap-1 mb-0.5 sm:mb-1">
            <Crosshair className="w-3.5 h-3.5 sm:w-4 sm:h-4 text-yellow-400" />
            <span className="text-xl sm:text-2xl font-bold">{sniperWinRate}%</span>
          </div>
          <div className="text-xs text-gray-500">Sniper</div>
          <div className="text-xs text-gray-600">{stats.sniper_wins}/{stats.sniper_trades}</div>
        </div>

        <div className="text-center p-2.5 sm:p-3 bg-poly-dark rounded-lg">
          <div className="flex items-center justify-center gap-1 mb-0.5 sm:mb-1">
            <Target className="w-3.5 h-3.5 sm:w-4 sm:h-4 text-blue-400" />
            <span className="text-xl sm:text-2xl font-bold">{noBiasWinRate}%</span>
          </div>
          <div className="text-xs text-gray-500">NO Bias</div>
          <div className="text-xs text-gray-600">{stats.no_bias_wins}/{stats.no_bias_trades}</div>
        </div>
      </div>

      {stats.avg_hold_time_hours > 0 && (
        <div className="flex items-center justify-center gap-1.5 sm:gap-2 mt-3 sm:mt-4 text-xs sm:text-sm text-gray-400">
          <Clock className="w-3.5 h-3.5 sm:w-4 sm:h-4" />
          <span>Avg hold: {stats.avg_hold_time_hours.toFixed(1)}h</span>
        </div>
      )}
    </div>
  )
})
