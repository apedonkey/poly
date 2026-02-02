import { useState, useEffect, useRef } from 'react'
import { Check, Wallet, Loader2, AlertTriangle } from 'lucide-react'
import { useChainId, useSwitchChain } from 'wagmi'
import { keccak256, encodeAbiParameters, getCreate2Address } from 'viem'
import type { Address } from 'viem'
import { Modal } from '../Modal'
import type { Position } from '../../types'
import { useWalletStore } from '../../stores/walletStore'
import { useClobClient } from '../../hooks/useClobClient'
import { useTradingBalance } from '../../hooks/useTradingBalance'
import { useActivation } from '../../hooks/useActivation'
import { closePosition, updatePositionEntryPrice } from '../../api/client'
import { rpcCallWithRetry } from '../../utils/rpc'

// Safe Proxy Factory for deriving proxy wallet address
const SAFE_FACTORY = '0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b' as const
const SAFE_INIT_CODE_HASH = '0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf' as const
const CTF_CONTRACT = '0x4D97DCd97eC945f40cF65F87097ACe5EA0476045' as const

// Derive the Polymarket Safe proxy wallet address from an EOA
function deriveSafeWallet(eoaAddress: Address): Address {
  const salt = keccak256(encodeAbiParameters([{ type: 'address' }], [eoaAddress]))
  return getCreate2Address({
    from: SAFE_FACTORY,
    salt,
    bytecodeHash: SAFE_INIT_CODE_HASH,
  })
}

interface Props {
  isOpen: boolean
  onClose: () => void
  position: Position
  onSold?: () => void
}

