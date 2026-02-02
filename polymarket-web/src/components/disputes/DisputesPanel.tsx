import { AlertTriangle, Info } from 'lucide-react'
import { useDisputeStore } from '../../stores/disputeStore'
import { DisputeCard } from './DisputeCard'

export function DisputesPanel() {
  const disputes = useDisputeStore((s) => s.disputes)

  // Count by status
  const proposedCount = disputes.filter((d) => d.dispute_status === 'Proposed').length
  const disputedCount = disputes.filter((d) => d.dispute_status === 'Disputed').length
  const dvmCount = disputes.filter((d) => d.dispute_status === 'DvmVote').length
  const round2Count = disputes.filter((d) => (d.dispute_round || 1) >= 2).length

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-2">
        <div className="flex items-center gap-2">
          <AlertTriangle className="w-5 h-5 text-red-400" />
          <h2 className="text-lg font-semibold">UMA Disputes</h2>
          <span className="text-xs bg-red-500/20 text-red-400 px-2 py-0.5 rounded-full">
            {disputes.length} active
          </span>
        </div>
        {disputes.length > 0 && (
          <div className="flex gap-2 text-xs">
            {proposedCount > 0 && (
              <span className="bg-yellow-500/20 text-yellow-400 px-2 py-0.5 rounded">
                {proposedCount} proposed
              </span>
            )}
            {disputedCount > 0 && (
              <span className="bg-orange-500/20 text-orange-400 px-2 py-0.5 rounded">
                {disputedCount} disputed
              </span>
            )}
            {dvmCount > 0 && (
              <span className="bg-red-500/20 text-red-400 px-2 py-0.5 rounded">
                {dvmCount} DVM voting
              </span>
            )}
            {round2Count > 0 && (
              <span className="bg-blue-500/20 text-blue-400 px-2 py-0.5 rounded">
                {round2Count} round 2
              </span>
            )}
          </div>
        )}
      </div>

      {/* Info box */}
      <div className="bg-red-500/10 border border-red-500/30 rounded-lg p-3">
        <div className="flex items-start gap-2">
          <Info className="w-4 h-4 text-red-400 flex-shrink-0 mt-0.5" />
          <div className="text-sm text-gray-300">
            <p className="font-medium text-red-400 mb-1">What are UMA Disputes?</p>
            <p className="text-gray-400">
              Polymarket uses the UMA Optimistic Oracle (v1/v2/v3 adapters) for resolution.
              When an outcome is proposed, there's a challenge window (typically 2 hours).
              The UmaCtfAdapter uses a <strong>two-round mechanism</strong>: the first dispute
              resets the proposal (round 2), and only a second dispute escalates to UMA DVM
              voting. Bond is typically $750 USDC. Possible DVM outcomes: proposer wins,
              challenger wins, too early, or 50-50 split.
            </p>
          </div>
        </div>
      </div>

      {/* Dispute list */}
      {disputes.length === 0 ? (
        <div className="text-center py-12 text-gray-500">
          <AlertTriangle className="w-12 h-12 mx-auto mb-3 opacity-50" />
          <p className="text-lg">No active disputes</p>
          <p className="text-sm">UMA oracle disputes will appear here</p>
        </div>
      ) : (
        <div className="grid gap-3">
          {disputes.map((alert) => (
            <DisputeCard key={alert.assertion_id} alert={alert} />
          ))}
        </div>
      )}
    </div>
  )
}
