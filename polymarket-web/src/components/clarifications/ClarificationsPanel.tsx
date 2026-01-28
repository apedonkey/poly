import { FileText, AlertCircle } from 'lucide-react'
import { useClarificationStore } from '../../stores/clarificationStore'
import { ClarificationCard } from './ClarificationCard'

export function ClarificationsPanel() {
  const clarifications = useClarificationStore((s) => s.clarifications)

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <FileText className="w-5 h-5 text-amber-400" />
          <h2 className="text-lg font-semibold">Market Clarifications</h2>
          <span className="text-xs bg-amber-500/20 text-amber-400 px-2 py-0.5 rounded-full">
            {clarifications.length} alerts
          </span>
        </div>
      </div>

      {/* Info box */}
      <div className="bg-amber-500/10 border border-amber-500/30 rounded-lg p-3">
        <div className="flex items-start gap-2">
          <AlertCircle className="w-4 h-4 text-amber-400 flex-shrink-0 mt-0.5" />
          <div className="text-sm text-gray-300">
            <p className="font-medium text-amber-400 mb-1">What are Clarifications?</p>
            <p className="text-gray-400">
              When Polymarket updates a market's description or resolution rules, it may signal
              new information that could affect the outcome. These alerts help you spot potential
              trading opportunities from rule changes.
            </p>
          </div>
        </div>
      </div>

      {/* Clarification list */}
      {clarifications.length === 0 ? (
        <div className="text-center py-12 text-gray-500">
          <FileText className="w-12 h-12 mx-auto mb-3 opacity-50" />
          <p className="text-lg">No clarifications detected</p>
          <p className="text-sm">Market description changes will appear here</p>
        </div>
      ) : (
        <div className="grid gap-3">
          {clarifications.map((alert) => (
            <ClarificationCard key={alert.market_id} alert={alert} />
          ))}
        </div>
      )}
    </div>
  )
}
