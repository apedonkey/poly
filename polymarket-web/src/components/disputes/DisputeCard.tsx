import { memo } from 'react'
import { AlertTriangle, ExternalLink, Clock, Scale } from 'lucide-react'
import type { DisputeAlert } from '../../types'

interface Props {
  alert: DisputeAlert
}

export const DisputeCard = memo(function DisputeCard({ alert }: Props) {
  const yesPrice = (parseFloat(alert.current_yes_price) * 100).toFixed(0)
  const noPrice = (parseFloat(alert.current_no_price) * 100).toFixed(0)

  const formatTimeAgo = (timestamp: number) => {
    const now = Date.now() / 1000
    const diff = now - timestamp
    if (diff < 60) return 'Just now'
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`
    return `${Math.floor(diff / 86400)}d ago`
  }

  const formatTimeUntil = (timestamp: number) => {
    const now = Date.now() / 1000
    const diff = timestamp - now
    if (diff <= 0) return 'Ended'
    if (diff < 3600) return `${Math.ceil(diff / 60)}m`
    if (diff < 86400) return `${Math.ceil(diff / 3600)}h`
    return `${Math.ceil(diff / 86400)}d`
  }

  const getStatusBadge = (status: string) => {
    switch (status) {
      case 'Proposed':
        return (
          <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-yellow-500/20 text-yellow-400">
            Proposed
          </span>
        )
      case 'Disputed':
        return (
          <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-orange-500/20 text-orange-400">
            Disputed
          </span>
        )
      case 'DvmVote':
        return (
          <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-red-500/20 text-red-400">
            DVM Vote
          </span>
        )
      default:
        return (
          <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-gray-500/20 text-gray-400">
            {status}
          </span>
        )
    }
  }

  return (
    <div className="bg-poly-card rounded-xl border border-red-500/30 hover:border-red-500/50 transition p-3 sm:p-4">
      {/* Header with badge and question */}
      <div className="flex items-start justify-between gap-2 sm:gap-3 mb-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 sm:gap-2 mb-1 flex-wrap">
            <AlertTriangle className="w-3.5 h-3.5 sm:w-4 sm:h-4 text-red-400 flex-shrink-0" />
            {getStatusBadge(alert.dispute_status)}
            <span className="text-xs text-gray-500 flex items-center gap-1">
              <Clock className="w-3 h-3" />
              {formatTimeAgo(alert.dispute_timestamp)}
            </span>
          </div>
          <h3 className="font-medium text-sm leading-tight line-clamp-2">
            {alert.question}
          </h3>
        </div>
        <div className="flex gap-1">
          {alert.slug && (
            <a
              href={`https://polymarket.com/event/${alert.slug}`}
              target="_blank"
              rel="noopener noreferrer"
              className="p-2 sm:p-1.5 hover:bg-poly-dark active:bg-poly-dark rounded transition flex-shrink-0"
              title="View on Polymarket"
            >
              <ExternalLink className="w-4 h-4 text-gray-500" />
            </a>
          )}
          <a
            href="https://oracle.uma.xyz/"
            target="_blank"
            rel="noopener noreferrer"
            className="p-2 sm:p-1.5 hover:bg-poly-dark active:bg-poly-dark rounded transition flex-shrink-0"
            title="View on UMA Oracle"
          >
            <Scale className="w-4 h-4 text-purple-400" />
          </a>
        </div>
      </div>

      {/* Dispute details */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 sm:gap-3 mb-3">
        <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
          <div className="text-sm font-bold text-gray-300">{alert.proposed_outcome || '?'}</div>
          <div className="text-xs text-gray-500">Proposed</div>
        </div>
        <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
          <div className="text-sm font-bold text-poly-green">{yesPrice}c</div>
          <div className="text-xs text-gray-500">Yes Price</div>
        </div>
        <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
          <div className="text-sm font-bold text-poly-red">{noPrice}c</div>
          <div className="text-xs text-gray-500">No Price</div>
        </div>
        <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
          <div className="text-sm font-bold text-amber-400">
            {formatTimeUntil(alert.estimated_resolution)}
          </div>
          <div className="text-xs text-gray-500">Resolution</div>
        </div>
      </div>

      {/* Status explanation */}
      <div className="p-2 bg-red-500/10 border border-red-500/20 rounded-lg">
        <p className="text-xs text-gray-400">
          {alert.dispute_status === 'Proposed' && (
            <>
              <span className="text-yellow-400 font-medium">Challenge Window: </span>
              An outcome has been proposed. Anyone can dispute within 2 hours.
            </>
          )}
          {alert.dispute_status === 'Disputed' && (
            <>
              <span className="text-orange-400 font-medium">Disputed: </span>
              The proposed outcome was challenged. The oracle will auto-reset or escalate.
            </>
          )}
          {alert.dispute_status === 'DvmVote' && (
            <>
              <span className="text-red-400 font-medium">DVM Vote in Progress: </span>
              UMA token holders are voting on the final resolution.
            </>
          )}
        </p>
      </div>
    </div>
  )
})
