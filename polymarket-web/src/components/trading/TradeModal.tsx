import { useState, useEffect } from 'react'
import { AlertTriangle, Check, DollarSign, Wallet, Shield, Loader2, Zap } from 'lucide-react'
import { useChainId, useSwitchChain } from 'wagmi'
import { Modal } from '../Modal'
import type { Opportunity } from '../../types'
import { useWalletStore } from '../../stores/walletStore'
import { executeTrade, recordPosition } from '../../api/client'
import { useClobClient } from '../../hooks/useClobClient'
import { useTradingBalance } from '../../hooks/useTradingBalance'
import { useActivation } from '../../hooks/useActivation'

interface Props {
  isOpen: boolean
  onClose: () => void
  opportunity: Opportunity
}

export function TradeModal({ isOpen, onClose, opportunity }: Props) {
  const { sessionToken, isConnected, isExternal, address } = useWalletStore()
  const chainId = useChainId()
  const { switchChain } = useSwitchChain()
  const {
    placeMarketOrder,
    checkAllowance,
    isInitializing,
    isPlacingOrder,
    isApproving,
    isCheckingAllowance,
    allowanceStatus,
    error: clobError,
    clearError: clearClobError
  } = useClobClient()
  const { balance: tradingBalance, refetch: refetchBalance } = useTradingBalance()
  const {
    activate,
    checkStatus: checkActivationStatus,
    isActivating,
    isChecking: isCheckingActivation,
    status: activationStatus,
    error: activationError,
  } = useActivation()

  const [sizeUsdc, setSizeUsdc] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState(false)

  // External wallets need to be on Polygon for live trading
  const needsNetworkSwitch = isExternal && chainId !== 137

  // Check allowance when modal opens for live trading
  useEffect(() => {
    if (isOpen && isExternal && !allowanceStatus) {
      checkAllowance()
    }
  }, [isOpen, isExternal, allowanceStatus, checkAllowance])

  // Check activation status when modal opens for live trading
  useEffect(() => {
    if (isOpen && isExternal && !activationStatus) {
      checkActivationStatus()
    }
  }, [isOpen, isExternal, activationStatus, checkActivationStatus])

  // Refetch balance when modal opens
  useEffect(() => {
    if (isOpen && isExternal) {
      refetchBalance()
    }
  }, [isOpen, isExternal, refetchBalance])

  // Refresh allowance check when activation status changes to fully activated
  useEffect(() => {
    if (activationStatus?.isDeployed && activationStatus?.hasAllowances) {
      // Activation complete, refresh the CLOB client's allowance status too
      checkAllowance()
    }
  }, [activationStatus?.isDeployed, activationStatus?.hasAllowances, checkAllowance])

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
    } catch {
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

    // For external wallets, check token_id
    if (isExternal && !opportunity.token_id) {
      setError('Token ID not available for this market.')
      return
    }

    // For external wallets, check allowance
    // Use activationStatus as primary check (updated after activation)
    // Fall back to allowanceStatus from CLOB client
    const hasAllowance = activationStatus?.hasAllowances || allowanceStatus?.hasAllowance
    if (isExternal && !hasAllowance) {
      setError('Please enable trading first by approving USDC spending.')
      return
    }

    // Check if user has sufficient balance
    if (isExternal && tradingBalance) {
      const balance = parseFloat(tradingBalance.usdcFormatted)
      if (amountNum > balance) {
        setError(`Insufficient balance. You have $${tradingBalance.usdcFormatted} USDC in your trading wallet.`)
        return
      }
    }

    // For generated wallets, check password
    if (!isExternal && !password) {
      setError('Password required for live trades')
      return
    }

    setLoading(true)
    setError(null)
    clearClobError()

    try {
      if (isExternal) {
        // External wallet live trade - use official Polymarket SDK
        if (needsNetworkSwitch) {
          setError('Please switch to Polygon network first')
          setLoading(false)
          return
        }

        // Determine the side for the SDK (buy YES or buy NO)
        // If opportunity.side is "Yes", we buy the YES token
        // If opportunity.side is "No", we buy the NO token
        const side = 'buy' as const

        // Place order using the official SDK
        const orderId = await placeMarketOrder({
          tokenId: opportunity.token_id!,
          side,
          size: amountNum,
          price,
        })

        if (!orderId) {
          // Error already set by hook
          if (clobError) {
            setError(clobError)
          }
          setLoading(false)
          return
        }

        // console.log('Order placed successfully:', orderId)

        // Query the order to get the actual fill price
        let actualEntryPrice = opportunity.entry_price
        try {
          // Wait a moment for the order to be processed
          await new Promise(resolve => setTimeout(resolve, 1000))

          // Query order details to get fill price
          const orderResponse = await fetch(
            `https://clob.polymarket.com/order/${orderId}`
          )
          if (orderResponse.ok) {
            const orderData = await orderResponse.json()
            // console.log('Order details:', orderData)

            // Get the average fill price from the order
            if (orderData.price) {
              actualEntryPrice = orderData.price
              // console.log('Using fill price from order:', actualEntryPrice)
            } else if (orderData.associate_trades && orderData.associate_trades.length > 0) {
              // Calculate average fill price from trades
              const trades = orderData.associate_trades
              const totalValue = trades.reduce((sum: number, t: any) => sum + parseFloat(t.price) * parseFloat(t.size), 0)
              const totalSize = trades.reduce((sum: number, t: any) => sum + parseFloat(t.size), 0)
              if (totalSize > 0) {
                actualEntryPrice = (totalValue / totalSize).toString()
                // console.log('Calculated average fill price:', actualEntryPrice)
              }
            }
          }
        } catch (priceErr) {
          console.warn('Could not fetch order details, using opportunity price:', priceErr)
        }

        // Record the position in our backend
        await recordPosition(sessionToken!, {
          market_id: opportunity.market_id,
          question: opportunity.question,
          slug: opportunity.slug,
          side: opportunity.side,
          size_usdc: sizeUsdc,
          entry_price: actualEntryPrice,
          token_id: opportunity.token_id!,
          order_id: orderId,
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
    clearClobError()
    onClose()
  }

  const isProcessing = loading || isInitializing || isPlacingOrder || isActivating
  const isCheckingStatus = isCheckingActivation || isCheckingAllowance
  const needsActivation = !!(address && isExternal && activationStatus !== null && !activationStatus.isDeployed)
  const needsApproval = !!(address && isExternal && activationStatus !== null && activationStatus.isDeployed && !activationStatus.hasAllowances)
  const displayError = error || clobError

  if (success) {
    return (
      <Modal isOpen={isOpen} onClose={handleClose} title="Trade Submitted">
        <div className="text-center py-6">
          <div className="w-16 h-16 bg-poly-green/20 rounded-full flex items-center justify-center mx-auto mb-4">
            <Check className="w-8 h-8 text-poly-green" />
          </div>
          <h3 className="text-xl font-semibold mb-2">Trade Submitted!</h3>
          <p className="text-gray-400">
            Your order has been submitted to Polymarket.
          </p>
        </div>
      </Modal>
    )
  }

  return (
    <Modal isOpen={isOpen} onClose={handleClose} title="Execute Trade">
      <div className="space-y-3 sm:space-y-4">
        <div className="bg-poly-dark rounded-lg p-3 border border-poly-border">
          <div className="text-xs sm:text-sm text-gray-400 mb-1">Market</div>
          <div className="font-medium text-sm sm:text-base line-clamp-2">{opportunity.question}</div>
        </div>

        <div className="grid grid-cols-2 gap-2 sm:gap-3">
          <div className="bg-poly-dark rounded-lg p-3 border border-poly-border text-center">
            <div className="text-xs sm:text-sm text-gray-400">Side</div>
            <div className={`text-lg sm:text-xl font-bold ${
              opportunity.side === 'Yes' ? 'text-poly-green' : 'text-poly-red'
            }`}>
              {opportunity.side}
            </div>
          </div>
          <div className="bg-poly-dark rounded-lg p-3 border border-poly-border text-center">
            <div className="text-xs sm:text-sm text-gray-400">Price</div>
            <div className="text-lg sm:text-xl font-bold">
              {(price * 100).toFixed(0)}c
            </div>
          </div>
        </div>

        {/* Market verification warning */}
        <div className="p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
          <div className="flex items-start gap-2">
            <AlertTriangle className="w-5 h-5 text-yellow-400 flex-shrink-0 mt-0.5" />
            <div className="flex-1">
              <p className="text-xs text-gray-400">
                {opportunity.time_to_close_hours !== null ? (
                  <>Estimated close: <span className="text-yellow-400">{opportunity.time_to_close_hours < 24
                    ? `${opportunity.time_to_close_hours.toFixed(1)} hours`
                    : `${(opportunity.time_to_close_hours / 24).toFixed(1)} days`}</span>. </>
                ) : null}
                Closing times may not be accurate. <a
                  href={`https://polymarket.com/event/${opportunity.slug}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-blue-400 hover:text-blue-300 underline"
                >Verify on Polymarket</a>
              </p>
            </div>
          </div>
        </div>


        {/* Trading wallet balance for external wallets */}
        {isExternal && tradingBalance && (
          <div className="p-3 bg-poly-dark rounded-lg border border-poly-border">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Wallet className="w-4 h-4 text-poly-green" />
                <span className="text-sm text-gray-400">Trading Wallet</span>
              </div>
              <span className="font-semibold text-poly-green">
                ${tradingBalance.usdcFormatted} USDC
              </span>
            </div>
            <p className="text-xs text-gray-500 mt-1">
              {tradingBalance.proxyAddress.slice(0, 10)}...{tradingBalance.proxyAddress.slice(-8)}
            </p>
          </div>
        )}

        {/* Wallet activation for external wallets */}
        {address && isExternal && !needsNetworkSwitch && (
          <>
            {isCheckingActivation && (
              <div className="p-3 bg-poly-dark rounded-lg border border-poly-border flex items-center gap-2">
                <Loader2 className="w-4 h-4 animate-spin text-gray-400" />
                <span className="text-sm text-gray-400">Checking wallet activation...</span>
              </div>
            )}
            {activationStatus && !activationStatus.isDeployed && !isActivating && (
              <div className="p-3 bg-purple-500/10 border border-purple-500/30 rounded-lg">
                <div className="flex items-start gap-2">
                  <Zap className="w-5 h-5 text-purple-400 flex-shrink-0 mt-0.5" />
                  <div className="flex-1">
                    <p className="text-sm text-purple-400 font-medium">Wallet Not Activated</p>
                    <p className="text-xs text-gray-400 mt-1">
                      Your trading wallet needs to be activated before you can trade. This is a one-time setup.
                    </p>
                    <button
                      onClick={activate}
                      className="mt-2 text-sm bg-purple-500/20 text-purple-400 px-4 py-1.5 rounded hover:bg-purple-500/30 transition font-medium"
                    >
                      Activate Wallet
                    </button>
                  </div>
                </div>
              </div>
            )}
            {isActivating && (
              <div className="p-3 bg-poly-dark rounded-lg border border-poly-border flex items-center gap-2">
                <Loader2 className="w-4 h-4 animate-spin text-purple-400" />
                <span className="text-sm text-gray-400">Activating wallet... Please sign when prompted</span>
              </div>
            )}
            {activationError && (
              <div className="text-sm text-poly-red bg-poly-red/10 px-3 py-2 rounded">
                Activation error: {activationError}
              </div>
            )}
          </>
        )}

        {/* Allowance check / Enable trading for external wallets */}
        {address && isExternal && !needsNetworkSwitch && activationStatus?.isDeployed && !activationStatus?.hasAllowances && (
          <>
            {!isActivating && (
              <div className="p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
                <div className="flex items-start gap-2">
                  <Shield className="w-5 h-5 text-yellow-400 flex-shrink-0 mt-0.5" />
                  <div className="flex-1">
                    <p className="text-sm text-yellow-400 font-medium">Trading Not Enabled</p>
                    <p className="text-xs text-gray-400 mt-1">
                      You need to approve token spending before you can trade. This is a one-time setup.
                    </p>
                    <button
                      onClick={activate}
                      className="mt-2 text-sm bg-yellow-500/20 text-yellow-400 px-4 py-1.5 rounded hover:bg-yellow-500/30 transition font-medium"
                    >
                      Enable Trading
                    </button>
                  </div>
                </div>
              </div>
            )}
            {isActivating && (
              <div className="p-3 bg-poly-dark rounded-lg border border-poly-border flex items-center gap-2">
                <Loader2 className="w-4 h-4 animate-spin text-poly-green" />
                <span className="text-sm text-gray-400">Setting approvals... Please sign when prompted</span>
              </div>
            )}
          </>
        )}

        {/* Trading enabled success message */}
        {address && isExternal && activationStatus?.isDeployed && activationStatus?.hasAllowances && (
          <div className="p-3 bg-poly-green/10 border border-poly-green/30 rounded-lg flex items-center gap-2">
            <Check className="w-4 h-4 text-poly-green" />
            <span className="text-sm text-poly-green">Trading enabled</span>
          </div>
        )}

        {/* Network switch warning for external wallets */}
        {isExternal && needsNetworkSwitch && (
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
          <label className="block text-xs sm:text-sm text-gray-400 mb-1.5">
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
              inputMode="decimal"
              className="w-full pl-9 pr-3 py-3 sm:py-2 bg-poly-dark border border-poly-border rounded-lg focus:outline-none focus:border-poly-green text-base"
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
              <span className="text-gray-400">Profit if wins</span>
              <span className="text-poly-green">+${potentialProfit.toFixed(2)}</span>
            </div>
          </div>
        )}

        {/* Password field for generated wallets */}
        {!isExternal && (
          <div>
            <label className="block text-xs sm:text-sm text-gray-400 mb-1.5">
              Wallet Password
            </label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter password"
              className="w-full px-3 py-3 sm:py-2 bg-poly-dark border border-poly-border rounded-lg focus:outline-none focus:border-poly-green text-base"
            />
            <div className="flex items-start gap-2 mt-2 text-xs text-yellow-400">
              <AlertTriangle className="w-4 h-4 flex-shrink-0 mt-0.5" />
              <span>Password decrypts your wallet for this trade.</span>
            </div>
          </div>
        )}

        {displayError && (
          <div className="text-poly-red text-sm bg-poly-red/10 px-3 py-2 rounded">
            {displayError}
          </div>
        )}

        <div className="flex gap-2 pt-2">
          <button
            onClick={handleClose}
            className="flex-1 py-3 sm:py-2 border border-poly-border rounded-lg hover:bg-poly-dark active:bg-poly-dark transition touch-target font-medium"
          >
            Cancel
          </button>
          <button
            onClick={handleTrade}
            disabled={isProcessing || isCheckingStatus || !isConnected() || (isExternal && needsNetworkSwitch) || needsActivation || needsApproval}
            className={`flex-1 py-3 sm:py-2 font-semibold rounded-lg transition touch-target disabled:opacity-50 active:scale-[0.98] ${
              opportunity.side === 'Yes'
                ? 'bg-poly-green text-black hover:bg-poly-green/90 active:bg-poly-green/80'
                : 'bg-poly-red text-white hover:bg-poly-red/90 active:bg-poly-red/80'
            }`}
          >
            {isProcessing
              ? (isInitializing ? 'Connecting...' : isPlacingOrder ? 'Placing...' : isApproving ? 'Approving...' : isActivating ? 'Activating...' : 'Processing...')
              : isCheckingStatus
                ? 'Checking...'
                : needsActivation
                  ? 'Activate First'
                  : needsApproval
                    ? 'Enable Trading'
                    : `Buy ${opportunity.side}`}
          </button>
        </div>
      </div>
    </Modal>
  )
}
