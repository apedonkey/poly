import { BarChart3, TrendingUp, TrendingDown } from 'lucide-react'
import type { AutoTradingStats } from '../../types'

interface Props {
  stats: AutoTradingStats | undefined
}

export function AutoTradingStatsCard({ stats }: Props) {
  const totalPnl = parseFloat(stats?.total_pnl || '0')
  const winRate = (stats?.win_rate || 0) * 100

  return (
    <div className="bg-poly-card rounded-xl border border-poly-border p-4">
      <div className="flex items-center gap-2 mb-4">
        <BarChart3 className="w-5 h-5 text-poly-green" />
        <h3 className="font-semibold">Performance</h3>
      </div>

      {/* Main stats */}
      <div className="grid grid-cols-2 gap-4 mb-4">
        <div>
          <p className="text-sm text-gray-400">Total Trades</p>
          <p className="text-xl font-bold">{stats?.total_trades || 0}</p>
        </div>
        <div>
          <p className="text-sm text-gray-400">Win Rate</p>
          <p className="text-xl font-bold">{winRate.toFixed(0)}%</p>
        </div>
      </div>

      {/* P&L */}
      <div className="mb-4">
        <p className="text-sm text-gray-400">Total P&L</p>
        <p
          className={`text-2xl font-bold ${
            totalPnl >= 0 ? 'text-poly-green' : 'text-poly-red'
          }`}
        >
          {totalPnl >= 0 ? '+' : ''}${totalPnl.toFixed(2)}
        </p>
      </div>

      {/* Breakdown by trigger type */}
      <div className="space-y-2 pt-4 border-t border-poly-border">
        <div className="flex items-center justify-between text-sm">
          <div className="flex items-center gap-2">
            <TrendingUp className="w-3 h-3 text-poly-green" />
            <span className="text-gray-400">Take Profit</span>
          </div>
          <span>
            {stats?.take_profit_count || 0} (${parseFloat(stats?.take_profit_pnl || '0').toFixed(2)})
          </span>
        </div>

        <div className="flex items-center justify-between text-sm">
          <div className="flex items-center gap-2">
            <TrendingDown className="w-3 h-3 text-poly-red" />
            <span className="text-gray-400">Stop Loss</span>
          </div>
          <span>
            {stats?.stop_loss_count || 0} (${parseFloat(stats?.stop_loss_pnl || '0').toFixed(2)})
          </span>
        </div>

        <div className="flex items-center justify-between text-sm">
          <div className="flex items-center gap-2">
            <span className="w-3 h-3 text-yellow-500">~</span>
            <span className="text-gray-400">Trailing Stop</span>
          </div>
          <span>
            {stats?.trailing_stop_count || 0} ($
            {parseFloat(stats?.trailing_stop_pnl || '0').toFixed(2)})
          </span>
        </div>

        <div className="flex items-center justify-between text-sm">
          <div className="flex items-center gap-2">
            <span className="w-3 h-3 text-blue-400">T</span>
            <span className="text-gray-400">Time Exit</span>
          </div>
          <span>
            {stats?.time_exit_count || 0} (${parseFloat(stats?.time_exit_pnl || '0').toFixed(2)})
          </span>
        </div>

        <div className="flex items-center justify-between text-sm">
          <div className="flex items-center gap-2">
            <span className="w-3 h-3 text-poly-green">+</span>
            <span className="text-gray-400">Auto-Buy</span>
          </div>
          <span>{stats?.auto_buy_count || 0}</span>
        </div>
      </div>
    </div>
  )
}
