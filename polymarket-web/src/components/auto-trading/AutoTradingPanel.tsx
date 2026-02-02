import { useState, useEffect } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Zap, Wallet, Lock, X, ShieldCheck } from 'lucide-react'
import { useWalletStore } from '../../stores/walletStore'
import {
  getAutoTradingSettings,
  updateAutoTradingSettings,
  enableAutoTrading,
  disableAutoTrading,
  getAutoTradingHistory,
  getAutoTradingStats,
  getAutoTradingStatus,
} from '../../api/client'
import { AutoBuySettings } from './AutoBuySettings'
import { AutoSellSettings } from './AutoSellSettings'
import { DisputeSniperSettings } from './DisputeSniperSettings'
import { AutoTradingStatsCard } from './AutoTradingStatsCard'
import { ActivityLog } from './ActivityLog'
import { MillionairesClub } from './MillionairesClub'
import type { AutoTradingSettings } from '../../types'

// Access password for Auto-Trade section
const ACCESS_PASSWORD = 'Workingsucks123!!'

export function AutoTradingPanel() {
  const { sessionToken, isConnected } = useWalletStore()
  const queryClient = useQueryClient()
  const [showPasswordModal, setShowPasswordModal] = useState(false)
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')

  // Access gate state
  const [accessGranted, setAccessGranted] = useState(false)
  const [accessPassword, setAccessPassword] = useState('')
  const [accessError, setAccessError] = useState('')

  // Fetch settings
  const { data: settingsData, isLoading } = useQuery({
    queryKey: ['auto-trading-settings', sessionToken],
    queryFn: () => getAutoTradingSettings(sessionToken!),
    enabled: isConnected(),
  })

  const settings = settingsData?.settings

  // Fetch history
  const { data: historyData } = useQuery({
    queryKey: ['auto-trading-history', sessionToken],
    queryFn: () => getAutoTradingHistory(sessionToken!),
    enabled: isConnected(),
    refetchInterval: 10000,
  })

  // Fetch stats
  const { data: statsData } = useQuery({
    queryKey: ['auto-trading-stats', sessionToken],
    queryFn: () => getAutoTradingStats(sessionToken!),
    enabled: isConnected(),
    refetchInterval: 30000,
  })

  // Fetch status (includes wallet balance - also updated in real-time via WebSocket)
  const { data: statusData } = useQuery({
    queryKey: ['auto-trading-status', sessionToken],
    queryFn: () => getAutoTradingStatus(sessionToken!),
    enabled: isConnected(),
    refetchInterval: 30000,
  })

  // Real-time wallet balance from WebSocket
  const [realtimeBalance, setRealtimeBalance] = useState<string | null>(null)
  const walletAddress = useWalletStore((s) => s.address)

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail.address?.toLowerCase() === walletAddress?.toLowerCase()) {
        setRealtimeBalance(detail.usdc_balance)
      }
    }
    window.addEventListener('wallet-balance', handler)
    return () => window.removeEventListener('wallet-balance', handler)
  }, [walletAddress])

  // Enable mutation (requires password)
  const enableMutation = useMutation({
    mutationFn: async (pwd: string) => {
      return enableAutoTrading(sessionToken!, pwd)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['auto-trading-settings'] })
      setShowPasswordModal(false)
      setPassword('')
      setError('')
    },
    onError: (err: Error) => {
      setError(err.message || 'Invalid password')
    },
  })

  // Disable mutation
  const disableMutation = useMutation({
    mutationFn: async () => {
      return disableAutoTrading(sessionToken!)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['auto-trading-settings'] })
    },
  })

  // Handle toggle click
  const handleToggle = () => {
    if (settings?.enabled) {
      disableMutation.mutate()
    } else {
      setShowPasswordModal(true)
    }
  }

  // Handle password submit
  const handlePasswordSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    enableMutation.mutate(password)
  }

  // Update settings mutation
  const updateMutation = useMutation({
    mutationFn: (newSettings: Partial<AutoTradingSettings>) =>
      updateAutoTradingSettings(sessionToken!, newSettings),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['auto-trading-settings'] })
    },
  })

  // Handle access password submit
  const handleAccessSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (accessPassword === ACCESS_PASSWORD) {
      setAccessGranted(true)
      setAccessError('')
    } else {
      setAccessError('Incorrect password')
    }
  }

  // Access gate - must enter password first
  if (!accessGranted) {
    return (
      <div className="flex items-center justify-center py-16">
        <div className="bg-poly-card border border-poly-border rounded-xl p-8 w-full max-w-md">
          <div className="flex items-center justify-center gap-3 mb-6">
            <ShieldCheck className="w-8 h-8 text-poly-green" />
            <h2 className="text-xl font-bold">Auto-Trade Access</h2>
          </div>

          <p className="text-gray-400 text-center mb-6">
            This feature requires authorization. Enter the access password to continue.
          </p>

          <form onSubmit={handleAccessSubmit}>
            <input
              type="password"
              value={accessPassword}
              onChange={(e) => setAccessPassword(e.target.value)}
              placeholder="Access password"
              className="w-full bg-gray-700 border border-gray-600 rounded-lg px-4 py-3 mb-3"
              autoFocus
            />

            {accessError && (
              <p className="text-poly-red text-sm mb-3">{accessError}</p>
            )}

            <button
              type="submit"
              disabled={!accessPassword}
              className="w-full bg-poly-green hover:bg-poly-green/90 disabled:bg-gray-600 disabled:cursor-not-allowed text-white font-semibold py-3 rounded-lg transition"
            >
              Unlock
            </button>
          </form>
        </div>
      </div>
    )
  }

  if (!isConnected()) {
    return (
      <div className="space-y-6">
        <div className="text-center py-12">
          <Wallet className="w-12 h-12 text-gray-600 mx-auto mb-4" />
          <h3 className="text-lg font-semibold mb-2">Connect Your Wallet</h3>
          <p className="text-gray-400">Connect your wallet to configure auto-trading.</p>
        </div>
        <MillionairesClub />
      </div>
    )
  }

  // Check if using external wallet (MetaMask) - auto-trading requires generated wallet
  const walletStore = useWalletStore.getState()
  if (walletStore.isExternal) {
    return (
      <div className="text-center py-12">
        <div className="w-16 h-16 bg-yellow-500/20 rounded-full flex items-center justify-center mx-auto mb-4">
          <Zap className="w-8 h-8 text-yellow-500" />
        </div>
        <h3 className="text-lg font-semibold mb-2">Generated Wallet Required</h3>
        <p className="text-gray-400 max-w-md mx-auto mb-4">
          Auto-trading requires a generated wallet so the bot can sign trades automatically.
          External wallets (MetaMask) can't be used for auto-trading because we don't have access to your private key.
        </p>
        <div className="bg-poly-card border border-poly-border rounded-lg p-4 max-w-md mx-auto">
          <p className="text-sm text-gray-300 mb-3">To use auto-trading:</p>
          <ol className="text-sm text-gray-400 text-left list-decimal list-inside space-y-1">
            <li>Disconnect your current wallet</li>
            <li>Click "Connect" and choose "Generate New Wallet"</li>
            <li>Set a password and save your wallet address</li>
            <li>Fund your new wallet with USDC</li>
          </ol>
        </div>
      </div>
    )
  }

  if (isLoading) {
    return <div className="text-center py-12 text-gray-400">Loading settings...</div>
  }

  return (
    <div className="space-y-6">
      {/* Header with master toggle */}
      <div className="bg-poly-card rounded-xl border border-poly-border p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <Zap className={`w-6 h-6 ${settings?.enabled ? 'text-poly-green' : 'text-gray-500'}`} />
            <div>
              <h2 className="text-lg font-semibold">Auto-Trading</h2>
              <p className="text-sm text-gray-400">
                {settings?.enabled ? 'Monitoring positions for auto-sell triggers' : 'Disabled'}
              </p>
            </div>
          </div>

          {/* Master toggle */}
          <button
            onClick={handleToggle}
            disabled={enableMutation.isPending || disableMutation.isPending}
            className={`relative w-14 h-8 rounded-full transition-colors ${
              settings?.enabled ? 'bg-poly-green' : 'bg-gray-600'
            }`}
          >
            <span
              className={`absolute top-1 left-1 w-6 h-6 bg-white rounded-full transition-transform ${
                settings?.enabled ? 'translate-x-6' : 'translate-x-0'
              }`}
            />
          </button>
        </div>

        {/* Wallet balance & status bar */}
        {statusData && (
          <div className="mt-3 pt-3 border-t border-poly-border grid grid-cols-2 md:grid-cols-4 gap-3">
            <div>
              <p className="text-xs text-gray-500">Wallet Balance</p>
              <p className="text-sm font-semibold text-white">
                <Wallet className="w-3.5 h-3.5 inline mr-1 text-poly-green" />
                ${realtimeBalance ?? statusData.wallet_balance}
              </p>
            </div>
            <div>
              <p className="text-xs text-gray-500">Total Exposure</p>
              <p className="text-sm font-semibold text-white">${statusData.total_exposure}</p>
            </div>
            <div>
              <p className="text-xs text-gray-500">Open Positions</p>
              <p className="text-sm font-semibold text-white">{statusData.open_positions}</p>
            </div>
            <div>
              <p className="text-xs text-gray-500">Daily PnL</p>
              <p className={`text-sm font-semibold ${
                parseFloat(statusData.daily_pnl) >= 0 ? 'text-poly-green' : 'text-poly-red'
              }`}>
                ${statusData.daily_pnl}
              </p>
            </div>
          </div>
        )}
      </div>

      {/* Password Modal */}
      {showPasswordModal && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-poly-card border border-poly-border rounded-xl p-6 w-full max-w-md mx-4">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2">
                <Lock className="w-5 h-5 text-poly-green" />
                <h3 className="text-lg font-semibold">Enable Auto-Trading</h3>
              </div>
              <button
                onClick={() => {
                  setShowPasswordModal(false)
                  setPassword('')
                  setError('')
                }}
                className="text-gray-400 hover:text-white"
              >
                <X className="w-5 h-5" />
              </button>
            </div>

            <p className="text-sm text-gray-400 mb-4">
              Enter your wallet password to enable auto-trading. This allows the bot to automatically
              sign and execute trades on your behalf.
            </p>

            <form onSubmit={handlePasswordSubmit}>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Wallet password"
                className="w-full bg-gray-700 border border-gray-600 rounded-lg px-4 py-3 mb-3"
                autoFocus
              />

              {error && (
                <p className="text-poly-red text-sm mb-3">{error}</p>
              )}

              <button
                type="submit"
                disabled={!password || enableMutation.isPending}
                className="w-full bg-poly-green hover:bg-poly-green/90 disabled:bg-gray-600 disabled:cursor-not-allowed text-white font-semibold py-3 rounded-lg transition"
              >
                {enableMutation.isPending ? 'Enabling...' : 'Enable Auto-Trading'}
              </button>
            </form>
          </div>
        </div>
      )}

      {/* Settings cards */}
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
        <AutoBuySettings
          settings={settings}
          onUpdate={(s) => updateMutation.mutate(s)}
          disabled={!settings?.enabled}
          isPending={updateMutation.isPending}
        />
        <AutoSellSettings
          settings={settings}
          onUpdate={(s) => updateMutation.mutate(s)}
          disabled={!settings?.enabled}
          isPending={updateMutation.isPending}
        />
        <DisputeSniperSettings
          settings={settings}
          onUpdate={(s) => updateMutation.mutate(s)}
          disabled={!settings?.enabled}
          isPending={updateMutation.isPending}
        />
        <AutoTradingStatsCard stats={statsData?.stats} />
      </div>

      {/* Activity log */}
      <ActivityLog history={historyData?.history || []} />

      {/* Millionaires Club — observation scanner */}
      <MillionairesClub />
    </div>
  )
}
