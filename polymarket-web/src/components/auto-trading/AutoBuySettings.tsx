import { useState } from 'react'
import { ShoppingCart, ChevronDown, ChevronUp } from 'lucide-react'
import type { AutoTradingSettings } from '../../types'

interface Props {
  settings: AutoTradingSettings | undefined
  onUpdate: (settings: Partial<AutoTradingSettings>) => void
  disabled: boolean
  isPending: boolean
}

export function AutoBuySettings({ settings, onUpdate, disabled, isPending }: Props) {
  const [expanded, setExpanded] = useState(false)

  const handleToggle = (field: keyof AutoTradingSettings, value: boolean) => {
    onUpdate({ [field]: value })
  }

  const handleStrategyToggle = (strategy: string) => {
    const current = settings?.strategies || []
    const updated = current.includes(strategy)
      ? current.filter((s) => s !== strategy)
      : [...current, strategy]
    onUpdate({ strategies: updated })
  }

  return (
    <div
      className={`bg-poly-card rounded-xl border border-poly-border p-4 ${
        disabled ? 'opacity-50' : ''
      }`}
    >
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <ShoppingCart className="w-5 h-5 text-poly-green" />
          <h3 className="font-semibold">Auto-Buy</h3>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => handleToggle('auto_buy_enabled', !settings?.auto_buy_enabled)}
            disabled={disabled || isPending}
            className={`relative w-10 h-6 rounded-full transition-colors ${
              settings?.auto_buy_enabled ? 'bg-poly-green' : 'bg-gray-600'
            }`}
          >
            <span
              className={`absolute top-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
                settings?.auto_buy_enabled ? 'translate-x-4' : 'translate-x-0.5'
              }`}
            />
          </button>
        </div>
      </div>

      <p className="text-sm text-gray-400 mb-4">
        {settings?.auto_buy_enabled
          ? 'Automatically buying opportunities'
          : 'Auto-buy disabled'}
      </p>

      {/* Strategies */}
      <div className="mb-4">
        <label className="text-sm text-gray-400 mb-2 block">Strategies</label>
        <div className="flex gap-2">
          <button
            onClick={() => handleStrategyToggle('sniper')}
            disabled={disabled || isPending}
            className={`px-3 py-1.5 rounded-lg text-sm transition ${
              settings?.strategies?.includes('sniper')
                ? 'bg-poly-green/20 text-poly-green border border-poly-green'
                : 'bg-gray-700 text-gray-400 border border-gray-600'
            }`}
          >
            Sniper
          </button>
          <button
            onClick={() => handleStrategyToggle('no_bias')}
            disabled={disabled || isPending}
            className={`px-3 py-1.5 rounded-lg text-sm transition ${
              settings?.strategies?.includes('no_bias')
                ? 'bg-poly-green/20 text-poly-green border border-poly-green'
                : 'bg-gray-700 text-gray-400 border border-gray-600'
            }`}
          >
            NO Bias
          </button>
        </div>
      </div>

      {/* Expand/collapse advanced settings */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1 text-sm text-gray-400 hover:text-white transition"
      >
        {expanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        {expanded ? 'Hide' : 'Show'} advanced settings
      </button>

      {expanded && (
        <div className="mt-4 space-y-3 pt-4 border-t border-poly-border">
          {/* Max position size */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">Max per trade ($)</label>
            <input
              type="number"
              value={settings?.max_position_size || '50'}
              onChange={(e) => onUpdate({ max_position_size: e.target.value })}
              disabled={disabled || isPending}
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-base"
              min="1"
              step="1"
            />
          </div>

          {/* Max total exposure */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">Max total exposure ($)</label>
            <input
              type="number"
              value={settings?.max_total_exposure || '500'}
              onChange={(e) => onUpdate({ max_total_exposure: e.target.value })}
              disabled={disabled || isPending}
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-base"
              min="1"
              step="1"
            />
          </div>

          {/* Min edge */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">
              Min edge ({((settings?.min_edge || 0.05) * 100).toFixed(0)}%)
            </label>
            <input
              type="range"
              value={(settings?.min_edge || 0.05) * 100}
              onChange={(e) => onUpdate({ min_edge: parseFloat(e.target.value) / 100 })}
              disabled={disabled || isPending}
              className="w-full"
              min="1"
              max="30"
              step="1"
            />
          </div>

          {/* Max positions */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">Max positions</label>
            <input
              type="number"
              value={settings?.max_positions || 10}
              onChange={(e) => onUpdate({ max_positions: parseInt(e.target.value) })}
              disabled={disabled || isPending}
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-base"
              min="1"
              max="50"
            />
          </div>
        </div>
      )}
    </div>
  )
}
