import { memo } from 'react'
import { FileText, ExternalLink, Clock, DollarSign } from 'lucide-react'
import type { ClarificationAlert } from '../../types'

interface Props {
  alert: ClarificationAlert
}

export const ClarificationCard = memo(function ClarificationCard({ alert }: Props) {
  const yesPrice = (parseFloat(alert.current_yes_price) * 100).toFixed(0)
  const noPrice = (parseFloat(alert.current_no_price) * 100).toFixed(0)
  const liquidity = parseFloat(alert.liquidity)

  const formatLiquidity = (liq: number) => {
    if (liq >= 1_000_000) return `$${(liq / 1_000_000).toFixed(1)}M`
    if (liq >= 1_000) return `$${(liq / 1_000).toFixed(0)}K`
    return `$${liq.toFixed(0)}`
  }

  const formatTimeAgo = (timestamp: number) => {
    const now = Date.now() / 1000
    const diff = now - timestamp
    if (diff < 60) return 'Just now'
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
    return `${Math.floor(diff / 86400)}d ago`
  }

  return (
    <div className="bg-poly-card rounded-xl border border-amber-500/30 hover:border-amber-500/50 transition p-3 sm:p-4">
      {/* Header with badge and question */}
      <div className="flex items-start justify-between gap-2 sm:gap-3 mb-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 sm:gap-2 mb-1 flex-wrap">
            <FileText className="w-3.5 h-3.5 sm:w-4 sm:h-4 text-amber-400 flex-shrink-0" />
            <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-amber-500/20 text-amber-400">
              Clarification
            </span>
            <span className="text-xs text-gray-500 flex items-center gap-1">
              <Clock className="w-3 h-3" />
              {formatTimeAgo(alert.detected_at)}
            </span>
          </div>
          <h3 className="font-medium text-sm leading-tight line-clamp-2">
            {alert.question}
          </h3>
        </div>
        {alert.slug ? (
          <a
            href={`https://polymarket.com/event/${alert.slug}`}
            target="_blank"
            rel="noopener noreferrer"
            className="p-2 sm:p-1.5 hover:bg-poly-dark active:bg-poly-dark rounded transition flex-shrink-0 -mr-1 sm:mr-0"
          >
            <ExternalLink className="w-4 h-4 text-gray-500" />
          </a>
        ) : (
          <span className="p-2 sm:p-1.5 flex-shrink-0 opacity-30 cursor-not-allowed -mr-1 sm:mr-0">
            <ExternalLink className="w-4 h-4 text-gray-500" />
          </span>
        )}
      </div>

      {/* Price and liquidity info */}
      <div className="grid grid-cols-3 gap-2 sm:gap-3 mb-3">
        <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
          <div className="text-sm font-bold text-poly-green">{yesPrice}c</div>
          <div className="text-xs text-gray-500">Yes</div>
        </div>
        <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
          <div className="text-sm font-bold text-poly-red">{noPrice}c</div>
          <div className="text-xs text-gray-500">No</div>
        </div>
        <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
          <div className="text-sm font-bold flex items-center justify-center gap-1">
            <DollarSign className="w-3 h-3" />
            {formatLiquidity(liquidity)}
          </div>
          <div className="text-xs text-gray-500">Liquidity</div>
        </div>
      </div>

      {/* Description preview */}
      {alert.new_description_preview && (
        <div className="p-2 bg-amber-500/10 border border-amber-500/20 rounded-lg">
          <div className="text-xs text-amber-400 font-medium mb-1">Updated Description:</div>
          <p className="text-xs text-gray-400 line-clamp-4 whitespace-pre-wrap">
            {alert.new_description_preview}
          </p>
        </div>
      )}
    </div>
  )
})
