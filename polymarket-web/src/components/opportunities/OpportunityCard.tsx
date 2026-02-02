import { useState, useEffect, memo } from 'react'
import { Clock, TrendingUp, Droplets, ExternalLink, Crosshair, ChevronDown, ChevronUp, FileText, ShoppingCart, Users } from 'lucide-react'
import type { Opportunity, MarketHolder } from '../../types'
import { TradeModal } from '../trading/TradeModal'
import { useOrderStore } from '../../stores/orderStore'

function formatHolderAmount(amount: number): string {
  if (amount >= 1_000_000) return `$${(amount / 1_000_000).toFixed(1)}M`
  if (amount >= 1_000) return `$${(amount / 1_000).toFixed(1)}K`
  return `$${amount.toFixed(0)}`
}

function HolderRow({ rank, holder }: { rank: number; holder: MarketHolder }) {
  return (
    <div className="flex items-center justify-between text-xs py-0.5">
      <div className="flex items-center gap-1 min-w-0">
        <span className="text-gray-600 w-3 flex-shrink-0">{rank}.</span>
        <span className="text-gray-300 truncate" title={holder.address}>{holder.name}</span>
      </div>
      <span className="text-gray-400 flex-shrink-0 ml-1 font-mono">{formatHolderAmount(holder.amount)}</span>
    </div>
  )
}

interface Props {
  opportunity: Opportunity
  isPinned?: boolean
  onPin?: () => void
}

