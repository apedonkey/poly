import { useState } from 'react'
import { TrendingUp, LayoutDashboard, Briefcase, Zap, FileText, AlertTriangle } from 'lucide-react'
import { WalletConnect } from './components/wallet/WalletConnect'
import { OpportunityList } from './components/opportunities/OpportunityList'
import { PositionList } from './components/positions/PositionList'
import { AutoTradingPanel } from './components/auto-trading/AutoTradingPanel'
import { ClarificationsPanel } from './components/clarifications/ClarificationsPanel'
import { DisputesPanel } from './components/disputes/DisputesPanel'
import { FAQ } from './components/FAQ'
import { useWebSocket } from './hooks/useWebSocket'
import { useDiscordAlerts } from './hooks/useDiscordAlerts'
import { useOpportunityStore } from './stores/opportunityStore'
import { useClarificationStore } from './stores/clarificationStore'
import { useDisputeStore } from './stores/disputeStore'

type Tab = 'opportunities' | 'positions' | 'auto-trade' | 'clarifications' | 'disputes'

function App() {
  const [activeTab, setActiveTab] = useState<Tab>('opportunities')
  // Only subscribe to the count, not the entire array - prevents re-renders on every update
  const opportunityCount = useOpportunityStore((s) => s.opportunities.length)
  const clarificationCount = useClarificationStore((s) => s.clarifications.length)
  const disputeCount = useDisputeStore((s) => s.disputes.length)

  // Connect to WebSocket for real-time updates
  useWebSocket()

  // Send new sniper opportunities to Discord
  useDiscordAlerts()

  return (
    <div className="min-h-screen bg-poly-dark flex flex-col">
      {/* Header */}
      <header className="border-b border-poly-border bg-poly-card/50 backdrop-blur-sm sticky top-0 z-40 safe-area-inset-top">
        <div className="container mx-auto px-3 sm:px-4 py-2 sm:py-3">
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2 sm:gap-3 min-w-0">
              <TrendingUp className="w-6 h-6 sm:w-8 sm:h-8 text-poly-green flex-shrink-0" />
              <div className="min-w-0">
                <h1 className="text-base sm:text-xl font-bold truncate">Polymarket Bot</h1>
                <p className="text-xs text-gray-500 hidden sm:block">
                  {opportunityCount} opportunities found
                </p>
              </div>
            </div>
            <WalletConnect />
          </div>
        </div>
      </header>

      {/* Navigation */}
      <nav className="border-b border-poly-border bg-poly-card/30">
        <div className="container mx-auto px-3 sm:px-4">
          <div className="flex">
            <button
              onClick={() => setActiveTab('opportunities')}
              className={`flex items-center justify-center gap-1.5 sm:gap-2 px-3 sm:px-4 py-3 border-b-2 transition flex-1 sm:flex-none touch-target ${
                activeTab === 'opportunities'
                  ? 'border-poly-green text-poly-green'
                  : 'border-transparent text-gray-400 hover:text-white active:text-white'
              }`}
            >
              <LayoutDashboard className="w-4 h-4" />
              <span className="text-sm sm:text-base">Opportunities</span>
              <span className="sm:hidden text-xs opacity-70">({opportunityCount})</span>
            </button>
            <button
              onClick={() => setActiveTab('positions')}
              className={`flex items-center justify-center gap-1.5 sm:gap-2 px-3 sm:px-4 py-3 border-b-2 transition flex-1 sm:flex-none touch-target ${
                activeTab === 'positions'
                  ? 'border-poly-green text-poly-green'
                  : 'border-transparent text-gray-400 hover:text-white active:text-white'
              }`}
            >
              <Briefcase className="w-4 h-4" />
              <span className="text-sm sm:text-base">Positions</span>
            </button>
            <button
              onClick={() => setActiveTab('auto-trade')}
              className={`flex items-center justify-center gap-1.5 sm:gap-2 px-3 sm:px-4 py-3 border-b-2 transition flex-1 sm:flex-none touch-target ${
                activeTab === 'auto-trade'
                  ? 'border-poly-green text-poly-green'
                  : 'border-transparent text-gray-400 hover:text-white active:text-white'
              }`}
            >
              <Zap className="w-4 h-4" />
              <span className="text-sm sm:text-base">Auto-Trade</span>
            </button>
            <button
              onClick={() => setActiveTab('clarifications')}
              className={`flex items-center justify-center gap-1.5 sm:gap-2 px-3 sm:px-4 py-3 border-b-2 transition flex-1 sm:flex-none touch-target ${
                activeTab === 'clarifications'
                  ? 'border-amber-400 text-amber-400'
                  : 'border-transparent text-gray-400 hover:text-white active:text-white'
              }`}
            >
              <FileText className="w-4 h-4" />
              <span className="text-sm sm:text-base hidden sm:inline">Clarifications</span>
              <span className="text-sm sm:hidden">Clarify</span>
              {clarificationCount > 0 && (
                <span className="text-xs bg-amber-500/20 text-amber-400 px-1.5 py-0.5 rounded-full">
                  {clarificationCount}
                </span>
              )}
            </button>
            <button
              onClick={() => setActiveTab('disputes')}
              className={`flex items-center justify-center gap-1.5 sm:gap-2 px-3 sm:px-4 py-3 border-b-2 transition flex-1 sm:flex-none touch-target ${
                activeTab === 'disputes'
                  ? 'border-red-400 text-red-400'
                  : 'border-transparent text-gray-400 hover:text-white active:text-white'
              }`}
            >
              <AlertTriangle className="w-4 h-4" />
              <span className="text-sm sm:text-base">Disputes</span>
              {disputeCount > 0 && (
                <span className="text-xs bg-red-500/20 text-red-400 px-1.5 py-0.5 rounded-full">
                  {disputeCount}
                </span>
              )}
            </button>
          </div>
        </div>
      </nav>

      {/* Main Content */}
      <main className="container mx-auto px-3 sm:px-4 py-4 sm:py-6 flex-1">
        <FAQ />
        {activeTab === 'opportunities' && <OpportunityList />}
        {activeTab === 'positions' && <PositionList />}
        {activeTab === 'auto-trade' && <AutoTradingPanel />}
        {activeTab === 'clarifications' && <ClarificationsPanel />}
        {activeTab === 'disputes' && <DisputesPanel />}
      </main>

      {/* Footer */}
      <footer className="border-t border-poly-border py-3 sm:py-4 mt-auto safe-area-inset-bottom">
        <div className="container mx-auto px-3 sm:px-4 text-center text-xs sm:text-sm text-gray-500">
          <p>Slow and steady wins the race.</p>
        </div>
      </footer>
    </div>
  )
}

export default App
