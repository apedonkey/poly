import { useState, useEffect, useRef } from 'react'
import { updateMintMakerSettings } from '../../api/client'
import { useWalletStore } from '../../stores/walletStore'
import type { MintMakerSettings as MintMakerSettingsType, MintMakerMarketStatus } from '../../types'

interface Props {
  settings: MintMakerSettingsType
  onUpdate: () => void
  activeMarkets: MintMakerMarketStatus[]
}

/** A number input that uses local state so you can freely type/delete,
 *  and only saves to the server onBlur. */
function NumField({ value, field, parse, className, ...props }: {
  value: number | string
  field: string
  parse: (v: string) => unknown
  className?: string
  [key: string]: unknown
}) {
  const [local, setLocal] = useState(String(value))
  const [focused, setFocused] = useState(false)
  const savingRef = useRef(false)
  const sessionToken = useWalletStore((s) => s.sessionToken)
  const { onUpdate } = props as { onUpdate: () => void }

  // Sync local state when server value changes, but not while focused or saving
  useEffect(() => {
    if (!focused && !savingRef.current) {
      setLocal(String(value))
    }
  }, [value, focused])

  const save = async () => {
    setFocused(false)
    if (!sessionToken) return
    const parsed = parse(local)
    if (parsed === null || parsed === undefined) {
      setLocal(String(value)) // revert empty/invalid
      return
    }
    savingRef.current = true
    try {
      await updateMintMakerSettings(sessionToken, { [field]: parsed })
      onUpdate()
    } catch (e) {
      console.error('Failed to update setting:', e)
      setLocal(String(value))
    }
    savingRef.current = false
  }

  return (
    <input
      type="number"
      value={local}
      onChange={(e) => setLocal(e.target.value)}
      onFocus={() => setFocused(true)}
      onBlur={save}
      onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur() }}
      className={`px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm text-center ${className || 'w-16'}`}
      {...(props.min !== undefined ? { min: props.min as number } : {})}
      {...(props.max !== undefined ? { max: props.max as number } : {})}
      {...(props.step !== undefined ? { step: props.step as number } : {})}
    />
  )
}

