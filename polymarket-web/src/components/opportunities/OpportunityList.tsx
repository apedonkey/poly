import { useState, useRef, useEffect, useMemo, useCallback } from 'react'
import { useOpportunityStore, type FilterType, type SortType, type SideFilter, isCrypto, isSports } from '../../stores/opportunityStore'
import { OpportunityCard } from './OpportunityCard'
import { Filter, Crosshair, Bitcoin, Trophy, ChevronDown } from 'lucide-react'
import type { Opportunity } from '../../types'

export function OpportunityList() {
  const filter = useOpportunityStore((s) => s.filter)
  const setFilter = useOpportunityStore((s) => s.setFilter)
  const sortBy = useOpportunityStore((s) => s.sortBy)
  const setSortBy = useOpportunityStore((s) => s.setSortBy)
  const sideFilter = useOpportunityStore((s) => s.sideFilter)
  const setSideFilter = useOpportunityStore((s) => s.setSideFilter)
  const opportunities = useOpportunityStore((s) => s.opportunities)
  const scanReceivedAt = useOpportunityStore((s) => s.scanReceivedAt)
  const scanElapsedAtReceive = useOpportunityStore((s) => s.scanElapsedAtReceive)
  const scanIntervalSeconds = useOpportunityStore((s) => s.scanIntervalSeconds)
  const scanVersion = useOpportunityStore((s) => s.scanVersion)

  const [showSortMenu, setShowSortMenu] = useState(false)
  const [showPaused, setShowPaused] = useState(false)
  const sortMenuRef = useRef<HTMLDivElement>(null)
  const [_tick, setTick] = useState(0)

  // Tick every second for countdown (triggers re-render)
  useEffect(() => {
    const interval = setInterval(() => setTick(t => t + 1), 1000)
    return () => clearInterval(interval)
  }, [])

  // Calculate progress directly each render (uses _tick implicitly via re-render)
  let scanProgress = 0
  let secondsRemaining = scanIntervalSeconds
  if (scanReceivedAt) {
    const timeSinceReceive = (Date.now() - scanReceivedAt) / 1000
    const totalElapsed = scanElapsedAtReceive + timeSinceReceive
    scanProgress = Math.min(Math.max(0, (totalElapsed / scanIntervalSeconds) * 100), 100)
    secondsRemaining = Math.max(0, Math.round(scanIntervalSeconds - totalElapsed))
  }

  // Close menu when clicking outside
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (sortMenuRef.current && !sortMenuRef.current.contains(e.target as Node)) {
        setShowSortMenu(false)
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [])

  // Pinned card state — pinned card stays frozen in its grid position
  const [pinnedKey, setPinnedKey] = useState<string | null>(null)
  const pinnedIndexRef = useRef<number>(-1)

  // Click outside any opportunity card to unpin
  useEffect(() => {
    if (!pinnedKey) return
    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as HTMLElement
      if (!target.closest('[data-opportunity-card]')) {
        setPinnedKey(null)
        pinnedIndexRef.current = -1
      }
    }
    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [pinnedKey])

  const oppKeyFn = (o: Opportunity) => `${o.market_id}-${o.strategy}-${o.side}`

  // Step 1: Filter opportunities (runs on every update including price changes)
  const filtered = useMemo(() => {
    let result = opportunities

    switch (filter) {
      case 'sniper':
        result = result.filter((o) =>
          o.strategy === 'ResolutionSniper' &&
          !isCrypto(o) &&
          !isSports(o) &&
          o.time_to_close_hours !== null &&
          o.time_to_close_hours <= 12
        )
        break
      case 'crypto':
        result = result.filter(isCrypto)
        break
      case 'sports':
        result = result.filter(isSports)
        break
      default:
        break
    }

    if (sideFilter === 'no') {
      result = result.filter((o) => o.side === 'No')
    } else if (sideFilter === 'yes') {
      result = result.filter((o) => o.side === 'Yes')
    }

    // If a card is pinned, keep it in the list even if it got paused (meets_criteria === false)
    if (pinnedKey) {
      const pinnedInResult = result.some(o => oppKeyFn(o) === pinnedKey)
      if (!pinnedInResult) {
        const pinnedOpp = opportunities.find(o => oppKeyFn(o) === pinnedKey)
        if (pinnedOpp) result.push(pinnedOpp)
      }
    }

    return result
  }, [opportunities, filter, sideFilter, pinnedKey])

  // Step 2: Compute sort order — only re-sort on new scans or sort/filter changes,
  // NOT on price updates. This prevents cards from jumping around.
  const filteredRef = useRef(filtered)
  filteredRef.current = filtered

  const sortOrder = useMemo(() => {
    const sorted = [...filteredRef.current].sort((a, b) => {
      switch (sortBy) {
        case 'time':
          return (a.time_to_close_hours ?? Infinity) - (b.time_to_close_hours ?? Infinity)
        case 'edge':
          return b.edge - a.edge
        case 'return':
          return b.expected_return - a.expected_return
        case 'liquidity':
          return parseFloat(b.liquidity) - parseFloat(a.liquidity)
        default:
          return 0
      }
    })
    return sorted.map(oppKeyFn)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scanVersion, sortBy, filter, sideFilter])

  // Step 3: Apply stable sort order to current data (preserves position on price updates)
  const stableSorted = useMemo(() => {
    const map = new Map(filtered.map(o => [oppKeyFn(o), o]))
    const result: Opportunity[] = []
    for (const key of sortOrder) {
      const opp = map.get(key)
      if (opp) {
        result.push(opp)
        map.delete(key)
      }
    }
    // Append any items not yet in the sort order
    for (const opp of map.values()) {
      result.push(opp)
    }

    // If a card is pinned, keep it at its saved grid position
    if (pinnedKey && pinnedIndexRef.current >= 0) {
      const currentIdx = result.findIndex(o => oppKeyFn(o) === pinnedKey)
      if (currentIdx >= 0 && currentIdx !== pinnedIndexRef.current) {
        const [pinned] = result.splice(currentIdx, 1)
        const insertAt = Math.min(pinnedIndexRef.current, result.length)
        result.splice(insertAt, 0, pinned)
      }
    }

    return result
  }, [filtered, sortOrder, pinnedKey])

  // Ref to let handlePin read current stableSorted without it being a dependency
  const stableSortedRef = useRef(stableSorted)
  stableSortedRef.current = stableSorted

  const handlePin = useCallback((key: string) => {
    setPinnedKey(key)
    const idx = stableSortedRef.current.findIndex(o => oppKeyFn(o) === key)
    pinnedIndexRef.current = idx
  }, [])

  // Get paused opportunities - ONLY from Sniper section
  // (ResolutionSniper + NOT crypto + NOT sports + closing within 12h)
  const pausedOpportunities = useMemo(() =>
    opportunities.filter((o) =>
      o.meets_criteria === false &&
      o.strategy === 'ResolutionSniper' &&
      !isCrypto(o) &&
      !isSports(o) &&
      o.time_to_close_hours !== null &&
      o.time_to_close_hours <= 12
    ),
    [opportunities]
  )
  const pausedCount = pausedOpportunities.length

  // Compute counts using the SAME functions (only count active opportunities)
  const activeOpportunities = useMemo(() =>
    opportunities.filter((o) => o.meets_criteria !== false),
    [opportunities]
  )

  const counts = useMemo(() => ({
    all: activeOpportunities.length,
    sniper: activeOpportunities.filter((o) =>
      o.strategy === 'ResolutionSniper' &&
      !isCrypto(o) &&
      !isSports(o) &&
      o.time_to_close_hours !== null &&
      o.time_to_close_hours <= 12
    ).length,
    crypto: activeOpportunities.filter(isCrypto).length,
    sports: activeOpportunities.filter(isSports).length,
  }), [activeOpportunities])

  const sortOptions: { value: SortType; label: string }[] = [
    { value: 'time', label: 'Ending Soonest' },
    { value: 'edge', label: 'Highest Edge' },
    { value: 'return', label: 'Highest Return' },
    { value: 'liquidity', label: 'Most Liquidity' },
  ]

  const sideOptions: { value: SideFilter; label: string }[] = [
    { value: 'all', label: 'All Sides' },
    { value: 'no', label: 'NO Only' },
    { value: 'yes', label: 'YES Only' },
  ]

  const filters: { value: FilterType; label: string; icon?: React.ReactNode; color?: string }[] = [
    { value: 'all', label: 'All' },
    { value: 'sniper', label: 'Sniper', icon: <Crosshair className="w-3.5 h-3.5" />, color: 'text-yellow-400' },
    { value: 'crypto', label: 'Crypto', icon: <Bitcoin className="w-3.5 h-3.5" />, color: 'text-orange-400' },
    { value: 'sports', label: 'Sports', icon: <Trophy className="w-3.5 h-3.5" />, color: 'text-green-400' },
  ]

  return (
    <div>
      {/* Mobile: Stack controls vertically, Desktop: Side by side */}
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 mb-4">
        <div className="flex items-center gap-2">
          {/* Sort Menu */}
          <div className="relative" ref={sortMenuRef}>
            <button
              onClick={() => setShowSortMenu(!showSortMenu)}
              className="flex items-center gap-1 p-2.5 sm:p-2 hover:bg-poly-card active:bg-poly-card rounded transition touch-target"
            >
              <Filter className="w-5 h-5 text-gray-400" />
              <ChevronDown className="w-3 h-3 text-gray-400" />
            </button>
            {showSortMenu && (
              <div className="absolute top-full left-0 mt-1 bg-poly-card border border-poly-border rounded-lg shadow-lg z-20 min-w-[160px]">
                <div className="p-2 border-b border-poly-border text-xs text-gray-500 font-medium">
                  Sort By
                </div>
                {sortOptions.map((option) => (
                  <button
                    key={option.value}
                    onClick={() => {
                      setSortBy(option.value)
                    }}
                    className={`w-full text-left px-3 py-3 sm:py-2 text-sm hover:bg-poly-dark active:bg-poly-dark transition touch-target ${
                      sortBy === option.value ? 'text-poly-green' : 'text-gray-300'
                    }`}
                  >
                    {option.label}
                  </button>
                ))}
                <div className="p-2 border-t border-b border-poly-border text-xs text-gray-500 font-medium">
                  Side Filter
                </div>
                {sideOptions.map((option) => (
                  <button
                    key={option.value}
                    onClick={() => {
                      setSideFilter(option.value)
                      setShowSortMenu(false)
                    }}
                    className={`w-full text-left px-3 py-3 sm:py-2 text-sm hover:bg-poly-dark active:bg-poly-dark transition touch-target ${
                      sideFilter === option.value ? 'text-poly-green' : 'text-gray-300'
                    }`}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* Filter Pills - Horizontally scrollable on mobile */}
          <div className="flex-1 overflow-x-auto scrollbar-hide mobile-scroll-x -mx-1 px-1">
            <div className="flex bg-poly-card rounded-lg p-1 border border-poly-border w-max">
              {filters.map((f) => (
                <button
                  key={f.value}
                  onClick={() => setFilter(f.value)}
                  className={`flex items-center gap-1 sm:gap-1.5 px-2.5 sm:px-3 py-2 sm:py-1.5 rounded-md text-sm font-medium transition whitespace-nowrap touch-target ${
                    filter === f.value
                      ? 'bg-poly-green text-black'
                      : `text-gray-400 hover:text-white active:text-white ${f.color || ''}`
                  }`}
                >
                  {f.icon}
                  <span className="hidden xs:inline sm:inline">{f.label}</span>
                  <span className="xs:hidden sm:hidden">{f.value === 'all' ? 'All' : ''}</span>
                  <span className="opacity-70 text-xs">({counts[f.value]})</span>
                </button>
              ))}
            </div>
          </div>
        </div>

        {/* Scan progress and status */}
        <div className="flex items-center gap-3 justify-end sm:justify-start">
          <div className="bg-poly-card border border-poly-border rounded-lg px-3 py-2 min-w-[120px]">
            <div className="text-[10px] text-gray-500 uppercase tracking-wide mb-1.5">Next Scan</div>
            <div className="flex items-center gap-2">
              <div className="relative flex-1 h-1.5 bg-poly-dark rounded-full overflow-hidden">
                <div
                  className="absolute inset-y-0 left-0 bg-poly-green rounded-full"
                  style={{ width: `${scanProgress}%`, transition: scanProgress === 0 ? 'none' : 'width 100ms linear' }}
                />
              </div>
              <span className="text-xs tabular-nums text-gray-400 w-6 text-right">{secondsRemaining}s</span>
            </div>
          </div>
          {pausedCount > 0 && (
            <button
              onClick={() => setShowPaused(!showPaused)}
              className={`flex items-center gap-1 text-xs transition hover:text-yellow-400 ${
                showPaused ? 'text-yellow-400' : 'text-yellow-500/70'
              }`}
              title="Click to show/hide paused opportunities"
            >
              <span className={`w-1.5 h-1.5 rounded-full ${showPaused ? 'bg-yellow-400' : 'bg-yellow-500/70'}`} />
              <span>{pausedCount} paused</span>
            </button>
          )}
        </div>
      </div>

      {/* Paused opportunities panel */}
      {showPaused && pausedOpportunities.length > 0 && (
        <div className="mb-4 p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
          <div className="text-sm font-medium text-yellow-400 mb-2">
            Paused Opportunities (price moved outside threshold)
          </div>
          <div className="space-y-2 text-sm">
            {pausedOpportunities.map((opp) => (
              <div key={`paused-${opp.market_id}-${opp.strategy}-${opp.side}`} className="flex items-center justify-between text-gray-300 bg-poly-dark/50 rounded px-2 py-1.5">
                <span className="truncate flex-1 mr-2">{opp.question}</span>
                <div className="flex items-center gap-2 text-xs flex-shrink-0">
                  <span className="text-yellow-400">Sniper</span>
                  <span className="text-gray-500">
                    {(parseFloat(opp.entry_price) * 100).toFixed(0)}c
                  </span>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {stableSorted.length === 0 ? (
        <div className="text-center py-12 text-gray-500">
          <p className="text-lg mb-2">No opportunities found</p>
          <p className="text-sm">Waiting for market data...</p>
        </div>
      ) : (
        <div className="grid gap-3 sm:gap-4 md:grid-cols-2 lg:grid-cols-3">
          {stableSorted.map((opportunity) => {
            const key = oppKeyFn(opportunity)
            return (
              <OpportunityCard
                key={key}
                opportunity={opportunity}
                isPinned={pinnedKey === key}
                onPin={() => handlePin(key)}
              />
            )
          })}
        </div>
      )}
    </div>
  )
}
