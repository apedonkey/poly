import { useState, memo } from 'react'
import { Clock, TrendingUp, Droplets, ExternalLink, Target, Crosshair, ChevronDown, ChevronUp, FileText } from 'lucide-react'
import type { Opportunity } from '../../types'
import { TradeModal } from '../trading/TradeModal'

interface Props {
  opportunity: Opportunity
}

export const OpportunityCard = memo(function OpportunityCard({ opportunity }: Props) {
  const [tradeModalOpen, setTradeModalOpen] = useState(false)
  const [rulesExpanded, setRulesExpanded] = useState(false)

  const pricePercent = (parseFloat(opportunity.entry_price) * 100).toFixed(0)
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

  const isSniper = opportunity.strategy === 'ResolutionSniper'

  return (
    <>
      <div className="bg-poly-card rounded-xl border border-poly-border hover:border-poly-green/50 active:border-poly-green/50 transition p-3 sm:p-4">
        {/* Header with strategy badge and question */}
        <div className="flex items-start justify-between gap-2 sm:gap-3 mb-3">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-1.5 sm:gap-2 mb-1 flex-wrap">
              {isSniper ? (
                <Crosshair className="w-3.5 h-3.5 sm:w-4 sm:h-4 text-yellow-400 flex-shrink-0" />
              ) : (
                <Target className="w-3.5 h-3.5 sm:w-4 sm:h-4 text-blue-400 flex-shrink-0" />
              )}
              <span className={`text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded ${
                isSniper
                  ? 'bg-yellow-500/20 text-yellow-400'
                  : 'bg-blue-500/20 text-blue-400'
              }`}>
                {isSniper ? 'Sniper' : 'NO Bias'}
              </span>
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
            <div className="text-xs text-gray-500">{pricePercent}c</div>
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
    o1.description === o2.description
  )
})