export function MintMakerSettingsPanel({ settings, onUpdate, activeMarkets }: Props) {
  const sessionToken = useWalletStore((s) => s.sessionToken)
  const balance = useWalletStore((s) => s.balance)

  const updateField = async (field: string, value: unknown) => {
    if (!sessionToken) return
    try {
      await updateMintMakerSettings(sessionToken, { [field]: value })
      onUpdate()
    } catch (e) {
      console.error('Failed to update setting:', e)
    }
  }

  const parseInt0 = (v: string) => { const n = parseInt(v); return isNaN(n) ? null : n }
  const parseFloat0 = (v: string) => { const n = parseFloat(v); return isNaN(n) ? null : n }
  const parseStr = (v: string) => v || null

  return (
    <div className="space-y-4">
      {/* Auto Place */}
      <div className="p-3 rounded-lg bg-poly-dark/50 border border-poly-border space-y-3">
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
          <div>
            <div className="text-sm font-medium">Auto Place</div>
            <div className="text-xs text-gray-500">Places YES+NO pairs when new markets open</div>
          </div>
          {settings.auto_place && (
            <div className="ml-auto flex rounded overflow-hidden border border-poly-border">
              <button
                onClick={() => { if (settings.smart_mode) updateField('smart_mode', false) }}
                className={`px-2 py-0.5 text-xs font-medium transition-colors ${
                  !settings.smart_mode ? 'bg-poly-green text-white' : 'bg-poly-dark text-gray-500 hover:text-gray-300'
                }`}
              >Manual</button>
              <button
                onClick={() => { if (!settings.smart_mode) updateField('smart_mode', true) }}
                className={`px-2 py-0.5 text-xs font-medium transition-colors ${
                  settings.smart_mode ? 'bg-poly-green text-white' : 'bg-poly-dark text-gray-500 hover:text-gray-300'
                }`}
              >Smart</button>
            </div>
          )}
        </div>

        {/* Live balance */}
        {(() => {
          const bal = balance?.safe_usdc_balance ? parseFloat(balance.safe_usdc_balance) : 0
          const reserveAmt = settings.balance_reserve || 0
          const usable = Math.max(0, bal - reserveAmt)
          return (
            <div className="flex items-center justify-between text-xs bg-poly-dark/80 rounded px-2 py-1.5 border border-poly-border/50">
              <span className="text-gray-400">Safe balance</span>
              <div className="flex items-center gap-2">
                {reserveAmt > 0 && (
                  <span className="text-gray-500">${usable.toFixed(2)} usable</span>
                )}
                <span className="text-white font-medium">${bal.toFixed(2)}</span>
              </div>
            </div>
          )
        })()}

        {/* Settings rows */}
        <div className="space-y-3 text-xs">
          <div className="text-gray-500 font-medium">How much to spend</div>
          <div className="flex items-center justify-between gap-4">
            <div>
              <div className="text-gray-400">Size per side</div>
              <div className="text-gray-600">{!settings.auto_size_pct ? 'Fixed $ amount for each YES and NO order' : '% of usable balance, split across markets'}</div>
            </div>
            <div className="flex items-center gap-1 shrink-0">
              <div className="flex rounded overflow-hidden border border-poly-border">
                <button
                  onClick={() => { if (settings.auto_size_pct > 0) updateField('auto_size_pct', 0) }}
                  className={`px-1.5 py-0.5 text-xs font-medium transition-colors ${
                    !settings.auto_size_pct ? 'bg-poly-green text-white' : 'bg-poly-dark text-gray-500 hover:text-gray-300'
                  }`}
                >$</button>
                <button
                  onClick={() => { if (!settings.auto_size_pct) updateField('auto_size_pct', 100) }}
                  className={`px-1.5 py-0.5 text-xs font-medium transition-colors ${
                    settings.auto_size_pct > 0 ? 'bg-poly-green text-white' : 'bg-poly-dark text-gray-500 hover:text-gray-300'
                  }`}
                >%</button>
              </div>
              {!settings.auto_size_pct ? (
                <NumField value={settings.auto_place_size} field="auto_place_size" parse={parseStr} onUpdate={onUpdate} min={1} />
              ) : (
                <NumField value={settings.auto_size_pct} field="auto_size_pct" parse={parseInt0} onUpdate={onUpdate} min={1} max={100} />
              )}
            </div>
          </div>
          <div className="flex items-center justify-between gap-4">
            <div>
              <div className="text-gray-400">Keep reserve</div>
              <div className="text-gray-600">Never spend below this amount — stays untouched in Safe</div>
            </div>
            <div className="flex items-center gap-1 shrink-0">
              <span className="text-gray-500">$</span>
              <NumField value={settings.balance_reserve} field="balance_reserve" parse={parseFloat0} onUpdate={onUpdate} min={0} step={1} />
            </div>
          </div>

          {!settings.smart_mode && (
            <div className="text-gray-500 font-medium pt-1">Bid pricing</div>
          )}
          {!settings.smart_mode && (
            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-gray-400">Bid below cheap side</div>
                <div className="text-gray-600">How many cents under the cheaper side's price to bid</div>
              </div>
              <div className="flex items-center gap-1 shrink-0">
                <NumField value={settings.bid_offset_cents} field="bid_offset_cents" parse={parseInt0} onUpdate={onUpdate} min={0} />
                <span className="text-gray-500">¢</span>
              </div>
            </div>
          )}
          {!settings.smart_mode && (
            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-gray-400">Max pair cost</div>
                <div className="text-gray-600">Most you'll pay for YES + NO combined (profit = 1 - cost)</div>
              </div>
              <NumField value={settings.max_pair_cost} field="max_pair_cost" parse={parseFloat0} onUpdate={onUpdate} step={0.01} className="shrink-0 w-16" />
            </div>
          )}
          <div className="flex items-center justify-between gap-4">
            <div>
              <div className="text-gray-400">Min profit per pair</div>
              <div className="text-gray-600">Skip if profit per share is below this amount</div>
            </div>
            <NumField value={settings.min_spread_profit} field="min_spread_profit" parse={parseFloat0} onUpdate={onUpdate} step={0.005} className="shrink-0 w-16" />
          </div>

          {!settings.smart_mode && (
            <div className="text-gray-500 font-medium pt-1">Limits</div>
          )}
          {!settings.smart_mode && (
            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-gray-400">Open pairs per market</div>
                <div className="text-gray-600">Max pending/active pairs on a single market at once</div>
              </div>
              <NumField value={settings.max_pairs_per_market} field="max_pairs_per_market" parse={parseInt0} onUpdate={onUpdate} min={1} className="shrink-0 w-16" />
            </div>
          )}
          {!settings.smart_mode && (
            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-gray-400">Total open pairs</div>
                <div className="text-gray-600">Hard cap across all markets — stops placing when hit</div>
              </div>
              <NumField value={settings.max_total_pairs} field="max_total_pairs" parse={parseInt0} onUpdate={onUpdate} min={1} className="shrink-0 w-16" />
            </div>
          )}
          <div className="flex items-center justify-between gap-4">
            <div>
              <div className="text-gray-400">Buys per market</div>
              <div className="text-gray-600">Lifetime cap — won't re-enter a market past this count</div>
            </div>
            <NumField value={settings.auto_max_attempts} field="auto_max_attempts" parse={parseInt0} onUpdate={onUpdate} min={1} max={20} className="shrink-0 w-16" />
          </div>
            <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-gray-400">Wait before buying</div>
                <div className="text-gray-600">Minutes to wait after a market opens before placing</div>
              </div>
              <div className="flex items-center gap-1 shrink-0">
                <NumField value={settings.auto_place_delay_mins} field="auto_place_delay_mins" parse={parseInt0} onUpdate={onUpdate} min={0} max={14} />
                <span className="text-gray-500">min</span>
              </div>
            </div>
          <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-gray-400">Pre-place next window</div>
                <div className="text-gray-600">Queue orders on the upcoming market for early fills</div>
              </div>
              <button
                onClick={() => updateField('pre_place', !settings.pre_place)}
                className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
                  settings.pre_place ? 'bg-poly-green text-white' : 'bg-poly-dark text-gray-500 border border-poly-border hover:text-gray-300'
                }`}
              >{settings.pre_place ? 'On' : 'Off'}</button>
            </div>
          <div className="flex items-center justify-between gap-4">
              <div>
                <div className="text-gray-400">Stop after profit</div>
                <div className="text-gray-600">1 pair at a time — stop market on merge, retry on orphan</div>
              </div>
              <button
                onClick={() => updateField('stop_after_profit', !settings.stop_after_profit)}
                className={`px-3 py-1 rounded text-xs font-medium transition-colors ${
                  settings.stop_after_profit ? 'bg-poly-green text-white' : 'bg-poly-dark text-gray-500 border border-poly-border hover:text-gray-300'
                }`}
              >{settings.stop_after_profit ? 'On' : 'Off'}</button>
            </div>
          {settings.smart_mode && (
            <div className="text-xs text-gray-500 italic pt-1">
              Offset, pair cost, and limits auto-calculated each cycle
            </div>
          )}

          <div className="text-xs text-green-600/80 italic pt-2">
            Momentum filter active: only enters 60/40+ markets with orderbook depth confirmation
          </div>
        </div>
        {(() => {
          if (settings.auto_size_pct <= 0) return null
          const bal = balance?.safe_usdc_balance ? parseFloat(balance.safe_usdc_balance) : 0
          const reserveAmt = settings.balance_reserve || 0
          const usable = Math.max(0, bal - reserveAmt)
          const pct = settings.auto_size_pct
          const capital = usable * pct / 100
          const numMarkets = settings.assets.length || 1
          const perMarket = capital / numMarkets
          const perSide = perMarket / 2

          // Mirror runner's cheap-side bidding logic for each active market
          const offset = settings.bid_offset_cents / 100
          const maxCost = settings.max_pair_cost
          const selectedMarkets = activeMarkets
            .filter((m) => settings.assets.includes(m.asset))
            .sort((a, b) => settings.assets.indexOf(a.asset) - settings.assets.indexOf(b.asset))
          const previews = selectedMarkets.map((m) => {
            const yp = parseFloat(m.yes_price)
            const np = parseFloat(m.no_price)
            const cheapPrice = Math.min(yp, np)
            const cheapBid = cheapPrice - offset
            const expBid = maxCost - cheapBid
            const yesBid = yp <= np ? cheapBid : expBid
            const noBid = yp <= np ? expBid : cheapBid
            const maxPrice = Math.max(yesBid, noBid)
            const shares = maxPrice > 0 ? Math.floor(perSide / maxPrice) : 0
            const pairCost = (yesBid + noBid)
            const profitPerShare = 1 - pairCost
            return { asset: m.asset, yesBid, noBid, shares, pairCost, profitPerShare, totalProfit: profitPerShare * shares }
          })

          return (
            <div className="text-xs space-y-1 bg-poly-dark/80 rounded p-2 border border-poly-border/50">
              <div className="flex justify-between text-gray-400">
                <span>Safe balance</span>
                <span className="text-white font-medium">${bal.toFixed(2)}</span>
              </div>
              {reserveAmt > 0 && (
                <div className="flex justify-between text-gray-400">
                  <span>Reserve (untouched)</span>
                  <span className="text-yellow-500">-${reserveAmt.toFixed(2)}</span>
                </div>
              )}
              <div className="flex justify-between text-gray-400">
                <span>{reserveAmt > 0 ? `Usable → ${pct}%` : `Capital (${pct}%)`}</span>
                <span>${capital.toFixed(2)}</span>
              </div>
              <div className="flex justify-between text-gray-400">
                <span>Per market ({numMarkets} market{numMarkets !== 1 ? 's' : ''})</span>
                <span>${perMarket.toFixed(2)}</span>
              </div>
              <div className="flex justify-between text-gray-400 border-t border-poly-border/50 pt-1">
                <span>Per side (YES + NO)</span>
                <span className="text-poly-green font-medium">${perSide.toFixed(2)}/side</span>
              </div>
              {previews.length > 0 && (
                <>
                  <div className="border-t border-poly-border/50 pt-1 mt-1 text-gray-500 font-medium">Buy preview (current markets)</div>
                  {previews.map((p) => (
                    <div key={p.asset} className="flex justify-between text-gray-400">
                      <span>{p.asset}: {p.shares} shares @ Y{(p.yesBid * 100).toFixed(0)}¢+N{(p.noBid * 100).toFixed(0)}¢</span>
                      <span className={p.shares >= 5 ? 'text-poly-green' : 'text-red-400'}>
                        {p.shares >= 5 ? `+$${p.totalProfit.toFixed(2)} profit` : 'below min 5'}
                      </span>
                    </div>
                  ))}
                </>
              )}
              {previews.length === 0 && (
                <div className="text-gray-500 italic border-t border-poly-border/50 pt-1 mt-1">No active markets to preview</div>
              )}
            </div>
          )
        })()}
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
                onClick={async () => {
                  const next = isActive
                    ? settings.assets.filter((a) => a !== asset)
                    : [...settings.assets, asset]
                  if (next.length > 0) {
                    await updateField('assets', next)
                    await updateField('auto_max_markets', next.length)
                  }
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

    </div>
  )
}
