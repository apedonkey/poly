import { useState } from 'react'
import { Clock, TrendingUp, Droplets, ExternalLink, Target, Crosshair } from 'lucide-react'
import type { Opportunity } from '../../types'
import { TradeModal } from '../trading/TradeModal'

interface Props {
  opportunity: Opportunity
}

export function OpportunityCard({ opportunity }: Props) {
  const [tradeModalOpen, setTradeModalOpen] = useState(false)

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
      <div className="bg-poly-card rounded-xl border border-poly-border hover:border-poly-green/50 transition p-4">
        <div className="flex items-start justify-between gap-3 mb-3">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              {isSniper ? (
                <Crosshair className="w-4 h-4 text-yellow-400 flex-shrink-0" />
              ) : (
                <Target className="w-4 h-4 text-blue-400 flex-shrink-0" />
              )}
              <span className={`text-xs font-medium px-2 py-0.5 rounded ${
                isSniper
                  ? 'bg-yellow-500/20 text-yellow-400'
                  : 'bg-blue-500/20 text-blue-400'
              }`}>
                {isSniper ? 'Sniper' : 'NO Bias'}
              </span>
              {opportunity.category && (
                <span className="text-xs text-gray-500">{opportunity.category}</span>
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
              className="p-1.5 hover:bg-poly-dark rounded transition flex-shrink-0"
            >
              <ExternalLink className="w-4 h-4 text-gray-500" />
            </a>
          ) : (
            <span className="p-1.5 flex-shrink-0 opacity-30 cursor-not-allowed" title="Link unavailable">
              <ExternalLink className="w-4 h-4 text-gray-500" />
            </span>
          )}
        </div>

        <div className="grid grid-cols-4 gap-3 mb-3">
          <div className="text-center">
            <div className={`text-lg font-bold ${
              opportunity.side === 'Yes' ? 'text-poly-green' : 'text-poly-red'
            }`}>
              {opportunity.side}
            </div>
            <div className="text-xs text-gray-500">{pricePercent}c</div>
          </div>
          <div className="text-center">
            <div className="text-lg font-bold text-poly-green">+{edgePercent}%</div>
            <div className="text-xs text-gray-500">Edge</div>
          </div>
          <div className="text-center">
            <div className="text-lg font-bold">{returnPercent}%</div>
            <div className="text-xs text-gray-500">Return</div>
          </div>
          <div className="text-center">
            <div className="text-lg font-bold">{formatTime(opportunity.time_to_close_hours)}</div>
            <div className="text-xs text-gray-500">Close</div>
          </div>
        </div>

        <div className="flex items-center justify-between text-xs text-gray-500 mb-3">
          <div className="flex items-center gap-1">
            <Droplets className="w-3.5 h-3.5" />
            {formatVolume(opportunity.liquidity)}
          </div>
          <div className="flex items-center gap-1">
            <TrendingUp className="w-3.5 h-3.5" />
            {formatVolume(opportunity.volume)} vol
          </div>
          <div className="flex items-center gap-1">
            <Clock className="w-3.5 h-3.5" />
            {(opportunity.confidence * 100).toFixed(0)}% conf
          </div>
        </div>

        <button
          onClick={() => setTradeModalOpen(true)}
          className={`w-full py-2 font-semibold rounded-lg transition ${
            opportunity.side === 'Yes'
              ? 'bg-poly-green/20 text-poly-green hover:bg-poly-green/30 border border-poly-green/50'
              : 'bg-poly-red/20 text-poly-red hover:bg-poly-red/30 border border-poly-red/50'
          }`}
        >
          Buy {opportunity.side} @ {pricePercent}c
        </button>
      </div>

      <TradeModal
        isOpen={tradeModalOpen}
        onClose={() => setTradeModalOpen(false)}
        opportunity={opportunity}
      />
    </>
  )
}
