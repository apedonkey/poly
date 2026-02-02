import { useState, useCallback, useEffect, useMemo, useRef } from 'react'
import { Coins, Power, PowerOff, X, ArrowUpDown, Clock, Loader2, ExternalLink, AlertTriangle, Filter } from 'lucide-react'
import { useMintMakerStore } from '../../stores/mintMakerStore'
import { useWalletStore } from '../../stores/walletStore'
import { enableMintMaker, disableMintMaker, placeMintMakerPair, cancelMintMakerPair, getMintMakerSettings, getMintMakerPairs } from '../../api/client'
import { MintMakerSettingsPanel } from './MintMakerSettings'
import type { MintMakerMarketStatus, MintMakerSettings, MintMakerPairSummary } from '../../types'

function polymarketUrl(slug: string | null, conditionId: string) {
  return `https://polymarket.com/event/${slug || conditionId}`
}

function timeAgo(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime()
  const mins = Math.floor(diff / 60000)
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins}m ago`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ago`
  const days = Math.floor(hrs / 24)
  return `${days}d ago`
}

export function MintMakerPanel() {
  const status = useMintMakerStore((s) => s.status)
  const sessionToken = useWalletStore((s) => s.sessionToken)
  const [password, setPassword] = useState('')
  const [enabling, setEnabling] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [localSettings, setLocalSettings] = useState<MintMakerSettings | null>(null)
  const [allPairs, setAllPairs] = useState<MintMakerPairSummary[]>([])
  const [pairFilter, setPairFilter] = useState<string>('all')

  // Real-time price cache: token_id -> price string (from price_update WS events)
  const livePricesRef = useRef<Map<string, string>>(new Map())
  const [livePricesTick, setLivePricesTick] = useState(0)

  // Listen for real-time price_update DOM events (dispatched by useWebSocket)
  useEffect(() => {
    const handler = (e: Event) => {
      const { token_id, price } = (e as CustomEvent).detail
      livePricesRef.current.set(token_id, price)
    }
    window.addEventListener('price_update', handler)

    // Flush live prices to trigger re-render every 500ms
    const iv = setInterval(() => {
      if (livePricesRef.current.size > 0) {
        setLivePricesTick(t => t + 1)
      }
    }, 500)

    return () => {
      window.removeEventListener('price_update', handler)
      clearInterval(iv)
    }
  }, [])

  // Overlay real-time prices onto active markets
  const activeMarkets = useMemo(() => {
    const markets = status?.active_markets || []
    void livePricesTick // used as dependency to trigger recalc
    const prices = livePricesRef.current
    if (prices.size === 0) return markets

    return markets.map(m => {
      const yesLive = prices.get(m.yes_token_id)
      if (!yesLive) return m
      const yesPrice = parseFloat(yesLive)
      if (isNaN(yesPrice) || yesPrice <= 0) return m
      return {
        ...m,
        yes_price: yesPrice.toString(),
        no_price: (1 - yesPrice).toFixed(4),
      }
    })
  }, [status?.active_markets, livePricesTick])

  const settings = localSettings || status?.settings || null

  const refreshSettings = useCallback(async () => {
    if (!sessionToken) return
    try {
      const { settings: s } = await getMintMakerSettings(sessionToken)
      setLocalSettings(s)
    } catch (_) { /* ignore */ }
  }, [sessionToken])

  // Prefer localSettings (from API) over WS status — WS can lag after enable/disable
  const isEnabled = localSettings !== null ? localSettings.enabled : (status?.enabled ?? false)

  // Track previous open pair IDs to detect transitions (pair left open_pairs → merged/cancelled)
  const prevOpenIdsRef = useRef<Set<number>>(new Set())
  const fetchingRef = useRef(false)

  const refreshPairs = useCallback(() => {
    if (!sessionToken || fetchingRef.current) return
    fetchingRef.current = true
    getMintMakerPairs(sessionToken)
      .then(res => setAllPairs(res.pairs))
      .catch(() => {})
      .finally(() => { fetchingRef.current = false })
  }, [sessionToken])

  // Fetch historical pairs on mount
  useEffect(() => {
    if (sessionToken && isEnabled) refreshPairs()
  }, [sessionToken, isEnabled, refreshPairs])

  // Re-fetch whenever WS open_pairs changes (real-time: picks up merges, cancels, new pairs)
  const openPairs = status?.open_pairs
  useEffect(() => {
    if (!sessionToken || !isEnabled) return
    const currentIds = new Set((openPairs || []).map(p => p.id))
    const prevIds = prevOpenIdsRef.current

    // Detect if any pair left the open set (= status transition) or new pair appeared
    const changed = currentIds.size !== prevIds.size ||
      [...prevIds].some(id => !currentIds.has(id)) ||
      [...currentIds].some(id => !prevIds.has(id))

    if (changed) {
      prevOpenIdsRef.current = currentIds
      refreshPairs()
    }
  }, [openPairs, sessionToken, isEnabled, refreshPairs])

  // Merge WS real-time open_pairs with REST historical pairs
  // WS open_pairs are always the freshest for active statuses; REST fills in merged/cancelled history
  const mergedPairs = useMemo(() => {
    const openMap = new Map((status?.open_pairs || []).map(p => [p.id, p]))
    // For IDs that exist in both, prefer the WS version (fresher status)
    const result = new Map<number, MintMakerPairSummary>()
    for (const p of allPairs) {
      result.set(p.id, openMap.get(p.id) || p)
    }
    // Add any WS open pairs not yet in allPairs (brand new pairs before REST catches up)
    for (const p of (status?.open_pairs || [])) {
      if (!result.has(p.id)) result.set(p.id, p)
    }
    return [...result.values()]
      .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
  }, [status?.open_pairs, allPairs])

  // Filter pairs
  const filteredPairs = useMemo(() => {
    if (pairFilter === 'all') return mergedPairs
    if (pairFilter === 'open') return mergedPairs.filter(p => ['Pending', 'HalfFilled', 'Orphaned'].includes(p.status))
    if (pairFilter === 'matched') return mergedPairs.filter(p => ['Matched', 'Merging'].includes(p.status))
    if (pairFilter === 'merged') return mergedPairs.filter(p => p.status === 'Merged')
    if (pairFilter === 'cancelled') return mergedPairs.filter(p => p.status === 'Cancelled' || p.status === 'StopLoss')
    return mergedPairs
  }, [mergedPairs, pairFilter])

  // Count for filter badges
  const openCount = mergedPairs.filter(p => ['Pending', 'HalfFilled', 'Orphaned'].includes(p.status)).length
  const matchedCount = mergedPairs.filter(p => ['Matched', 'Merging'].includes(p.status)).length

  const handleEnable = async () => {
    if (!sessionToken || !password) return
    setEnabling(true)
    setError(null)
    try {
      await enableMintMaker(sessionToken, password)
      setPassword('')
      setLocalSettings((prev) => prev ? { ...prev, enabled: true } : prev)
      refreshSettings()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to enable')
    }
    setEnabling(false)
  }

  const handleDisable = async () => {
    if (!sessionToken) return
    try {
      await disableMintMaker(sessionToken)
      setLocalSettings((prev) => prev ? { ...prev, enabled: false } : prev)
      refreshSettings()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to disable')
    }
  }

  if (!sessionToken) {
    return (
      <div className="text-center py-12 text-gray-500">
        <Coins className="w-12 h-12 mx-auto mb-4 opacity-50" />
        <p>Connect a wallet to use Mint Maker</p>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="bg-poly-card rounded-xl p-4 border border-poly-border">
        <div className="flex items-center justify-between mb-4">
          <div className="flex items-center gap-3">
            <Coins className="w-6 h-6 text-yellow-400" />
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-lg font-bold">Mint Maker</h2>
                {settings?.auto_place && (
                  <span className="text-xs font-bold bg-poly-green/20 text-poly-green px-1.5 py-0.5 rounded">
                    AUTO
                  </span>
                )}
              </div>
              <p className="text-xs text-gray-500">Market-make 15-min crypto Up/Down markets</p>
            </div>
          </div>
          {isEnabled ? (
            <button
              onClick={handleDisable}
              className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-red-500/20 text-red-400 hover:bg-red-500/30 transition text-sm"
            >
              <PowerOff className="w-4 h-4" />
              Disable
            </button>
          ) : (
            <div className="flex items-center gap-2">
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Password"
                className="px-2 py-1.5 rounded-lg bg-poly-dark border border-poly-border text-sm w-32"
                onKeyDown={(e) => e.key === 'Enter' && handleEnable()}
              />
              <button
                onClick={handleEnable}
                disabled={enabling || !password}
                className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-poly-green/20 text-poly-green hover:bg-poly-green/30 transition text-sm disabled:opacity-50"
              >
                {enabling ? <Loader2 className="w-4 h-4 animate-spin" /> : <Power className="w-4 h-4" />}
                Enable
              </button>
            </div>
          )}
        </div>

        {error && (
          <div className="text-sm text-red-400 bg-red-500/10 p-2 rounded mb-4">{error}</div>
        )}

        {/* Settings */}
        {settings && (
          <MintMakerSettingsPanel settings={settings} onUpdate={refreshSettings} />
        )}
      </div>

      {/* Stats */}
      {status?.stats && (
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
          <StatCard label="Total Pairs" value={String(status.stats.total_pairs)} />
          <StatCard label="Merged" value={String(status.stats.merged_pairs)} color="text-poly-green" />
          <StatCard label="Total Profit" value={`$${status.stats.total_profit}`} color="text-poly-green" />
          <StatCard label="Fill Rate" value={`${status.stats.fill_rate}%`} />
        </div>
      )}

      {/* Active Markets */}
      {activeMarkets.length > 0 && (
        <div className="bg-poly-card rounded-xl p-4 border border-poly-border">
          <h3 className="text-sm font-semibold mb-3 text-gray-400">Active Markets</h3>
          <div className="space-y-3">
            {activeMarkets.map((market) => (
              <MarketCard key={market.market_id} market={market} sessionToken={sessionToken} />
            ))}
          </div>
        </div>
      )}

      {activeMarkets.length === 0 && isEnabled && (
        <div className="text-center py-8 text-gray-500">
          <Clock className="w-8 h-8 mx-auto mb-2 opacity-50" />
          <p className="text-sm">No eligible 15-min crypto markets found right now</p>
          <p className="text-xs mt-1">Markets appear when 2-14 minutes from close</p>
        </div>
      )}

      {/* Position Manager */}
      {mergedPairs.length > 0 && (
        <div className="bg-poly-card rounded-xl p-4 border border-poly-border">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-semibold text-gray-400 flex items-center gap-2">
              <Filter className="w-3.5 h-3.5" />
              Positions ({filteredPairs.length})
            </h3>
          </div>

          {/* Filter bar */}
          <div className="flex gap-1 mb-3 flex-wrap">
            {[
              { key: 'all', label: 'All' },
              { key: 'open', label: `Open${openCount > 0 ? ` (${openCount})` : ''}` },
              { key: 'matched', label: `Matched${matchedCount > 0 ? ` (${matchedCount})` : ''}` },
              { key: 'merged', label: 'Merged' },
              { key: 'cancelled', label: 'Cancelled' },
            ].map(f => (
              <button
                key={f.key}
                onClick={() => setPairFilter(f.key)}
                className={`text-xs px-2 py-1 rounded transition ${
                  pairFilter === f.key
                    ? 'bg-poly-green/20 text-poly-green'
                    : 'text-gray-500 hover:text-gray-300 hover:bg-poly-border/30'
                }`}
              >
                {f.label}
              </button>
            ))}
          </div>

          {/* Pair cards */}
          <div className="space-y-2 max-h-[500px] overflow-y-auto">
            {filteredPairs.map((pair) => (
              <PairCard key={pair.id} pair={pair} sessionToken={sessionToken} />
            ))}
            {filteredPairs.length === 0 && (
              <p className="text-xs text-gray-600 text-center py-4">No pairs match this filter</p>
            )}
          </div>
        </div>
      )}

      {/* Activity Log */}
      {status?.recent_log && status.recent_log.length > 0 && (
        <div className="bg-poly-card rounded-xl p-4 border border-poly-border">
          <h3 className="text-sm font-semibold mb-3 text-gray-400">Activity Log</h3>
          <div className="space-y-1 max-h-60 overflow-y-auto">
            {status.recent_log.map((entry) => (
              <div key={entry.id} className="text-xs flex items-center gap-2 py-1 border-b border-poly-border/50">
                <span className={`font-mono ${
                  entry.action === 'merge' ? 'text-poly-green' :
                  entry.action === 'auto_place' ? 'text-blue-400' :
                  entry.action === 'cancel_stale' ? 'text-yellow-400' :
                  entry.action === 'cancel_pair' ? 'text-red-400' :
                  entry.action === 'orphaned' ? 'text-orange-400' :
                  'text-gray-400'
                }`}>
                  {entry.action}
                </span>
                <span className="text-gray-500 truncate flex-1">{entry.question || entry.details || ''}</span>
                {entry.profit && <span className="text-poly-green">${entry.profit}</span>}
                <span className="text-gray-600">{new Date(entry.created_at).toLocaleTimeString()}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

function StatCard({ label, value, color }: { label: string; value: string; color?: string }) {
  return (
    <div className="bg-poly-card rounded-lg p-3 border border-poly-border">
      <div className="text-xs text-gray-500">{label}</div>
      <div className={`text-lg font-bold ${color || 'text-white'}`}>{value}</div>
    </div>
  )
}

function MarketCard({ market, sessionToken }: { market: MintMakerMarketStatus; sessionToken: string }) {
  const [size, setSize] = useState('2')
  const [pw, setPw] = useState('')
  const [placing, setPlacing] = useState(false)
  const [result, setResult] = useState<string | null>(null)

  const handlePlace = async () => {
    if (!pw || !size) return
    setPlacing(true)
    setResult(null)
    try {
      const res = await placeMintMakerPair(sessionToken, {
        market_id: market.market_id,
        condition_id: market.condition_id,
        question: market.question,
        asset: market.asset,
        yes_token_id: market.yes_token_id,
        no_token_id: market.no_token_id,
        yes_price: market.yes_bid || market.yes_price,
        no_price: market.no_bid || market.no_price,
        size,
        password: pw,
        slug: market.slug,
      })
      if (res.success) {
        const yShares = res.yes_shares ? ` Y:${res.yes_shares}sh` : ''
        const nShares = res.no_shares ? ` N:${res.no_shares}sh` : ''
        setResult(`Pair placed!${yShares}${nShares} Cost: $${res.pair_cost}, Profit: $${res.expected_profit}`)
        setPw('')
      }
    } catch (e) {
      setResult(e instanceof Error ? e.message : 'Failed')
    }
    setPlacing(false)
  }

  // Calculate estimated shares and cost for preview
  const yesPrice = parseFloat(market.yes_bid || market.yes_price)
  const noPrice = parseFloat(market.no_bid || market.no_price)
  const usdPerSide = parseFloat(size) || 0

  const yesShares = yesPrice > 0 ? Math.floor(usdPerSide / yesPrice) : 0
  const noShares = noPrice > 0 ? Math.floor(usdPerSide / noPrice) : 0
  const yesCost = yesShares * yesPrice
  const noCost = noShares * noPrice
  const totalCost = yesCost + noCost

  const pairCostPerShare = market.yes_bid && market.no_bid
    ? (parseFloat(market.yes_bid) + parseFloat(market.no_bid)).toFixed(4)
    : null

  return (
    <div className="p-3 rounded-lg bg-poly-dark/50 border border-poly-border">
      <div className="flex items-start justify-between mb-2">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-xs font-bold text-yellow-400">{market.asset}</span>
            <span className="text-xs text-gray-500">{market.minutes_left.toFixed(1)}m left</span>
            {market.open_pairs > 0 && (
              <span className="text-xs bg-poly-green/20 text-poly-green px-1.5 py-0.5 rounded">
                {market.open_pairs} open
              </span>
            )}
          </div>
          <a
            href={polymarketUrl(market.slug, market.condition_id)}
            target="_blank"
            rel="noopener noreferrer"
            className="text-sm truncate mt-0.5 hover:text-poly-green transition flex items-center gap-1 group"
          >
            <span className="truncate">{market.question}</span>
            <ExternalLink className="w-3 h-3 flex-shrink-0 opacity-50 group-hover:opacity-100" />
          </a>
        </div>
      </div>

      <div className="grid grid-cols-4 gap-2 text-xs mb-2">
        <div>
          <span className="text-gray-500">YES:</span>
          <span className="ml-1">{(parseFloat(market.yes_price) * 100).toFixed(1)}c</span>
        </div>
        <div>
          <span className="text-gray-500">NO:</span>
          <span className="ml-1">{(parseFloat(market.no_price) * 100).toFixed(1)}c</span>
        </div>
        {market.yes_bid && (
          <div>
            <span className="text-gray-500">Bid Y:</span>
            <span className="ml-1 text-poly-green">{(parseFloat(market.yes_bid) * 100).toFixed(1)}c</span>
          </div>
        )}
        {market.no_bid && (
          <div>
            <span className="text-gray-500">Bid N:</span>
            <span className="ml-1 text-poly-green">{(parseFloat(market.no_bid) * 100).toFixed(1)}c</span>
          </div>
        )}
      </div>

      {pairCostPerShare && (
        <div className="text-xs mb-2">
          <span className="text-gray-500">Per-share cost:</span>
          <span className="ml-1">${pairCostPerShare}</span>
          <span className="text-gray-500 ml-2">Profit/sh:</span>
          <span className="ml-1 text-poly-green">${market.spread_profit || (1 - parseFloat(pairCostPerShare)).toFixed(4)}</span>
        </div>
      )}

      <div className="flex items-center gap-2">
        <div className="relative">
          <span className="absolute left-2 top-1/2 -translate-y-1/2 text-gray-500 text-sm">$</span>
          <input
            type="number"
            value={size}
            onChange={(e) => setSize(e.target.value)}
            placeholder="$ per side"
            className="w-20 pl-5 pr-2 py-1 rounded bg-poly-dark border border-poly-border text-sm"
            min="1"
            step="1"
          />
        </div>
        <input
          type="password"
          value={pw}
          onChange={(e) => setPw(e.target.value)}
          placeholder="Password"
          className="w-24 px-2 py-1 rounded bg-poly-dark border border-poly-border text-sm"
          onKeyDown={(e) => e.key === 'Enter' && handlePlace()}
        />
        <button
          onClick={handlePlace}
          disabled={placing || !pw || !size}
          className="flex items-center gap-1 px-3 py-1 rounded-lg bg-poly-green/20 text-poly-green hover:bg-poly-green/30 transition text-sm disabled:opacity-50"
        >
          {placing ? <Loader2 className="w-3 h-3 animate-spin" /> : <ArrowUpDown className="w-3 h-3" />}
          Place Pair
        </button>
      </div>

      {usdPerSide > 0 && yesPrice > 0 && noPrice > 0 && (
        <div className="text-xs mt-1 text-gray-500">
          Y: {yesShares}sh (${yesCost.toFixed(2)}) + N: {noShares}sh (${noCost.toFixed(2)}) = ${totalCost.toFixed(2)}
        </div>
      )}

      {result && (
        <div className={`text-xs mt-2 ${result.includes('Pair placed') ? 'text-poly-green' : 'text-red-400'}`}>
          {result}
        </div>
      )}
    </div>
  )
}

function PairCard({ pair, sessionToken }: { pair: MintMakerPairSummary; sessionToken: string }) {
  const [cancelling, setCancelling] = useState(false)

  const handleCancel = async () => {
    setCancelling(true)
    try {
      await cancelMintMakerPair(sessionToken, pair.id)
    } catch (e) {
      console.error('Cancel failed:', e)
    }
    setCancelling(false)
  }

  const statusConfig: Record<string, { color: string; bg: string }> = {
    Pending: { color: 'text-yellow-400', bg: 'bg-yellow-400/10' },
    HalfFilled: { color: 'text-orange-400', bg: 'bg-orange-400/10' },
    Matched: { color: 'text-blue-400', bg: 'bg-blue-400/10' },
    Merging: { color: 'text-purple-400', bg: 'bg-purple-400/10' },
    Merged: { color: 'text-poly-green', bg: 'bg-poly-green/10' },
    Cancelled: { color: 'text-gray-500', bg: 'bg-gray-500/10' },
    Orphaned: { color: 'text-orange-400', bg: 'bg-orange-400/10' },
    StopLoss: { color: 'text-red-400', bg: 'bg-red-400/10' },
  }
  const { color: statusColor, bg: statusBg } = statusConfig[pair.status] || { color: 'text-gray-400', bg: 'bg-gray-400/10' }

  const canCancel = ['Pending', 'HalfFilled', 'Orphaned'].includes(pair.status)

  const yesBidCents = (parseFloat(pair.yes_bid_price) * 100).toFixed(1)
  const noBidCents = pair.no_bid_price !== '0' ? (parseFloat(pair.no_bid_price) * 100).toFixed(1) : null
  const yesFillCents = pair.yes_fill_price ? (parseFloat(pair.yes_fill_price) * 100).toFixed(1) : null
  const noFillCents = pair.no_fill_price ? (parseFloat(pair.no_fill_price) * 100).toFixed(1) : null

  return (
    <div className="p-3 rounded-lg bg-poly-dark/50 border border-poly-border">
      {/* Top row: asset badge, status, time */}
      <div className="flex items-center gap-2 mb-1.5">
        <span className="text-xs font-bold text-yellow-400">{pair.asset}</span>
        <span className={`text-xs px-1.5 py-0.5 rounded ${statusColor} ${statusBg}`}>
          {pair.status === 'Orphaned' && <AlertTriangle className="w-3 h-3 inline mr-0.5 -mt-0.5" />}
          {pair.status}
        </span>
        <span className="text-xs text-gray-600 ml-auto">{timeAgo(pair.created_at)}</span>
        {canCancel && (
          <button
            onClick={handleCancel}
            disabled={cancelling}
            className="p-0.5 rounded hover:bg-red-500/20 text-gray-500 hover:text-red-400 transition"
            title="Cancel pair"
          >
            {cancelling ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <X className="w-3.5 h-3.5" />}
          </button>
        )}
      </div>

      {/* Question as link */}
      <a
        href={polymarketUrl(pair.slug, pair.condition_id)}
        target="_blank"
        rel="noopener noreferrer"
        className="text-xs text-gray-400 hover:text-poly-green transition flex items-center gap-1 group truncate mb-2"
      >
        <span className="truncate">{pair.question}</span>
        <ExternalLink className="w-3 h-3 flex-shrink-0 opacity-40 group-hover:opacity-100" />
      </a>

      {/* Two-column YES/NO display */}
      <div className="grid grid-cols-2 gap-3 text-xs">
        <div>
          <span className="text-gray-500">YES</span>
          <div className="flex items-center gap-1">
            <span>{yesBidCents}c</span>
            {yesFillCents && (
              <>
                <span className="text-gray-600">&rarr;</span>
                <span className="text-poly-green">{yesFillCents}c</span>
              </>
            )}
          </div>
          {pair.yes_size && <span className="text-gray-600">{pair.yes_size}sh</span>}
        </div>
        <div>
          <span className="text-gray-500">NO</span>
          {noBidCents ? (
            <>
              <div className="flex items-center gap-1">
                <span>{noBidCents}c</span>
                {noFillCents && (
                  <>
                    <span className="text-gray-600">&rarr;</span>
                    <span className="text-poly-green">{noFillCents}c</span>
                  </>
                )}
              </div>
              {pair.no_size && <span className="text-gray-600">{pair.no_size}sh</span>}
            </>
          ) : (
            <div className="text-gray-600 italic">none (orphaned)</div>
          )}
        </div>
      </div>

      {/* Cost + Profit row */}
      <div className="flex items-center gap-3 mt-2 text-xs">
        {pair.pair_cost && (
          <span className="text-gray-500">
            Cost: <span className="text-gray-300">${parseFloat(pair.pair_cost).toFixed(4)}</span>
          </span>
        )}
        {pair.profit && (
          <span className="text-gray-500">
            Profit: <span className="text-poly-green">${parseFloat(pair.profit).toFixed(4)}</span>
          </span>
        )}
        {pair.merge_tx_id && (
          <a
            href={`https://polygonscan.com/tx/${pair.merge_tx_id}`}
            target="_blank"
            rel="noopener noreferrer"
            className="text-blue-400 hover:text-blue-300 transition flex items-center gap-0.5 ml-auto"
          >
            TX <ExternalLink className="w-3 h-3" />
          </a>
        )}
      </div>
    </div>
  )
}
