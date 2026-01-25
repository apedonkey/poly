import { useState, useRef, useEffect, useMemo } from 'react'
import { useOpportunityStore, type FilterType, type SortType, isCrypto, isSports } from '../../stores/opportunityStore'
import { OpportunityCard } from './OpportunityCard'
import { Filter, RefreshCw, Crosshair, Target, Bitcoin, Trophy, ChevronDown } from 'lucide-react'

export function OpportunityList() {
  const filter = useOpportunityStore((s) => s.filter)
  const setFilter = useOpportunityStore((s) => s.setFilter)
  const sortBy = useOpportunityStore((s) => s.sortBy)
  const setSortBy = useOpportunityStore((s) => s.setSortBy)
  const opportunities = useOpportunityStore((s) => s.opportunities)

  const [showSortMenu, setShowSortMenu] = useState(false)
  const sortMenuRef = useRef<HTMLDivElement>(null)

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

  // Compute filtered results using the SAME functions as the store
  const filtered = useMemo(() => {
    let result = opportunities

    switch (filter) {
      case 'sniper':
        result = opportunities.filter((o) =>
          o.strategy === 'ResolutionSniper' &&
          !isCrypto(o) &&
          !isSports(o) &&
          o.time_to_close_hours !== null &&
          o.time_to_close_hours <= 12
        )
        break
      case 'nobias':
        result = opportunities.filter((o) => o.strategy === 'NoBias')
        break
      case 'crypto':
        result = opportunities.filter(isCrypto)
        break
      case 'sports':
        result = opportunities.filter(isSports)
        break
      default:
        result = opportunities
    }

    // Sort
    return [...result].sort((a, b) => {
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
  }, [opportunities, filter, sortBy])

  // Compute counts using the SAME functions
  const counts = useMemo(() => ({
    all: opportunities.length,
    sniper: opportunities.filter((o) =>
      o.strategy === 'ResolutionSniper' &&
      !isCrypto(o) &&
      !isSports(o) &&
      o.time_to_close_hours !== null &&
      o.time_to_close_hours <= 12
    ).length,
    nobias: opportunities.filter((o) => o.strategy === 'NoBias').length,
    crypto: opportunities.filter(isCrypto).length,
    sports: opportunities.filter(isSports).length,
  }), [opportunities])

  const sortOptions: { value: SortType; label: string }[] = [
    { value: 'time', label: 'Ending Soonest' },
    { value: 'edge', label: 'Highest Edge' },
    { value: 'return', label: 'Highest Return' },
    { value: 'liquidity', label: 'Most Liquidity' },
  ]

  const filters: { value: FilterType; label: string; icon?: React.ReactNode; color?: string }[] = [
    { value: 'all', label: 'All' },
    { value: 'sniper', label: 'Sniper', icon: <Crosshair className="w-3.5 h-3.5" />, color: 'text-yellow-400' },
    { value: 'nobias', label: 'NO Bias', icon: <Target className="w-3.5 h-3.5" />, color: 'text-blue-400' },
    { value: 'crypto', label: 'Crypto', icon: <Bitcoin className="w-3.5 h-3.5" />, color: 'text-orange-400' },
    { value: 'sports', label: 'Sports', icon: <Trophy className="w-3.5 h-3.5" />, color: 'text-green-400' },
  ]

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <div className="relative" ref={sortMenuRef}>
            <button
              onClick={() => setShowSortMenu(!showSortMenu)}
              className="flex items-center gap-1 p-2 hover:bg-poly-card rounded transition"
            >
              <Filter className="w-5 h-5 text-gray-400" />
              <ChevronDown className="w-3 h-3 text-gray-400" />
            </button>
            {showSortMenu && (
              <div className="absolute top-full left-0 mt-1 bg-poly-card border border-poly-border rounded-lg shadow-lg z-10 min-w-[160px]">
                <div className="p-2 border-b border-poly-border text-xs text-gray-500 font-medium">
                  Sort By
                </div>
                {sortOptions.map((option) => (
                  <button
                    key={option.value}
                    onClick={() => {
                      setSortBy(option.value)
                      setShowSortMenu(false)
                    }}
                    className={`w-full text-left px-3 py-2 text-sm hover:bg-poly-dark transition ${
                      sortBy === option.value ? 'text-poly-green' : 'text-gray-300'
                    }`}
                  >
                    {option.label}
                  </button>
                ))}
              </div>
            )}
          </div>
          <div className="flex bg-poly-card rounded-lg p-1 border border-poly-border">
            {filters.map((f) => (
              <button
                key={f.value}
                onClick={() => setFilter(f.value)}
                className={`flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm font-medium transition ${
                  filter === f.value
                    ? 'bg-poly-green text-black'
                    : `text-gray-400 hover:text-white ${f.color || ''}`
                }`}
              >
                {f.icon}
                {f.label}
                <span className="opacity-70">({counts[f.value]})</span>
              </button>
            ))}
          </div>
        </div>
        <div className="flex items-center gap-2 text-sm text-gray-500">
          <RefreshCw className="w-4 h-4 animate-spin" />
          Live updates
        </div>
      </div>

      {filtered.length === 0 ? (
        <div className="text-center py-12 text-gray-500">
          <p className="text-lg mb-2">No opportunities found</p>
          <p className="text-sm">Waiting for market data...</p>
        </div>
      ) : (
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
          {filtered.map((opportunity, index) => (
            <OpportunityCard key={`${opportunity.market_id}-${opportunity.strategy}-${opportunity.side}-${index}`} opportunity={opportunity} />
          ))}
        </div>
      )}
    </div>
  )
}