export const OpportunityCard = memo(function OpportunityCard({ opportunity, isPinned, onPin }: Props) {
  const [tradeModalOpen, setTradeModalOpen] = useState(false)
  const [rulesExpanded, setRulesExpanded] = useState(false)
  const [holdersExpanded, setHoldersExpanded] = useState(false)
  const hasPendingOrder = useOrderStore((s) => s.hasPendingOrder(opportunity.market_id))

  // Live price from WebSocket event (immediate, not debounced)
  const [livePrice, setLivePrice] = useState<string | null>(null)
  const [priceFlash, setPriceFlash] = useState<'up' | 'down' | null>(null)

  useEffect(() => {
    if (!opportunity.token_id) return

    const handlePriceUpdate = (event: CustomEvent<{ token_id: string; price: string }>) => {
      const { token_id, price } = event.detail
      if (token_id === opportunity.token_id) {
        setLivePrice((prev) => {
          if (prev !== null) {
            const prevNum = parseFloat(prev)
            const currNum = parseFloat(price)
            if (currNum > prevNum) setPriceFlash('up')
            else if (currNum < prevNum) setPriceFlash('down')
            setTimeout(() => setPriceFlash(null), 500)
          }
          return price
        })
      }
    }

    window.addEventListener('price-update', handlePriceUpdate as EventListener)
    return () => window.removeEventListener('price-update', handlePriceUpdate as EventListener)
  }, [opportunity.token_id])

  const currentPrice = livePrice ?? opportunity.entry_price
  const pricePercent = (parseFloat(currentPrice) * 100).toFixed(0)
  const edgePercent = (opportunity.edge * 100).toFixed(1)
  const returnPercent = (opportunity.expected_return * 100).toFixed(1)

  const formatTime = (hours: number | null) => {
    if (hours === null) return '?'
    if (hours < 1) return `${(hours * 60).toFixed(0)}m`
    if (hours < 24) return `${hours.toFixed(1)}h`
    if (hours < 168) return `${(hours / 24).toFixed(1)}d`
    return `${(hours / 168).toFixed(1)}w`
  }

  const formatVolume = (volume: string) => {
    const num = parseFloat(volume)
    if (num >= 1_000_000) return `$${(num / 1_000_000).toFixed(1)}M`
    if (num >= 1_000) return `$${(num / 1_000).toFixed(0)}K`
    return `$${num.toFixed(0)}`
  }

  return (
    <>
      <div
        data-opportunity-card
        onClick={(e) => {
          // Only pin when clicking the card body, not interactive elements
          if ((e.target as HTMLElement).closest('button, a')) return
          onPin?.()
        }}
        className={`bg-poly-card rounded-xl border transition p-3 sm:p-4 ${
          isPinned
            ? 'border-poly-green shadow-[0_0_10px_rgba(0,255,102,0.15)]'
            : 'border-poly-border hover:border-poly-green/50 active:border-poly-green/50'
        } ${opportunity.meets_criteria === false ? 'opacity-60' : ''}`}>
        {/* Header with strategy badge and question */}
        <div className="flex items-start justify-between gap-2 sm:gap-3 mb-3">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-1.5 sm:gap-2 mb-1 flex-wrap">
              <Crosshair className="w-3.5 h-3.5 sm:w-4 sm:h-4 text-yellow-400 flex-shrink-0" />
              <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-yellow-500/20 text-yellow-400">
                Sniper
              </span>
              {opportunity.meets_criteria === false && (
                <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-yellow-500/20 text-yellow-400">
                  Paused
                </span>
              )}
              {hasPendingOrder && (
                <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-orange-500/20 text-orange-400 flex items-center gap-1">
                  <ShoppingCart className="w-3 h-3" />
                  Order Pending
                </span>
              )}
              {opportunity.category && (
                <span className="text-xs text-gray-500 truncate">{opportunity.category}</span>
              )}
            </div>
            <h3 className="font-medium text-sm leading-tight line-clamp-2">
              {opportunity.question}
            </h3>
          </div>
          {opportunity.slug ? (
            <a
              href={`https://polymarket.com/event/${opportunity.slug}`}
              target="_blank"
              rel="noopener noreferrer"
              className="p-2 sm:p-1.5 hover:bg-poly-dark active:bg-poly-dark rounded transition flex-shrink-0 -mr-1 sm:mr-0"
            >
              <ExternalLink className="w-4 h-4 text-gray-500" />
            </a>
          ) : (
            <span className="p-2 sm:p-1.5 flex-shrink-0 opacity-30 cursor-not-allowed -mr-1 sm:mr-0" title="Link unavailable">
              <ExternalLink className="w-4 h-4 text-gray-500" />
            </span>
          )}
        </div>

        {/* Stats Grid - 2x2 on mobile, 4 cols on larger screens */}
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 sm:gap-3 mb-3">
          <div className="text-center p-2 sm:p-0 bg-poly-dark/30 sm:bg-transparent rounded-lg">
            <div className={`text-base sm:text-lg font-bold ${
              opportunity.side === 'Yes' ? 'text-poly-green' : 'text-poly-red'
            }`}>
              {opportunity.side}
            </div>
            <div className={`text-xs transition-colors duration-500 ${
              priceFlash === 'up' ? 'text-green-400' : priceFlash === 'down' ? 'text-red-400' : 'text-gray-500'
            }`}>{pricePercent}c</div>
          </div>
          <div className="text-center p-2 sm:p-0 bg-poly-dark/30 sm:bg-transparent rounded-lg">
            <div className="text-base sm:text-lg font-bold text-poly-green">+{edgePercent}%</div>
            <div className="text-xs text-gray-500">Edge</div>
          </div>
          <div className="text-center p-2 sm:p-0 bg-poly-dark/30 sm:bg-transparent rounded-lg">
            <div className="text-base sm:text-lg font-bold">{returnPercent}%</div>
            <div className="text-xs text-gray-500">Return</div>
          </div>
          <div className="text-center p-2 sm:p-0 bg-poly-dark/30 sm:bg-transparent rounded-lg">
            <div className="text-base sm:text-lg font-bold">{formatTime(opportunity.time_to_close_hours)}</div>
            <div className="text-xs text-gray-500">Close</div>
          </div>
        </div>

        {/* Secondary Stats Row */}
        <div className="flex items-center justify-between text-xs text-gray-500 mb-3 px-1">
          <div className="flex items-center gap-1">
            <Droplets className="w-3 h-3 sm:w-3.5 sm:h-3.5" />
            <span>{formatVolume(opportunity.liquidity)}</span>
          </div>
          <div className="flex items-center gap-1">
            <TrendingUp className="w-3 h-3 sm:w-3.5 sm:h-3.5" />
            <span>{formatVolume(opportunity.volume)}</span>
          </div>
          <div className="flex items-center gap-1">
            <Clock className="w-3 h-3 sm:w-3.5 sm:h-3.5" />
            <span>{(opportunity.confidence * 100).toFixed(0)}%</span>
          </div>
        </div>

        {/* Resolution Rules - Collapsible */}
        {opportunity.description && (
          <div className="mb-3">
            <button
              onClick={() => setRulesExpanded(!rulesExpanded)}
              className="w-full flex items-center justify-between text-xs text-gray-400 hover:text-gray-300 transition px-1 py-1"
            >
              <div className="flex items-center gap-1">
                <FileText className="w-3 h-3" />
                <span>Resolution Rules</span>
              </div>
              {rulesExpanded ? (
                <ChevronUp className="w-3.5 h-3.5" />
              ) : (
                <ChevronDown className="w-3.5 h-3.5" />
              )}
            </button>
            {rulesExpanded && (
              <div className="mt-2 p-2 bg-poly-dark/50 rounded-lg text-xs text-gray-400 max-h-48 overflow-y-auto whitespace-pre-wrap">
                {opportunity.description}
              </div>
            )}
          </div>
        )}

        {/* Top Holders - Collapsible */}
        {opportunity.holders && (opportunity.holders.yes_holders.length > 0 || opportunity.holders.no_holders.length > 0) && (
          <div className="mb-3">
            <button
              onClick={() => setHoldersExpanded(!holdersExpanded)}
              className="w-full flex items-center justify-between text-xs text-gray-400 hover:text-gray-300 transition px-1 py-1"
            >
              <div className="flex items-center gap-1">
                <Users className="w-3 h-3" />
                <span>Top Holders</span>
              </div>
              {holdersExpanded ? (
                <ChevronUp className="w-3.5 h-3.5" />
              ) : (
                <ChevronDown className="w-3.5 h-3.5" />
              )}
            </button>
            {holdersExpanded && (
              <div className="mt-2 grid grid-cols-2 gap-3">
                {/* YES Side */}
                <div>
                  <div className="text-xs font-medium text-poly-green mb-1.5">YES Side</div>
                  {opportunity.holders.yes_holders.map((holder, i) => (
                    <HolderRow key={i} rank={i + 1} holder={holder} />
                  ))}
                  {opportunity.holders.yes_total_count > 5 && (
                    <div className="text-xs text-gray-500 mt-1 pl-4">
                      +{opportunity.holders.yes_total_count - 5} more
                    </div>
                  )}
                  {opportunity.holders.yes_holders.length === 0 && (
                    <div className="text-xs text-gray-600 italic">No holders</div>
                  )}
                </div>
                {/* NO Side */}
                <div>
                  <div className="text-xs font-medium text-poly-red mb-1.5">NO Side</div>
                  {opportunity.holders.no_holders.map((holder, i) => (
                    <HolderRow key={i} rank={i + 1} holder={holder} />
                  ))}
                  {opportunity.holders.no_total_count > 5 && (
                    <div className="text-xs text-gray-500 mt-1 pl-4">
                      +{opportunity.holders.no_total_count - 5} more
                    </div>
                  )}
                  {opportunity.holders.no_holders.length === 0 && (
                    <div className="text-xs text-gray-600 italic">No holders</div>
                  )}
                </div>
              </div>
            )}
          </div>
        )}

        {/* Trade Button - Larger touch target on mobile */}
        <button
          onClick={() => setTradeModalOpen(true)}
          className={`w-full py-3 sm:py-2 font-semibold rounded-lg transition touch-target active:scale-[0.98] ${
            opportunity.side === 'Yes'
              ? 'bg-poly-green/20 text-poly-green hover:bg-poly-green/30 active:bg-poly-green/30 border border-poly-green/50'
              : 'bg-poly-red/20 text-poly-red hover:bg-poly-red/30 active:bg-poly-red/30 border border-poly-red/50'
          }`}
        >
          Buy {opportunity.side} @ {pricePercent}c
        </button>
      </div>

      {tradeModalOpen && (
        <TradeModal
          isOpen={tradeModalOpen}
          onClose={() => setTradeModalOpen(false)}
          opportunity={opportunity}
        />
      )}
    </>
  )
}, (prev, next) => {
  // Custom comparison - only re-render if these fields change
  if (prev.isPinned !== next.isPinned) return false
  const o1 = prev.opportunity
  const o2 = next.opportunity
  return (
    o1.market_id === o2.market_id &&
    o1.entry_price === o2.entry_price &&
    o1.edge === o2.edge &&
    o1.expected_return === o2.expected_return &&
    o1.time_to_close_hours === o2.time_to_close_hours &&
    o1.side === o2.side &&
    o1.meets_criteria === o2.meets_criteria &&
    o1.description === o2.description &&
    (o1.holders?.yes_total_count ?? -1) === (o2.holders?.yes_total_count ?? -1) &&
    (o1.holders?.no_total_count ?? -1) === (o2.holders?.no_total_count ?? -1)
  )
})
