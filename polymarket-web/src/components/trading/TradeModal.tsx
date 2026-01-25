import { useState, useEffect } from 'react'
import { AlertTriangle, Check, DollarSign, Wallet, Shield } from 'lucide-react'
import { useChainId, useSwitchChain } from 'wagmi'
import { Modal } from '../Modal'
import type { Opportunity } from '../../types'
import { useWalletStore } from '../../stores/walletStore'
import { executeTrade, executePaperTrade, executeSignedTrade } from '../../api/client'
import { usePolymarketSigning } from '../../hooks/usePolymarketSigning'
import { usePolymarketAuth } from '../../hooks/usePolymarketAuth'

interface Props {
  isOpen: boolean
  onClose: () => void
  opportunity: Opportunity
}

export function TradeModal({ isOpen, onClose, opportunity }: Props) {
  const { sessionToken, isConnected, isExternal } = useWalletStore()
  const chainId = useChainId()
  const { switchChain } = useSwitchChain()
  const { createAndSignOrder, isLoading: signingLoading, error: signingError, clearError } = usePolymarketSigning()
  const { authenticate, isAuthenticating, hasCredentials, error: authError, clearError: clearAuthError } = usePolymarketAuth()

  const [sizeUsdc, setSizeUsdc] = useState('')
  const [password, setPassword] = useState('')
  const [isPaperTrade, setIsPaperTrade] = useState(true)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState(false)

  // External wallets need to be on Polygon for live trading
  const needsNetworkSwitch = isExternal && chainId !== 137

  // Reset error when signing or auth error changes
  useEffect(() => {
    if (signingError) {
      setError(signingError)
    }
    if (authError) {
      setError(authError)
    }
  }, [signingError, authError])

  const price = parseFloat(opportunity.entry_price)
  const amountNum = parseFloat(sizeUsdc) || 0
  const shares = amountNum / price
  const potentialProfit = shares - amountNum

  // Calculate end_date from time_to_close_hours
  const getEndDate = (): string | undefined => {
    if (!opportunity.time_to_close_hours) return undefined
    const endMs = Date.now() + opportunity.time_to_close_hours * 60 * 60 * 1000
    return new Date(endMs).toISOString()
  }

  const handleSwitchNetwork = async () => {
    try {
      await switchChain({ chainId: 137 })
    } catch (err) {
      setError('Failed to switch network. Please switch to Polygon manually.')
    }
  }

  const handleTrade = async () => {
    if (!isConnected()) {
      setError('Please connect your wallet first')
      return
    }

    if (!sizeUsdc || amountNum <= 0) {
      setError('Please enter a valid amount')
      return
    }

    // For live trades with external wallets, check token_id
    if (!isPaperTrade && isExternal && !opportunity.token_id) {
      setError('Token ID not available for this market. Please try paper trading.')
      return
    }

    // For live trades with generated wallets, check password
    if (!isPaperTrade && !isExternal && !password) {
      setError('Password required for live trades')
      return
    }

    setLoading(true)
    setError(null)
    clearError()
    clearAuthError()

    try {
      if (isPaperTrade) {
        // Paper trade - no signing needed
        await executePaperTrade(sessionToken!, {
          market_id: opportunity.market_id,
          side: opportunity.side,
          size_usdc: sizeUsdc,
        })
      } else if (isExternal) {
        // External wallet live trade - use client-side signing
        if (needsNetworkSwitch) {
          setError('Please switch to Polygon network first')
          setLoading(false)
          return
        }

        // Step 1: Authenticate with Polymarket if not already done
        if (!hasCredentials) {
          const authSuccess = await authenticate()
          if (!authSuccess) {
            // Error already set by hook
            setLoading(false)
            return
          }
        }

        // Step 2: Sign the order with wagmi
        const signedOrder = await createAndSignOrder({
          tokenId: opportunity.token_id!,
          side: opportunity.side,
          sizeUsdc,
          price: opportunity.entry_price,
        })

        if (!signedOrder) {
          // Error already set by hook
          setLoading(false)
          return
        }

        // Step 3: Submit signed order to backend (which proxies to Polymarket CLOB)
        await executeSignedTrade(sessionToken!, {
          market_id: opportunity.market_id,
          question: opportunity.question,
          side: opportunity.side,
          size_usdc: sizeUsdc,
          entry_price: opportunity.entry_price,
          token_id: opportunity.token_id!,
          signed_order: signedOrder,
          end_date: getEndDate(),
        })
      } else {
        // Generated wallet live trade - backend handles signing
        await executeTrade(sessionToken!, {
          market_id: opportunity.market_id,
          side: opportunity.side,
          size_usdc: sizeUsdc,
          password,
        })
      }

      setSuccess(true)
      setTimeout(() => {
        onClose()
        setSuccess(false)
        setSizeUsdc('')
        setPassword('')
      }, 2000)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Trade failed')
    } finally {
      setLoading(false)
    }
  }

  const handleClose = () => {
    setError(null)
    setSuccess(false)
    setSizeUsdc('')
    setPassword('')
    clearError()
    clearAuthError()
    onClose()
  }

  if (success) {
    return (
      <Modal isOpen={isOpen} onClose={handleClose} title="Trade Submitted">
        <div className="text-center py-6">
          <div className="w-16 h-16 bg-poly-green/20 rounded-full flex items-center justify-center mx-auto mb-4">
            <Check className="w-8 h-8 text-poly-green" />
          </div>
          <h3 className="text-xl font-semibold mb-2">
            {isPaperTrade ? 'Paper Trade' : 'Trade'} Submitted!
          </h3>
          <p className="text-gray-400">
            {isPaperTrade
              ? 'Your paper trade has been recorded.'
              : 'Your order has been submitted to Polymarket.'}
          </p>
        </div>
      </Modal>
    )
  }

  return (
    <Modal isOpen={isOpen} onClose={handleClose} title="Execute Trade">
      <div className="space-y-4">
        <div className="bg-poly-dark rounded-lg p-3 border border-poly-border">
          <div className="text-sm text-gray-400 mb-1">Market</div>
          <div className="font-medium line-clamp-2">{opportunity.question}</div>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="bg-poly-dark rounded-lg p-3 border border-poly-border">
            <div className="text-sm text-gray-400">Side</div>
            <div className={`text-xl font-bold ${
              opportunity.side === 'Yes' ? 'text-poly-green' : 'text-poly-red'
            }`}>
              {opportunity.side}
            </div>
          </div>
          <div className="bg-poly-dark rounded-lg p-3 border border-poly-border">
            <div className="text-sm text-gray-400">Price</div>
            <div className="text-xl font-bold">
              {(price * 100).toFixed(0)}c
            </div>
          </div>
        </div>

        <div className="p-3 bg-poly-dark rounded-lg border border-poly-border">
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={isPaperTrade}
              onChange={(e) => setIsPaperTrade(e.target.checked)}
              className="w-4 h-4 accent-poly-green"
            />
            <span className="text-sm">Paper Trade (no real money)</span>
          </label>
          {!isPaperTrade && isExternal && (
            <div className="mt-2 space-y-1">
              <p className="text-xs text-purple-400 flex items-center gap-1">
                <Shield className="w-3 h-3" />
                {hasCredentials
                  ? 'Authenticated with Polymarket'
                  : 'Will authenticate with Polymarket (1 signature)'}
              </p>
              <p className="text-xs text-purple-400 flex items-center gap-1">
                <Wallet className="w-3 h-3" />
                {hasCredentials
                  ? 'Then sign order (1 signature)'
                  : 'Then sign order (1 signature)'}
              </p>
            </div>
          )}
        </div>

        {/* Network switch warning for external wallets */}
        {!isPaperTrade && isExternal && needsNetworkSwitch && (
          <div className="p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
            <div className="flex items-start gap-2">
              <AlertTriangle className="w-5 h-5 text-yellow-400 flex-shrink-0 mt-0.5" />
              <div>
                <p className="text-sm text-yellow-400 font-medium">Wrong Network</p>
                <p className="text-xs text-gray-400 mt-1">
                  Please switch to Polygon network to execute live trades.
                </p>
                <button
                  onClick={handleSwitchNetwork}
                  className="mt-2 text-xs bg-yellow-500/20 text-yellow-400 px-3 py-1 rounded hover:bg-yellow-500/30 transition"
                >
                  Switch to Polygon
                </button>
              </div>
            </div>
          </div>
        )}

        <div>
          <label className="block text-sm text-gray-400 mb-1">
            Amount (USDC)
          </label>
          <div className="relative">
            <DollarSign className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-500" />
            <input
              type="number"
              value={sizeUsdc}
              onChange={(e) => setSizeUsdc(e.target.value)}
              placeholder="0.00"
              min="0"
              step="0.01"
              className="w-full pl-9 pr-3 py-2 bg-poly-dark border border-poly-border rounded focus:outline-none focus:border-poly-green"
            />
          </div>
        </div>

        {amountNum > 0 && (
          <div className="bg-poly-dark rounded-lg p-3 border border-poly-border text-sm">
            <div className="flex justify-between mb-1">
              <span className="text-gray-400">Shares</span>
              <span>{shares.toFixed(2)}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-gray-400">Potential profit if wins</span>
              <span className="text-poly-green">+${potentialProfit.toFixed(2)}</span>
            </div>
          </div>
        )}

        {/* Password field for generated wallets */}
        {!isPaperTrade && !isExternal && (
          <div>
            <label className="block text-sm text-gray-400 mb-1">
              Wallet Password (required for live trades)
            </label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter your wallet password"
              className="w-full px-3 py-2 bg-poly-dark border border-poly-border rounded focus:outline-none focus:border-poly-green"
            />
            <div className="flex items-start gap-2 mt-2 text-xs text-yellow-400">
              <AlertTriangle className="w-4 h-4 flex-shrink-0 mt-0.5" />
              <span>Your password is used to decrypt your wallet for this trade.</span>
            </div>
          </div>
        )}

        {error && (
          <div className="text-poly-red text-sm bg-poly-red/10 px-3 py-2 rounded">
            {error}
          </div>
        )}

        <div className="flex gap-2">
          <button
            onClick={handleClose}
            className="flex-1 py-2 border border-poly-border rounded hover:bg-poly-dark transition"
          >
            Cancel
          </button>
          <button
            onClick={handleTrade}
            disabled={loading || signingLoading || isAuthenticating || !isConnected() || (!isPaperTrade && isExternal && needsNetworkSwitch)}
            className={`flex-1 py-2 font-semibold rounded transition disabled:opacity-50 ${
              opportunity.side === 'Yes'
                ? 'bg-poly-green text-black hover:bg-poly-green/90'
                : 'bg-poly-red text-white hover:bg-poly-red/90'
            }`}
          >
            {loading || signingLoading || isAuthenticating
              ? (isAuthenticating ? 'Authenticating...' : signingLoading ? 'Sign Order...' : 'Processing...')
              : `Buy ${opportunity.side}`}
          </button>
        </div>
      </div>
    </Modal>
  )
}
