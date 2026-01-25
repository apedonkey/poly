import { useQuery } from '@tanstack/react-query'
import { getPositions, getStats } from '../../api/client'
import { useWalletStore } from '../../stores/walletStore'
import { PositionCard } from './PositionCard'
import { StatsCard } from './StatsCard'
import { Wallet } from 'lucide-react'

export function PositionList() {
  const { sessionToken, isConnected } = useWalletStore()

  const { data: positions, isLoading: positionsLoading } = useQuery({
    queryKey: ['positions', sessionToken],
    queryFn: () => getPositions(sessionToken!),
    enabled: isConnected(),
    refetchInterval: 30000,
  })

  const { data: stats, isLoading: statsLoading } = useQuery({
    queryKey: ['stats', sessionToken],
    queryFn: () => getStats(sessionToken!),
    enabled: isConnected(),
    refetchInterval: 30000,
  })

  if (!isConnected()) {
    return (
      <div className="text-center py-12">
        <Wallet className="w-12 h-12 text-gray-600 mx-auto mb-4" />
        <h3 className="text-lg font-semibold mb-2">Connect Your Wallet</h3>
        <p className="text-gray-400">Connect your wallet to view your positions and trading history.</p>
      </div>
    )
  }

  if (positionsLoading || statsLoading) {
    return (
      <div className="text-center py-12 text-gray-400">
        Loading positions...
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {stats && <StatsCard stats={stats} />}

      <div>
        <h2 className="text-lg font-semibold mb-4">Your Positions</h2>
        {!positions || positions.length === 0 ? (
          <div className="text-center py-8 text-gray-500 bg-poly-card rounded-xl border border-poly-border">
            <p>No positions yet.</p>
            <p className="text-sm">Start trading to see your positions here.</p>
          </div>
        ) : (
          <div className="grid gap-4 md:grid-cols-2">
            {positions.map((position) => (
              <PositionCard key={position.id} position={position} />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
