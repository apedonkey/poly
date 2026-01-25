import { useState } from 'react'
import { TrendingUp, LayoutDashboard, Briefcase } from 'lucide-react'
import { WalletConnect } from './components/wallet/WalletConnect'
import { OpportunityList } from './components/opportunities/OpportunityList'
import { PositionList } from './components/positions/PositionList'
import { FAQ } from './components/FAQ'
import { useWebSocket } from './hooks/useWebSocket'
import { useOpportunityStore } from './stores/opportunityStore'

type Tab = 'opportunities' | 'positions'

function App() {
  const [activeTab, setActiveTab] = useState<Tab>('opportunities')
  const opportunities = useOpportunityStore((s) => s.opportunities)

  // Connect to WebSocket for real-time updates
  useWebSocket()

  return (
    <div className="min-h-screen bg-poly-dark">
      {/* Header */}
      <header className="border-b border-poly-border bg-poly-card/50 backdrop-blur-sm sticky top-0 z-40">
        <div className="container mx-auto px-4 py-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <TrendingUp className="w-8 h-8 text-poly-green" />
              <div>
                <h1 className="text-xl font-bold">Polymarket Bot</h1>
                <p className="text-xs text-gray-500">
                  {opportunities.length} opportunities found
                </p>
              </div>
            </div>
            <WalletConnect />
          </div>
        </div>
      </header>

      {/* Navigation */}
      <nav className="border-b border-poly-border bg-poly-card/30">
        <div className="container mx-auto px-4">
          <div className="flex gap-1">
            <button
              onClick={() => setActiveTab('opportunities')}
              className={`flex items-center gap-2 px-4 py-3 border-b-2 transition ${
                activeTab === 'opportunities'
                  ? 'border-poly-green text-poly-green'
                  : 'border-transparent text-gray-400 hover:text-white'
              }`}
            >
              <LayoutDashboard className="w-4 h-4" />
              Opportunities
            </button>
            <button
              onClick={() => setActiveTab('positions')}
              className={`flex items-center gap-2 px-4 py-3 border-b-2 transition ${
                activeTab === 'positions'
                  ? 'border-poly-green text-poly-green'
                  : 'border-transparent text-gray-400 hover:text-white'
              }`}
            >
              <Briefcase className="w-4 h-4" />
              Positions
            </button>
          </div>
        </div>
      </nav>

      {/* Main Content */}
      <main className="container mx-auto px-4 py-6">
        <FAQ />
        {activeTab === 'opportunities' && <OpportunityList />}
        {activeTab === 'positions' && <PositionList />}
      </main>

      {/* Footer */}
      <footer className="border-t border-poly-border py-4 mt-8">
        <div className="container mx-auto px-4 text-center text-sm text-gray-500">
          <p>Paper trading enabled by default. Trade responsibly.</p>
        </div>
      </footer>
    </div>
  )
}

export default App
