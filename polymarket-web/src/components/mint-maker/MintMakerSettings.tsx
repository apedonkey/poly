import { useState } from 'react'
import { Settings, ChevronDown, ChevronUp } from 'lucide-react'
import { updateMintMakerSettings } from '../../api/client'
import { useWalletStore } from '../../stores/walletStore'
import type { MintMakerSettings as MintMakerSettingsType } from '../../types'

interface Props {
  settings: MintMakerSettingsType
  onUpdate: () => void
}

const PRESETS = [
  {
    name: 'conservative',
    label: 'Conservative',
    desc: 'BTC only, wider spreads, lower risk',
  },
  {
    name: 'balanced',
    label: 'Balanced',
    desc: 'BTC/ETH/SOL, moderate spreads',
  },
  {
    name: 'aggressive',
    label: 'Aggressive',
    desc: 'All assets, tight spreads, higher volume',
  },
]

export function MintMakerSettingsPanel({ settings, onUpdate }: Props) {
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [saving, setSaving] = useState(false)
  const sessionToken = useWalletStore((s) => s.sessionToken)

  const applyPreset = async (preset: string) => {
    if (!sessionToken) return
    setSaving(true)
    try {
      await updateMintMakerSettings(sessionToken, { preset })
      onUpdate()
    } catch (e) {
      console.error('Failed to update preset:', e)
    }
    setSaving(false)
  }

  const updateField = async (field: string, value: unknown) => {
    if (!sessionToken) return
    try {
      await updateMintMakerSettings(sessionToken, { [field]: value })
      onUpdate()
    } catch (e) {
      console.error('Failed to update setting:', e)
    }
  }

  return (
    <div className="space-y-4">
      {/* Preset Buttons */}
      <div className="flex gap-2">
        {PRESETS.map((preset) => (
          <button
            key={preset.name}
            onClick={() => applyPreset(preset.name)}
            disabled={saving}
            className={`flex-1 px-3 py-2 rounded-lg border transition text-sm ${
              settings.preset === preset.name
                ? 'border-poly-green bg-poly-green/10 text-poly-green'
                : 'border-poly-border text-gray-400 hover:border-gray-500'
            }`}
          >
            <div className="font-medium">{preset.label}</div>
            <div className="text-xs opacity-70 mt-0.5">{preset.desc}</div>
          </button>
        ))}
      </div>

      {/* Auto Place */}
      <div className="p-3 rounded-lg bg-poly-dark/50 border border-poly-border space-y-2">
        <div className="flex items-center gap-3">
          <button
            onClick={() => updateField('auto_place', !settings.auto_place)}
            className={`relative w-11 h-6 rounded-full transition-colors ${
              settings.auto_place ? 'bg-poly-green' : 'bg-gray-600'
            }`}
          >
            <span
              className={`absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
                settings.auto_place ? 'translate-x-5' : ''
              }`}
            />
          </button>
          <div className="flex-1">
            <div className="text-sm font-medium">Auto Place</div>
            <div className="text-xs text-gray-500">Places pairs when new markets open</div>
          </div>
          <div className="flex items-center gap-2">
            <div className="flex items-center gap-1">
              <span className="text-xs text-gray-500">$</span>
              <input
                type="number"
                value={settings.auto_place_size}
                onChange={(e) => updateField('auto_place_size', e.target.value || '2')}
                className="w-16 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm text-center"
                min="3"
              />
              <span className="text-xs text-gray-500">per side</span>
            </div>
            <div className="flex items-center gap-1">
              <input
                type="number"
                value={settings.auto_max_markets}
                onChange={(e) => updateField('auto_max_markets', parseInt(e.target.value) || 1)}
                className="w-12 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm text-center"
                min="1"
                max="10"
              />
              <span className="text-xs text-gray-500">markets</span>
            </div>
          </div>
        </div>
        <div className="text-xs text-gray-600 leading-relaxed">
          Min $3/side (CLOB requires 5+ shares). Total cost per pair = 2 x size.
          At $5/side: ~10 shares each, total $10 risked, ~$0.02-0.04 profit if merged.
          25% stop loss on half-filled positions.
        </div>
      </div>

      {/* Auto Redeem */}
      <div className="p-3 rounded-lg bg-poly-dark/50 border border-poly-border">
        <div className="flex items-center gap-3">
          <button
            onClick={() => updateField('auto_redeem', !settings.auto_redeem)}
            className={`relative w-11 h-6 rounded-full transition-colors ${
              settings.auto_redeem ? 'bg-poly-green' : 'bg-gray-600'
            }`}
          >
            <span
              className={`absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full transition-transform ${
                settings.auto_redeem ? 'translate-x-5' : ''
              }`}
            />
          </button>
          <div className="flex-1">
            <div className="text-sm font-medium">Auto-Redeem</div>
            <div className="text-xs text-gray-500">Automatically claim resolved positions</div>
          </div>
        </div>
      </div>

      {/* Asset Selection */}
      <div className="p-3 rounded-lg bg-poly-dark/50 border border-poly-border">
        <div className="text-sm font-medium mb-2">Markets</div>
        <div className="flex gap-2">
          {['BTC', 'ETH', 'SOL', 'XRP'].map((asset) => {
            const isActive = settings.assets.includes(asset)
            return (
              <button
                key={asset}
                onClick={() => {
                  const next = isActive
                    ? settings.assets.filter((a) => a !== asset)
                    : [...settings.assets, asset]
                  if (next.length > 0) updateField('assets', next)
                }}
                className={`px-3 py-1.5 rounded-lg text-sm font-medium transition border ${
                  isActive
                    ? 'border-poly-green bg-poly-green/10 text-poly-green'
                    : 'border-poly-border text-gray-500 hover:border-gray-400'
                }`}
              >
                {asset}
              </button>
            )
          })}
        </div>
      </div>

      {/* Advanced Settings Toggle */}
      <button
        onClick={() => setShowAdvanced(!showAdvanced)}
        className="flex items-center gap-2 text-sm text-gray-400 hover:text-white transition"
      >
        <Settings className="w-4 h-4" />
        Advanced Settings
        {showAdvanced ? <ChevronUp className="w-4 h-4" /> : <ChevronDown className="w-4 h-4" />}
      </button>

      {showAdvanced && (
        <div className="grid grid-cols-2 gap-3 p-3 rounded-lg bg-poly-dark/50 border border-poly-border">
          <div>
            <label className="text-xs text-gray-500">Bid Offset (cents)</label>
            <input
              type="number"
              value={settings.bid_offset_cents}
              onChange={(e) => updateField('bid_offset_cents', parseInt(e.target.value) || 2)}
              className="w-full mt-1 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm"
            />
          </div>
          <div>
            <label className="text-xs text-gray-500">Max Pair Cost</label>
            <input
              type="number"
              step="0.01"
              value={settings.max_pair_cost}
              onChange={(e) => updateField('max_pair_cost', parseFloat(e.target.value) || 0.98)}
              className="w-full mt-1 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm"
            />
          </div>
          <div>
            <label className="text-xs text-gray-500">Min Spread Profit</label>
            <input
              type="number"
              step="0.005"
              value={settings.min_spread_profit}
              onChange={(e) => updateField('min_spread_profit', parseFloat(e.target.value) || 0.01)}
              className="w-full mt-1 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm"
            />
          </div>
          <div>
            <label className="text-xs text-gray-500">Max Pairs/Market</label>
            <input
              type="number"
              value={settings.max_pairs_per_market}
              onChange={(e) => updateField('max_pairs_per_market', parseInt(e.target.value) || 5)}
              className="w-full mt-1 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm"
            />
          </div>
          <div>
            <label className="text-xs text-gray-500">Max Total Pairs</label>
            <input
              type="number"
              value={settings.max_total_pairs}
              onChange={(e) => updateField('max_total_pairs', parseInt(e.target.value) || 20)}
              className="w-full mt-1 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm"
            />
          </div>
          <div>
            <label className="text-xs text-gray-500">Stale Timeout (sec)</label>
            <input
              type="number"
              value={settings.stale_order_seconds}
              onChange={(e) => updateField('stale_order_seconds', parseInt(e.target.value) || 120)}
              className="w-full mt-1 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm"
            />
          </div>
        </div>
      )}
    </div>
  )
}
