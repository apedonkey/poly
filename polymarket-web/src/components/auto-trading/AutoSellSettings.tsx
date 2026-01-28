import { useState } from 'react'
import { TrendingUp, TrendingDown, Activity, Clock, ChevronDown, ChevronUp } from 'lucide-react'
import type { AutoTradingSettings } from '../../types'

interface Props {
  settings: AutoTradingSettings | undefined
  onUpdate: (settings: Partial<AutoTradingSettings>) => void
  disabled: boolean
  isPending: boolean
}

export function AutoSellSettings({ settings, onUpdate, disabled, isPending }: Props) {
  const [expanded, setExpanded] = useState(false)

  const handleToggle = (field: keyof AutoTradingSettings, value: boolean) => {
    onUpdate({ [field]: value })
  }

  return (
    <div
      className={`bg-poly-card rounded-xl border border-poly-border p-4 ${
        disabled ? 'opacity-50' : ''
      }`}
    >
      <div className="flex items-center gap-2 mb-4">
        <TrendingUp className="w-5 h-5 text-poly-green" />
        <h3 className="font-semibold">Auto-Sell</h3>
      </div>

      {/* Take Profit */}
      <div className="flex items-center justify-between py-2 border-b border-poly-border">
        <div className="flex items-center gap-2">
          <TrendingUp className="w-4 h-4 text-poly-green" />
          <span className="text-sm">Take Profit</span>
          <span className="text-xs text-gray-400">
            ({((settings?.take_profit_percent || 0.2) * 100).toFixed(0)}%)
          </span>
        </div>
        <button
          onClick={() => handleToggle('take_profit_enabled', !settings?.take_profit_enabled)}
          disabled={disabled || isPending}
          className={`relative w-10 h-6 rounded-full transition-colors ${
            settings?.take_profit_enabled ? 'bg-poly-green' : 'bg-gray-600'
          }`}
        >
          <span
            className={`absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
              settings?.take_profit_enabled ? 'translate-x-4' : 'translate-x-0.5'
            }`}
          />
        </button>
      </div>

      {/* Stop Loss */}
      <div className="flex items-center justify-between py-2 border-b border-poly-border">
        <div className="flex items-center gap-2">
          <TrendingDown className="w-4 h-4 text-poly-red" />
          <span className="text-sm">Stop Loss</span>
          <span className="text-xs text-gray-400">
            ({((settings?.stop_loss_percent || 0.1) * 100).toFixed(0)}%)
          </span>
        </div>
        <button
          onClick={() => handleToggle('stop_loss_enabled', !settings?.stop_loss_enabled)}
          disabled={disabled || isPending}
          className={`relative w-10 h-6 rounded-full transition-colors ${
            settings?.stop_loss_enabled ? 'bg-poly-green' : 'bg-gray-600'
          }`}
        >
          <span
            className={`absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
              settings?.stop_loss_enabled ? 'translate-x-4' : 'translate-x-0.5'
            }`}
          />
        </button>
      </div>

      {/* Trailing Stop */}
      <div className="flex items-center justify-between py-2 border-b border-poly-border">
        <div className="flex items-center gap-2">
          <Activity className="w-4 h-4 text-yellow-500" />
          <span className="text-sm">Trailing Stop</span>
          <span className="text-xs text-gray-400">
            ({((settings?.trailing_stop_percent || 0.1) * 100).toFixed(0)}%)
          </span>
        </div>
        <button
          onClick={() => handleToggle('trailing_stop_enabled', !settings?.trailing_stop_enabled)}
          disabled={disabled || isPending}
          className={`relative w-10 h-6 rounded-full transition-colors ${
            settings?.trailing_stop_enabled ? 'bg-poly-green' : 'bg-gray-600'
          }`}
        >
          <span
            className={`absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
              settings?.trailing_stop_enabled ? 'translate-x-4' : 'translate-x-0.5'
            }`}
          />
        </button>
      </div>

      {/* Time Exit */}
      <div className="flex items-center justify-between py-2">
        <div className="flex items-center gap-2">
          <Clock className="w-4 h-4 text-blue-400" />
          <span className="text-sm">Time Exit</span>
          <span className="text-xs text-gray-400">({settings?.time_exit_hours || 24}h)</span>
        </div>
        <button
          onClick={() => handleToggle('time_exit_enabled', !settings?.time_exit_enabled)}
          disabled={disabled || isPending}
          className={`relative w-10 h-6 rounded-full transition-colors ${
            settings?.time_exit_enabled ? 'bg-poly-green' : 'bg-gray-600'
          }`}
        >
          <span
            className={`absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
              settings?.time_exit_enabled ? 'translate-x-4' : 'translate-x-0.5'
            }`}
          />
        </button>
      </div>

      {/* Expand/collapse advanced settings */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1 text-sm text-gray-400 hover:text-white transition mt-3"
      >
        {expanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        {expanded ? 'Hide' : 'Adjust'} percentages
      </button>

      {expanded && (
        <div className="mt-4 space-y-3 pt-4 border-t border-poly-border">
          {/* Take profit percent */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">
              Take Profit ({((settings?.take_profit_percent || 0.2) * 100).toFixed(0)}%)
            </label>
            <input
              type="range"
              value={(settings?.take_profit_percent || 0.2) * 100}
              onChange={(e) => onUpdate({ take_profit_percent: parseFloat(e.target.value) / 100 })}
              disabled={disabled || isPending}
              className="w-full"
              min="5"
              max="100"
              step="5"
            />
          </div>

          {/* Stop loss percent */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">
              Stop Loss ({((settings?.stop_loss_percent || 0.1) * 100).toFixed(0)}%)
            </label>
            <input
              type="range"
              value={(settings?.stop_loss_percent || 0.1) * 100}
              onChange={(e) => onUpdate({ stop_loss_percent: parseFloat(e.target.value) / 100 })}
              disabled={disabled || isPending}
              className="w-full"
              min="5"
              max="50"
              step="5"
            />
          </div>

          {/* Trailing stop percent */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">
              Trailing Stop ({((settings?.trailing_stop_percent || 0.1) * 100).toFixed(0)}%)
            </label>
            <input
              type="range"
              value={(settings?.trailing_stop_percent || 0.1) * 100}
              onChange={(e) => onUpdate({ trailing_stop_percent: parseFloat(e.target.value) / 100 })}
              disabled={disabled || isPending}
              className="w-full"
              min="5"
              max="30"
              step="5"
            />
          </div>

          {/* Time exit hours */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">
              Time Exit ({settings?.time_exit_hours || 24} hours)
            </label>
            <input
              type="range"
              value={settings?.time_exit_hours || 24}
              onChange={(e) => onUpdate({ time_exit_hours: parseFloat(e.target.value) })}
              disabled={disabled || isPending}
              className="w-full"
              min="1"
              max="72"
              step="1"
            />
          </div>
        </div>
      )}
    </div>
  )
}
