import { useState } from 'react'
import { Crosshair, ChevronDown, ChevronUp } from 'lucide-react'
import type { AutoTradingSettings } from '../../types'

interface Props {
  settings: AutoTradingSettings | undefined
  onUpdate: (settings: Partial<AutoTradingSettings>) => void
  disabled: boolean
  isPending: boolean
}

export function DisputeSniperSettings({ settings, onUpdate, disabled, isPending }: Props) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div
      className={`bg-poly-card rounded-xl border border-poly-border p-4 ${
        disabled ? 'opacity-50' : ''
      }`}
    >
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <Crosshair className="w-5 h-5 text-yellow-500" />
          <h3 className="font-semibold">Dispute Sniper</h3>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => onUpdate({ dispute_sniper_enabled: !settings?.dispute_sniper_enabled })}
            disabled={disabled || isPending}
            className={`relative w-11 h-6 rounded-full transition-colors ${
              settings?.dispute_sniper_enabled ? 'bg-yellow-500' : 'bg-gray-600'
            }`}
          >
            <span
              className={`absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
                settings?.dispute_sniper_enabled ? 'translate-x-5' : 'translate-x-0'
              }`}
            />
          </button>
        </div>
      </div>

      <p className="text-sm text-gray-400 mb-4">
        {settings?.dispute_sniper_enabled
          ? 'Auto-buying disputed market outcomes with edge'
          : 'Dispute sniper disabled'}
      </p>

      <p className="text-xs text-gray-500 mb-4">
        When a UMA dispute is proposed, automatically buys the proposed outcome side if the
        edge exceeds your threshold. Auto-exits if the dispute escalates.
      </p>

      {/* Expand/collapse advanced settings */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1 text-sm text-gray-400 hover:text-white transition"
      >
        {expanded ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
        {expanded ? 'Hide' : 'Show'} settings
      </button>

      {expanded && (
        <div className="mt-4 space-y-3 pt-4 border-t border-poly-border">
          {/* Min dispute edge */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">
              Min edge ({((settings?.min_dispute_edge || 0.10) * 100).toFixed(0)}%)
            </label>
            <input
              type="range"
              value={(settings?.min_dispute_edge || 0.10) * 100}
              onChange={(e) => onUpdate({ min_dispute_edge: parseFloat(e.target.value) / 100 })}
              disabled={disabled || isPending}
              className="w-full"
              min="1"
              max="30"
              step="1"
            />
          </div>

          {/* Max position size */}
          <div>
            <label className="text-sm text-gray-400 block mb-1">Snipe size ($)</label>
            <input
              type="number"
              value={settings?.dispute_position_size || '25'}
              onChange={(e) => onUpdate({ dispute_position_size: e.target.value })}
              disabled={disabled || isPending}
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-3 py-2 text-base"
              min="1"
              step="1"
            />
          </div>

          {/* Auto-exit on escalation */}
          <div className="flex items-center justify-between">
            <label className="text-sm text-gray-400">Auto-exit on escalation</label>
            <button
              onClick={() => onUpdate({ dispute_exit_on_escalation: !settings?.dispute_exit_on_escalation })}
              disabled={disabled || isPending}
              className={`relative w-11 h-6 rounded-full transition-colors ${
                settings?.dispute_exit_on_escalation !== false ? 'bg-poly-green' : 'bg-gray-600'
              }`}
            >
              <span
                className={`absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
                  settings?.dispute_exit_on_escalation !== false ? 'translate-x-5' : 'translate-x-0'
                }`}
              />
            </button>
          </div>
          <p className="text-xs text-gray-500">
            Automatically sells if a dispute escalates from Proposed to Disputed or DVM Vote.
          </p>
        </div>
      )}
    </div>
  )
}
