import { memo, useState } from 'react'
import { AlertTriangle, ExternalLink, Clock, Scale, TrendingUp, Shield, Repeat } from 'lucide-react'
import type { DisputeAlert } from '../../types'
import { TradeModal } from '../trading/TradeModal'

interface Props {
  alert: DisputeAlert
}

export const DisputeCard = memo(function DisputeCard({ alert }: Props) {
  const [tradeModalOpen, setTradeModalOpen] = useState(false)
  const [tradeSide, setTradeSide] = useState<'Yes' | 'No'>('Yes')

  const yesPrice = (parseFloat(alert.current_yes_price) * 100).toFixed(0)
  const noPrice = (parseFloat(alert.current_no_price) * 100).toFixed(0)
  const edge = alert.edge ? (parseFloat(alert.edge) * 100).toFixed(0) : null
  const ev = alert.expected_value ? (parseFloat(alert.expected_value) * 100).toFixed(1) : null
  const liquidity = parseFloat(alert.liquidity || '0')
  const disputeRound = alert.dispute_round || 1
  const bond = alert.proposer_bond ? parseFloat(alert.proposer_bond) : null
  const adapterVersion = alert.adapter_version || null

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

  const formatLiquidity = (liq: number) => {
    if (liq >= 1_000_000) return `$${(liq / 1_000_000).toFixed(1)}M`
    if (liq >= 1_000) return `$${(liq / 1_000).toFixed(0)}K`
    return `$${liq.toFixed(0)}`
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

  const handleTrade = (side: 'Yes' | 'No') => {
    setTradeSide(side)
    setTradeModalOpen(true)
  }

  // Determine which side to highlight based on proposed outcome
  const proposedIsYes = alert.proposed_outcome?.toUpperCase() === 'YES'
  const proposedIsNo = alert.proposed_outcome?.toUpperCase() === 'NO'

  // Can trade if we have token IDs
  const canTradeYes = !!alert.yes_token_id
  const canTradeNo = !!alert.no_token_id

  return (
    <>
      <div className="bg-poly-card rounded-xl border border-red-500/30 hover:border-red-500/50 transition p-3 sm:p-4">
        {/* Header with badge and question */}
        <div className="flex items-start justify-between gap-2 sm:gap-3 mb-3">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-1.5 sm:gap-2 mb-1 flex-wrap">
              <AlertTriangle className="w-3.5 h-3.5 sm:w-4 sm:h-4 text-red-400 flex-shrink-0" />
              {getStatusBadge(alert.dispute_status)}
              {disputeRound >= 2 && (
                <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-blue-500/20 text-blue-400 flex items-center gap-1">
                  <Repeat className="w-3 h-3" />
                  Round 2
                </span>
              )}
              {ev ? (
                <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-poly-green/20 text-poly-green flex items-center gap-1">
                  <TrendingUp className="w-3 h-3" />
                  EV +{ev}%
                </span>
              ) : edge && (
                <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-poly-green/20 text-poly-green flex items-center gap-1">
                  <TrendingUp className="w-3 h-3" />
                  +{edge}%
                </span>
              )}
              {bond !== null && (
                <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-amber-500/20 text-amber-400 flex items-center gap-1">
                  <Shield className="w-3 h-3" />
                  ${bond.toFixed(0)} bond
                </span>
              )}
              {adapterVersion && (
                <span className="text-xs text-gray-500 px-1 py-0.5 rounded bg-gray-700/50">
                  {adapterVersion}
                </span>
              )}
              <span className="text-xs text-gray-500 flex items-center gap-1">
                <Clock className="w-3 h-3" />
                {formatTimeAgo(alert.dispute_timestamp)}
              </span>
            </div>
            <h3 className="font-medium text-sm leading-tight line-clamp-2">
              {alert.question}
            </h3>
          </div>
          <div className="flex gap-1 sm:gap-2">
            {alert.slug && (
              <a
                href={`https://polymarket.com/event/${alert.slug}`}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1 px-2 py-1 text-xs bg-poly-dark hover:bg-poly-dark/80 rounded transition"
              >
                <ExternalLink className="w-3 h-3" />
                <span className="hidden sm:inline">Polymarket</span>
              </a>
            )}
            <a
              href={`https://oracle.uma.xyz/?assertionId=${alert.assertion_id}&chainId=137`}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1 px-2 py-1 text-xs bg-purple-500/20 text-purple-400 hover:bg-purple-500/30 rounded transition"
            >
              <Scale className="w-3 h-3" />
              <span className="hidden sm:inline">UMA</span>
            </a>
          </div>
        </div>

        {/* Dispute details */}
        <div className="grid grid-cols-2 sm:grid-cols-5 gap-2 sm:gap-3 mb-3">
          <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
            <div className={`text-sm font-bold ${proposedIsYes ? 'text-poly-green' : proposedIsNo ? 'text-poly-red' : 'text-gray-300'}`}>
              {alert.proposed_outcome || '?'}
            </div>
            <div className="text-xs text-gray-500">Proposed</div>
          </div>
          <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
            <div className="text-sm font-bold text-poly-green">{yesPrice}c</div>
            <div className="text-xs text-gray-500">Yes</div>
          </div>
          <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
            <div className="text-sm font-bold text-poly-red">{noPrice}c</div>
            <div className="text-xs text-gray-500">No</div>
          </div>
          <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
            <div className="text-sm font-bold text-gray-300">{formatLiquidity(liquidity)}</div>
            <div className="text-xs text-gray-500">Liquidity</div>
          </div>
          <div className="text-center p-2 bg-poly-dark/30 rounded-lg">
            <div className="text-sm font-bold text-amber-400">
              {formatTimeUntil(alert.estimated_resolution)}
            </div>
            <div className="text-xs text-gray-500">Resolution</div>
          </div>
        </div>

        {/* Trade buttons */}
        <div className="flex gap-2 mb-3">
          <button
            onClick={() => handleTrade('Yes')}
            disabled={!canTradeYes}
            className={`flex-1 py-2 px-3 rounded-lg font-medium text-sm transition ${
              canTradeYes
                ? proposedIsYes
                  ? 'bg-poly-green text-black hover:bg-poly-green/90'
                  : 'bg-poly-green/20 text-poly-green hover:bg-poly-green/30 border border-poly-green/30'
                : 'bg-gray-700 text-gray-500 cursor-not-allowed'
            }`}
          >
            Buy YES @ {yesPrice}c
            {proposedIsYes && edge && <span className="ml-1 opacity-75">(+{edge}%)</span>}
          </button>
          <button
            onClick={() => handleTrade('No')}
            disabled={!canTradeNo}
            className={`flex-1 py-2 px-3 rounded-lg font-medium text-sm transition ${
              canTradeNo
                ? proposedIsNo
                  ? 'bg-poly-red text-white hover:bg-poly-red/90'
                  : 'bg-poly-red/20 text-poly-red hover:bg-poly-red/30 border border-poly-red/30'
                : 'bg-gray-700 text-gray-500 cursor-not-allowed'
            }`}
          >
            Buy NO @ {noPrice}c
            {proposedIsNo && edge && <span className="ml-1 opacity-75">(+{edge}%)</span>}
          </button>
        </div>

        {/* Status explanation */}
        <div className="p-2 bg-red-500/10 border border-red-500/20 rounded-lg">
          <p className="text-xs text-gray-400">
            {alert.dispute_status === 'Proposed' && disputeRound >= 2 && (
              <>
                <span className="text-blue-400 font-medium">Re-Proposal (Round 2): </span>
                First dispute was rejected. This is a stronger re-proposal.
                {(proposedIsYes || proposedIsNo) && <> Buy {alert.proposed_outcome} for {ev ? `EV +${ev}%` : `+${edge}%`} if proposal holds.</>}
                {' '}A second dispute here would escalate to DVM voting.
              </>
            )}
            {alert.dispute_status === 'Proposed' && disputeRound < 2 && (
              <>
                <span className="text-yellow-400 font-medium">Challenge Window: </span>
                {(proposedIsYes || proposedIsNo) ? (
                  <>Proposed: <strong>{alert.proposed_outcome}</strong>. Buy {alert.proposed_outcome} for {ev ? `EV +${ev}%` : `+${edge}%`} if proposal holds.</>
                ) : (
                  <>An outcome has been proposed. Anyone can dispute within the challenge period.</>
                )}
                {' '}If disputed, a re-proposal (round 2) will be required.
              </>
            )}
            {alert.dispute_status === 'Disputed' && (
              <>
                <span className="text-orange-400 font-medium">Disputed (Round {disputeRound}): </span>
                {disputeRound >= 2
                  ? 'Second dispute - escalating to UMA DVM voting. Outcome decided by token holders.'
                  : 'First dispute - adapter will reset and require a new proposal. Moderate risk.'}
              </>
            )}
            {alert.dispute_status === 'DvmVote' && (
              <>
                <span className="text-red-400 font-medium">DVM Vote: </span>
                UMA token holders voting (24-48h). Possible outcomes: proposer wins, challenger wins, too early, or 50-50.
                {' '}Research evidence in UMA Discord before trading.
              </>
            )}
          </p>
        </div>
      </div>

      {/* Trade Modal */}
      {tradeModalOpen && (
        <TradeModal
          isOpen={tradeModalOpen}
          opportunity={{
            market_id: alert.condition_id,
            condition_id: alert.condition_id,
            question: alert.question,
            slug: alert.slug,
            side: tradeSide,
            entry_price: tradeSide === 'Yes' ? alert.current_yes_price : alert.current_no_price,
            edge: parseFloat(alert.edge || '0'),
            expected_return: parseFloat(alert.edge || '0') * 100,
            confidence: 0.5,
            strategy: 'Dispute',
            liquidity: alert.liquidity,
            volume: '0',
            time_to_close_hours: null,
            category: null,
            resolution_source: null,
            description: null,
            recommendation: `Buy ${tradeSide} if you believe the ${alert.proposed_outcome} proposal will hold`,
            token_id: tradeSide === 'Yes' ? alert.yes_token_id : alert.no_token_id,
          }}
          onClose={() => setTradeModalOpen(false)}
        />
      )}
    </>
  )
})