export function SellModal({ isOpen, onClose, position, onSold }: Props) {
  const { sessionToken, isConnected, isExternal, address } = useWalletStore()
  const chainId = useChainId()
  const { switchChain } = useSwitchChain()
  const {
    placeMarketOrder,
    isInitializing,
    isPlacingOrder,
    error: clobError,
    clearError: clearClobError
  } = useClobClient()
  const { balance: tradingBalance } = useTradingBalance()
  const {
    checkStatus: checkActivationStatus,
    activate,
    isActivating,
    status: activationStatus,
  } = useActivation()

  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState(false)
  const [currentPrice, setCurrentPrice] = useState<number | null>(null)
  const [loadingPrice, setLoadingPrice] = useState(false)
  const [correctedEntryPrice, setCorrectedEntryPrice] = useState<number | null>(null)
  const [actualTokenBalance, setActualTokenBalance] = useState<number | null>(null)
  const entryPriceCorrected = useRef(false)

  // Partial sell state
  const [sellPercentage, setSellPercentage] = useState<25 | 50 | 100>(100)
  const [sellResult, setSellResult] = useState<{
    sharesToSell: number
    isPartial: boolean
    remainingShares?: number
    pnl?: string
  } | null>(null)

  // External wallets need to be on Polygon for live trading
  const needsNetworkSwitch = isExternal && chainId !== 137

  // Check activation status when modal opens
  useEffect(() => {
    if (isOpen && isExternal) {
      checkActivationStatus()
    }
  }, [isOpen, isExternal, checkActivationStatus])

  // Re-check status after activation completes
  useEffect(() => {
    if (activationStatus?.hasCtfApproval) {
      // CTF approval confirmed, no need to re-check
    } else if (activationStatus && !isActivating) {
      // After activation attempt, re-check in case it succeeded
      const timer = setTimeout(() => {
        checkActivationStatus()
      }, 2000)
      return () => clearTimeout(timer)
    }
  }, [isActivating, activationStatus, checkActivationStatus])

  // Flash indicator for live price changes
  const [priceFlash, setPriceFlash] = useState<'up' | 'down' | null>(null)

  // Fetch current market price and token balance when modal opens
  useEffect(() => {
    if (isOpen && position.token_id) {
      fetchCurrentPrice()
      fetchActualTokenBalance()
    }
  }, [isOpen, position.token_id])

  // Live price updates via WebSocket while modal is open
  useEffect(() => {
    if (!isOpen || !position.token_id) return

    const handlePriceUpdate = (event: CustomEvent<{ token_id: string; price: string }>) => {
      const { token_id, price } = event.detail
      if (token_id === position.token_id) {
        const newPrice = parseFloat(price)
        setCurrentPrice((prev) => {
          if (prev !== null) {
            if (newPrice > prev) setPriceFlash('up')
            else if (newPrice < prev) setPriceFlash('down')
            setTimeout(() => setPriceFlash(null), 500)
          }
          return newPrice
        })
      }
    }

    window.addEventListener('price-update', handlePriceUpdate as EventListener)
    return () => window.removeEventListener('price-update', handlePriceUpdate as EventListener)
  }, [isOpen, position.token_id])

  // Fetch actual token balance from the blockchain
  const fetchActualTokenBalance = async () => {
    if (!position.token_id) return

    try {
      // Get proxy wallet address from the walletStore
      const { address } = useWalletStore.getState()
      if (!address) return

      // Derive proxy wallet using viem
      const proxyAddress = deriveSafeWallet(address as Address)
      // console.log('Fetching token balance for proxy:', proxyAddress, 'tokenId:', position.token_id)

      // Encode balanceOf(address, uint256) call
      // Function selector for balanceOf(address,uint256) is 0x00fdd58e
      const tokenIdHex = BigInt(position.token_id).toString(16).padStart(64, '0')
      const addressHex = proxyAddress.slice(2).toLowerCase().padStart(64, '0')
      const data = `0x00fdd58e${addressHex}${tokenIdHex}`

      const result = await rpcCallWithRetry({
        jsonrpc: '2.0',
        id: 1,
        method: 'eth_call',
        params: [{ to: CTF_CONTRACT, data }, 'latest'],
      })
      // console.log('Token balance response:', result)

      if (result.result) {
        // Balance is in 6 decimals (same as USDC)
        const balanceRaw = BigInt(result.result)
        const balance = Number(balanceRaw) / 1e6
        // console.log('Actual token balance:', balance)
        setActualTokenBalance(balance)
      }
    } catch (err) {
      console.error('Failed to fetch token balance:', err)
    }
  }

  // Auto-fetch and correct entry price when modal opens
  useEffect(() => {
    if (isOpen && !entryPriceCorrected.current) {
      if (position.order_id) {
        fetchAndCorrectEntryPrice()
      } else if (position.token_id) {
        // No order_id, try to find entry price from trade history
        fetchEntryPriceFromTradeHistory()
      }
    }
  }, [isOpen, position.order_id, position.token_id])

  const fetchAndCorrectEntryPrice = async () => {
    if (!position.order_id) return

    // console.log('Fetching order details for order_id:', position.order_id)

    try {
      // Query the order from CLOB API to get actual fill price
      const response = await fetch(
        `https://clob.polymarket.com/order/${position.order_id}`
      )

      if (response.ok) {
        const orderData = await response.json()
        // console.log('Order data:', orderData)

        // The order should have price or filled_price
        const actualPrice = orderData.price || orderData.filled_price || orderData.avg_price
        if (actualPrice) {
          const actualPriceNum = parseFloat(actualPrice)
          const storedPriceNum = parseFloat(position.entry_price)

          // console.log('Price comparison:', { actual: actualPriceNum, stored: storedPriceNum })

          // If prices differ significantly (more than 0.1%), update
          if (Math.abs(actualPriceNum - storedPriceNum) > 0.001) {
            // console.log('Entry price mismatch, correcting:', storedPriceNum, '->', actualPriceNum)
            setCorrectedEntryPrice(actualPriceNum)

            // Update in backend if we have a session
            if (sessionToken) {
              try {
                await updatePositionEntryPrice(sessionToken, position.id, actualPrice.toString())
                // console.log('Entry price updated in backend')
              } catch (err) {
                console.warn('Failed to update entry price in backend:', err)
              }
            }
          }
          entryPriceCorrected.current = true
        }
      } else {
        // console.log('Failed to fetch order:', response.status)
      }
    } catch (err) {
      console.error('Failed to fetch order details:', err)
    }
  }

  // Fetch entry price from trade history when we don't have order_id
  const fetchEntryPriceFromTradeHistory = async () => {
    if (!position.token_id) return

    try {
      const { address } = useWalletStore.getState()
      if (!address) return

      const proxyAddress = deriveSafeWallet(address as Address)
      // console.log('Fetching trade history for proxy:', proxyAddress, 'tokenId:', position.token_id)

      // Query user's trades from CLOB API
      // The trades endpoint requires the maker address
      const response = await fetch(
        `https://clob.polymarket.com/trades?maker=${proxyAddress}&asset_id=${position.token_id}`
      )

      if (response.ok) {
        const trades = await response.json()
        // console.log('Trade history:', trades)

        // Find buy trades (side = 0 or "BUY")
        // Trades are usually ordered newest first, so we want the earliest buy
        const buyTrades = (Array.isArray(trades) ? trades : []).filter((t: any) =>
          t.side === 'BUY' || t.side === 0 || t.type === 'BUY'
        )

        if (buyTrades.length > 0) {
          // Calculate weighted average entry price from all buy trades
          let totalSize = 0
          let totalValue = 0

          for (const trade of buyTrades) {
            const price = parseFloat(trade.price || '0')
            const size = parseFloat(trade.size || trade.amount || '0')
            if (price > 0 && size > 0) {
              totalSize += size
              totalValue += price * size
            }
          }

          if (totalSize > 0) {
            const avgPrice = totalValue / totalSize
            const storedPriceNum = parseFloat(position.entry_price)

            // console.log('Calculated avg entry price from trades:', avgPrice, 'stored:', storedPriceNum)

            if (Math.abs(avgPrice - storedPriceNum) > 0.001) {
              // console.log('Entry price mismatch, correcting:', storedPriceNum, '->', avgPrice)
              setCorrectedEntryPrice(avgPrice)

              // Update in backend
              if (sessionToken) {
                try {
                  await updatePositionEntryPrice(sessionToken, position.id, avgPrice.toString())
                  // console.log('Entry price updated in backend')
                } catch (err) {
                  console.warn('Failed to update entry price in backend:', err)
                }
              }
            }
            entryPriceCorrected.current = true
          }
        } else {
          // console.log('No buy trades found in history')
        }
      } else {
        // console.log('Failed to fetch trade history:', response.status)

        // Fallback: try the gamma API for historical price
        await fetchEntryPriceFromGamma()
      }
    } catch (err) {
      console.error('Failed to fetch trade history:', err)
    }
  }

  // Fallback: try to get price info from Gamma API
  const fetchEntryPriceFromGamma = async () => {
    try {
      // Try to get market info which might have price history
      const response = await fetch(
        `https://gamma-api.polymarket.com/markets?condition_id=${position.market_id}`
      )

      if (response.ok) {
        // Gamma API response - could be used for price validation in the future
        await response.json()
      }
    } catch (err) {
      console.error('Failed to fetch from Gamma API:', err)
    }
  }

  const fetchCurrentPrice = async () => {
    if (!position.token_id) return

    setLoadingPrice(true)
    // console.log('Fetching price for token_id:', position.token_id)

    try {
      // Try the /midpoint endpoint - gives the mid price between best bid/ask
      const midpointResponse = await fetch(
        `https://clob.polymarket.com/midpoint?token_id=${position.token_id}`
      )

      if (midpointResponse.ok) {
        const midpointData = await midpointResponse.json()
        // console.log('CLOB midpoint response:', midpointData)

        if (midpointData.mid) {
          const price = parseFloat(midpointData.mid)
          // console.log('Midpoint price:', price)
          setCurrentPrice(price)
          return
        }
      }

      // Try the /price endpoint
      const priceResponse = await fetch(
        `https://clob.polymarket.com/price?token_id=${position.token_id}&side=sell`
      )

      if (priceResponse.ok) {
        const priceData = await priceResponse.json()
        // console.log('CLOB price response:', priceData)

        if (priceData.price) {
          const price = parseFloat(priceData.price)
          // console.log('Current sell price:', price)
          setCurrentPrice(price)
          return
        }
      }

      // Fallback to /book endpoint
      // console.log('Falling back to /book endpoint...')
      const bookResponse = await fetch(
        `https://clob.polymarket.com/book?token_id=${position.token_id}`
      )
      const bookData = await bookResponse.json()
      // console.log('CLOB book response:', bookData)

      // Best bid price (what we'd sell at) - bids are sorted highest first
      if (bookData.bids && bookData.bids.length > 0) {
        const bestBid = parseFloat(bookData.bids[0].price)
        // console.log('Best bid price:', bestBid)
        setCurrentPrice(bestBid)
      } else if (bookData.asks && bookData.asks.length > 0) {
        // If no bids, use lowest ask as reference
        const lowestAsk = parseFloat(bookData.asks[0].price)
        // console.log('No bids, using ask as reference:', lowestAsk)
        setCurrentPrice(lowestAsk * 0.95)
      } else {
        // console.log('No bids or asks in orderbook')
        setCurrentPrice(null)
      }
    } catch (err) {
      console.error('Failed to fetch current price:', err)
    } finally {
      setLoadingPrice(false)
    }
  }

  // Use corrected entry price if available, otherwise fall back to stored
  const entryPrice = correctedEntryPrice ?? parseFloat(position.entry_price)
  const size = parseFloat(position.size) // Total USDC invested
  const calculatedShares = size / entryPrice // Estimated shares (may be inaccurate)

  // Get remaining shares from position (for partial sells tracking)
  // Priority: actualTokenBalance (on-chain) > position.remaining_size (DB) > calculatedShares
  const positionRemainingShares = position.remaining_size
    ? parseFloat(position.remaining_size)
    : null

  // Use actual token balance if available, then position's remaining_size, then calculated
  const totalRemainingShares = actualTokenBalance ?? positionRemainingShares ?? calculatedShares

  // Calculate shares to sell based on selected percentage
  const sharesToSell = (totalRemainingShares * sellPercentage) / 100
  const isPartialSell = sellPercentage < 100

  // Calculate potential PnL based on current price for the shares being sold
  // PnL = (exit_price - entry_price) * shares_to_sell
  const potentialPnl = currentPrice !== null
    ? (currentPrice - entryPrice) * sharesToSell
    : null

  // For backward compat, 'shares' refers to total remaining
  const shares = totalRemainingShares

  // console.log('Position calc:', { entryPrice, size, calculatedShares, actualTokenBalance, shares, currentPrice, potentialPnl })

  const handleSwitchNetwork = async () => {
    try {
      await switchChain({ chainId: 137 })
    } catch {
      setError('Failed to switch network. Please switch to Polygon manually.')
    }
  }

  const handleSell = async () => {
    if (!isConnected()) {
      setError('Please connect your wallet first')
      return
    }

    if (!position.token_id) {
      setError('Token ID not available for this position')
      return
    }

    if (needsNetworkSwitch) {
      setError('Please switch to Polygon network first')
      return
    }

    if (sharesToSell <= 0) {
      setError('Invalid share amount')
      return
    }

    setLoading(true)
    setError(null)
    clearClobError()

    try {
      // Place a sell order using the CLOB client for the selected amount
      const orderId = await placeMarketOrder({
        tokenId: position.token_id,
        side: 'sell',
        size: sharesToSell, // Sell selected percentage of shares
        price: currentPrice || entryPrice * 0.9, // Use current price or 90% of entry as fallback
      })

      if (!orderId) {
        if (clobError) {
          setError(clobError)
        } else {
          setError('Failed to place sell order')
        }
        setLoading(false)
        return
      }

      // console.log('Sell order placed successfully:', orderId, 'shares:', sharesToSell)

      // Update position in backend (mark as closed or partially closed)
      let closeResult: { pnl?: string; remaining_shares?: string; is_fully_closed?: boolean } = {}
      if (sessionToken) {
        try {
          closeResult = await closePosition(
            sessionToken,
            position.id,
            currentPrice?.toString() || position.entry_price,
            orderId,
            isPartialSell ? sharesToSell.toString() : undefined // Pass sellShares only for partial sells
          )
        } catch (err) {
          console.warn('Failed to update position status:', err)
          // Don't fail the whole operation if backend update fails
        }
      }

      // Store result for success message
      setSellResult({
        sharesToSell,
        isPartial: isPartialSell,
        remainingShares: closeResult.remaining_shares ? parseFloat(closeResult.remaining_shares) : undefined,
        pnl: closeResult.pnl,
      })

      setSuccess(true)
      setTimeout(() => {
        onClose()
        setSuccess(false)
        setSellResult(null)
        setSellPercentage(100) // Reset for next time
        onSold?.()
      }, 2500)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Sell failed')
    } finally {
      setLoading(false)
    }
  }

  const handleClose = () => {
    setError(null)
    setSuccess(false)
    setCorrectedEntryPrice(null)
    setActualTokenBalance(null)
    setSellPercentage(100) // Reset to Max
    setSellResult(null)
    setPriceFlash(null)
    entryPriceCorrected.current = false
    clearClobError()
    onClose()
  }

  const isProcessing = loading || isInitializing || isPlacingOrder || isActivating
  const displayError = error || clobError

  if (success) {
    const isPartial = sellResult?.isPartial
    return (
      <Modal isOpen={isOpen} onClose={handleClose} title={isPartial ? "Partial Sell Complete" : "Position Sold"}>
        <div className="text-center py-6">
          <div className="w-16 h-16 bg-poly-green/20 rounded-full flex items-center justify-center mx-auto mb-4">
            <Check className="w-8 h-8 text-poly-green" />
          </div>
          <h3 className="text-xl font-semibold mb-2">
            {isPartial ? `Sold ${sellPercentage}% of Position` : 'Position Sold!'}
          </h3>
          {sellResult?.pnl && (
            <p className={`text-lg font-medium mb-2 ${
              parseFloat(sellResult.pnl) >= 0 ? 'text-poly-green' : 'text-poly-red'
            }`}>
              {parseFloat(sellResult.pnl) >= 0 ? '+' : ''}${parseFloat(sellResult.pnl).toFixed(2)} USDC
            </p>
          )}
          <p className="text-gray-400">
            {isPartial && sellResult?.remainingShares !== undefined
              ? `${sellResult.remainingShares.toFixed(2)} shares remaining`
              : 'Your sell order has been submitted to Polymarket.'
            }
          </p>
        </div>
      </Modal>
    )
  }

  return (
    <Modal isOpen={isOpen} onClose={handleClose} title="Sell Position">
      <div className="space-y-3 sm:space-y-4">
        {/* Position info */}
        <div className="bg-poly-dark rounded-lg p-3 border border-poly-border">
          <div className="text-xs sm:text-sm text-gray-400 mb-1">Market</div>
          <div className="font-medium text-sm sm:text-base line-clamp-2">{position.question}</div>
        </div>

        <div className="grid grid-cols-3 gap-2 sm:gap-3">
          <div className="bg-poly-dark rounded-lg p-2.5 sm:p-3 border border-poly-border text-center">
            <div className="text-xs sm:text-sm text-gray-400">Side</div>
            <div className={`text-lg sm:text-xl font-bold ${
              position.side === 'Yes' ? 'text-poly-green' : 'text-poly-red'
            }`}>
              {position.side}
            </div>
          </div>
          <div className="bg-poly-dark rounded-lg p-2.5 sm:p-3 border border-poly-border text-center">
            <div className="text-xs sm:text-sm text-gray-400">Entry</div>
            <div className="text-lg sm:text-xl font-bold">
              {(entryPrice * 100).toFixed(0)}c
            </div>
          </div>
          <div className="bg-poly-dark rounded-lg p-2.5 sm:p-3 border border-poly-border text-center">
            <div className="text-xs sm:text-sm text-gray-400">
              Shares
            </div>
            <div className="text-lg sm:text-xl font-bold">
              {shares.toFixed(2)}
            </div>
          </div>
        </div>

        {/* Percentage selector - styled like Polymarket */}
        <div className="bg-poly-dark rounded-lg p-3 border border-poly-border">
          <div className="text-xs sm:text-sm text-gray-400 mb-2">Sell Amount</div>
          <div className="flex gap-2">
            {([25, 50, 100] as const).map((pct) => (
              <button
                key={pct}
                onClick={() => setSellPercentage(pct)}
                className={`flex-1 py-2.5 sm:py-2 px-2 rounded-lg font-medium text-sm transition ${
                  sellPercentage === pct
                    ? 'bg-poly-red text-white'
                    : 'bg-poly-card border border-poly-border text-gray-300 hover:bg-poly-border active:bg-poly-border'
                }`}
              >
                {pct === 100 ? 'Max' : `${pct}%`}
              </button>
            ))}
          </div>
          {/* Show shares to sell */}
          <div className="mt-2 text-center">
            <span className="text-sm text-gray-400">Selling </span>
            <span className="text-sm font-semibold text-white">{sharesToSell.toFixed(2)}</span>
            <span className="text-sm text-gray-400"> of {shares.toFixed(2)} shares</span>
          </div>
        </div>

        {/* Current price and PnL - Combined on mobile */}
        <div className="grid grid-cols-2 gap-2 sm:gap-3">
          <div className="bg-poly-dark rounded-lg p-2.5 sm:p-3 border border-poly-border text-center">
            <div className="text-xs sm:text-sm text-gray-400 mb-1">Current</div>
            {loadingPrice ? (
              <Loader2 className="w-4 h-4 animate-spin text-gray-400 mx-auto" />
            ) : currentPrice !== null ? (
              <span className={`text-lg sm:text-xl font-bold transition-colors duration-500 ${
                priceFlash === 'up' ? 'text-green-400' : priceFlash === 'down' ? 'text-red-400' : ''
              }`}>{(currentPrice * 100).toFixed(0)}c</span>
            ) : (
              <span className="text-gray-500">N/A</span>
            )}
          </div>
          <div className="bg-poly-dark rounded-lg p-2.5 sm:p-3 border border-poly-border text-center">
            <div className="text-xs sm:text-sm text-gray-400 mb-1">Est. P&L</div>
            {potentialPnl !== null ? (
              <span className={`text-lg sm:text-xl font-bold ${
                potentialPnl >= 0 ? 'text-poly-green' : 'text-poly-red'
              }`}>
                {potentialPnl >= 0 ? '+' : ''}${potentialPnl.toFixed(2)}
              </span>
            ) : (
              <span className="text-gray-500">-</span>
            )}
          </div>
        </div>

        {/* Trading wallet balance */}
        {isExternal && tradingBalance && (
          <div className="p-2.5 sm:p-3 bg-poly-dark rounded-lg border border-poly-border">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1.5 sm:gap-2">
                <Wallet className="w-4 h-4 text-poly-green flex-shrink-0" />
                <span className="text-xs sm:text-sm text-gray-400">Trading</span>
              </div>
              <span className="font-semibold text-poly-green text-sm sm:text-base">
                ${tradingBalance.usdcFormatted}
              </span>
            </div>
          </div>
        )}

        {/* CTF Approval check for selling */}
        {address && isExternal && !needsNetworkSwitch && activationStatus && !activationStatus.hasCtfApproval && (
          <div className="p-2.5 sm:p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
            <div className="flex items-start gap-2">
              <AlertTriangle className="w-4 h-4 sm:w-5 sm:h-5 text-yellow-400 flex-shrink-0 mt-0.5" />
              <div className="flex-1">
                <p className="text-xs sm:text-sm text-yellow-400 font-medium">Selling Not Enabled</p>
                <p className="text-xs text-gray-400 mt-1">
                  Approve token selling to continue (one-time setup).
                </p>
                <button
                  onClick={activate}
                  disabled={isActivating}
                  className="mt-2 text-sm bg-yellow-500/20 text-yellow-400 px-4 py-2 sm:py-1.5 rounded hover:bg-yellow-500/30 active:bg-yellow-500/30 transition font-medium disabled:opacity-50 touch-target"
                >
                  {isActivating ? 'Enabling...' : 'Enable Selling'}
                </button>
              </div>
            </div>
          </div>
        )}

        {/* CTF Approval success */}
        {isExternal && activationStatus?.hasCtfApproval && (
          <div className="p-2.5 sm:p-3 bg-poly-green/10 border border-poly-green/30 rounded-lg flex items-center gap-2">
            <Check className="w-4 h-4 text-poly-green flex-shrink-0" />
            <span className="text-xs sm:text-sm text-poly-green">Selling enabled</span>
          </div>
        )}

        {/* Network switch warning */}
        {needsNetworkSwitch && (
          <div className="p-2.5 sm:p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
            <div className="flex items-start gap-2">
              <AlertTriangle className="w-4 h-4 sm:w-5 sm:h-5 text-yellow-400 flex-shrink-0 mt-0.5" />
              <div className="flex-1">
                <p className="text-xs sm:text-sm text-yellow-400 font-medium">Wrong Network</p>
                <p className="text-xs text-gray-400 mt-1">
                  Switch to Polygon to sell.
                </p>
                <button
                  onClick={handleSwitchNetwork}
                  className="mt-2 text-xs sm:text-sm bg-yellow-500/20 text-yellow-400 px-3 py-2 sm:py-1 rounded hover:bg-yellow-500/30 active:bg-yellow-500/30 transition touch-target"
                >
                  Switch to Polygon
                </button>
              </div>
            </div>
          </div>
        )}

        {displayError && (
          <div className="text-poly-red text-xs sm:text-sm bg-poly-red/10 px-3 py-2 rounded">
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
            onClick={handleSell}
            disabled={isProcessing || !isConnected() || needsNetworkSwitch || !position.token_id || !!(address && isExternal && activationStatus && !activationStatus.hasCtfApproval)}
            className="flex-1 py-3 sm:py-2 font-semibold rounded-lg transition touch-target disabled:opacity-50 bg-poly-red text-white hover:bg-poly-red/90 active:bg-poly-red/80 active:scale-[0.98]"
          >
            {isProcessing
              ? (isInitializing ? 'Connecting...' : isPlacingOrder ? 'Placing...' : isActivating ? 'Enabling...' : 'Processing...')
              : (address && isExternal && activationStatus && !activationStatus.hasCtfApproval)
                ? 'Enable First'
                : isPartialSell
                  ? `Sell ${sellPercentage}% (${sharesToSell.toFixed(1)})`
                  : `Sell All (${sharesToSell.toFixed(1)})`}
          </button>
        </div>
      </div>
    </Modal>
  )
}
