import { useState, useEffect, memo } from 'react'
import { Clock, TrendingUp, TrendingDown, FileText, Timer, DollarSign, ExternalLink, Loader2, Gift, Share2, Zap } from 'lucide-react'
import type { Position } from '../../types'
import { useWalletStore } from '../../stores/walletStore'
import { updatePositionTokenId, fetchMarketTokenIds, redeemPosition } from '../../api/client'
import { PnlCard } from './PnlCard'

interface Props {
  position: Position
  onSell?: (position: Position) => void
  onTokenIdUpdated?: () => void
  onRedeemed?: () => void
}

export const PositionCard = memo(function PositionCard({ position, onSell, onTokenIdUpdated, onRedeemed }: Props) {
  const { sessionToken } = useWalletStore()
  const [localTokenId, setLocalTokenId] = useState<string | null>(position.token_id)
  const [isLoadingTokenId, setIsLoadingTokenId] = useState(false)
  const [isRedeeming, setIsRedeeming] = useState(false)
  const [redeemError, setRedeemError] = useState<string | null>(null)
  const [showPnlCard, setShowPnlCard] = useState(false)
  // Track live price for this position
  const [livePrice, setLivePrice] = useState<string | null>(null)
  const [priceFlash, setPriceFlash] = useState<'up' | 'down' | null>(null)

  const entryPrice = parseFloat(position.entry_price)
  const size = parseFloat(position.size)
  const pnl = position.pnl ? parseFloat(position.pnl) : null
  const isProfitable = pnl !== null && pnl > 0

  // Partial sell tracking
  const originalShares = size / entryPrice
  const remainingShares = position.remaining_size ? parseFloat(position.remaining_size) : originalShares
  const totalSoldShares = position.total_sold_size ? parseFloat(position.total_sold_size) : 0
  const realizedPnl = position.realized_pnl ? parseFloat(position.realized_pnl) : 0
  // avgExitPrice available in position.avg_exit_price if needed for display

  // Calculate remaining percentage of original position
  const remainingPercent = originalShares > 0 ? (remainingShares / originalShares) * 100 : 100
  const isPartialPosition = totalSoldShares > 0 && remainingShares > 0

  // Listen for real-time price updates
  useEffect(() => {
    const effectiveTokenId = localTokenId || position.token_id
    if (!effectiveTokenId || position.status !== 'Open') return

    const handlePriceUpdate = (event: CustomEvent<{ token_id: string; price: string }>) => {
      const { token_id, price } = event.detail
      if (token_id === effectiveTokenId) {
        setLivePrice((prevPrice) => {
          // Flash animation on price change
          if (prevPrice !== null) {
            const prev = parseFloat(prevPrice)
            const curr = parseFloat(price)
            if (curr > prev) {
              setPriceFlash('up')
            } else if (curr < prev) {
              setPriceFlash('down')
            }
            // Clear flash after animation
            setTimeout(() => setPriceFlash(null), 500)
          }
          return price
        })
      }
    }

    window.addEventListener('price-update', handlePriceUpdate as EventListener)
    return () => window.removeEventListener('price-update', handlePriceUpdate as EventListener)
  }, [localTokenId, position.token_id, position.status])

  // Calculate unrealized PnL for open positions (based on remaining shares)
  const currentPrice = livePrice ? parseFloat(livePrice) : entryPrice
  const unrealizedPnl = position.status === 'Open' ? (currentPrice - entryPrice) * remainingShares : null
  const isUnrealizedProfitable = unrealizedPnl !== null && unrealizedPnl > 0

  // Total PnL (realized + unrealized) for partial positions
  const totalUnrealizedAndRealized = unrealizedPnl !== null ? unrealizedPnl + realizedPnl : null

  // Auto-fetch token_id if missing for open positions
  useEffect(() => {
    const fetchTokenId = async () => {
      if (position.token_id || localTokenId || position.status !== 'Open' || position.is_paper) {
        return
      }

      setIsLoadingTokenId(true)
      try {
        // Use market_id as condition_id to fetch token IDs
        const tokens = await fetchMarketTokenIds(position.market_id)
        if (tokens) {
          const tokenId = position.side === 'Yes' ? tokens.yes_token_id : tokens.no_token_id
          if (tokenId && sessionToken) {
            // Update in backend
            await updatePositionTokenId(sessionToken, position.id, tokenId)
            setLocalTokenId(tokenId)
            onTokenIdUpdated?.()
          }
        }
      } catch (err) {
        console.error('Failed to fetch token_id:', err)
      } finally {
        setIsLoadingTokenId(false)
      }
    }

    fetchTokenId()
  }, [position.id, position.token_id, position.market_id, position.side, position.status, position.is_paper, localTokenId, sessionToken, onTokenIdUpdated])

  const formatDate = (dateStr: string) => {
    const date = new Date(dateStr)
    return date.toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  // Calculate time remaining until market ends
  const getTimeRemaining = () => {
    if (!position.end_date) return null
    const endDate = new Date(position.end_date)
    const now = new Date()
    const diffMs = endDate.getTime() - now.getTime()

    if (diffMs <= 0) return 'Ended'

    const hours = diffMs / (1000 * 60 * 60)
    if (hours < 1) return `${Math.round(hours * 60)}m`
    if (hours < 24) return `${hours.toFixed(1)}h`
    if (hours < 24 * 7) return `${(hours / 24).toFixed(1)}d`
    return `${(hours / (24 * 7)).toFixed(1)}w`
  }

  const timeRemaining = getTimeRemaining()

  // Can sell if: open status, not paper, has token_id (local or from position), and has sell handler
  const effectiveTokenId = localTokenId || position.token_id
  const canSell = position.status === 'Open' && !position.is_paper && effectiveTokenId && onSell

  // Can redeem if: resolved status, not paper (can claim $0 for losses too)
  const canRedeem = position.status === 'Resolved' && pnl !== null && !position.is_paper
  const isWinner = pnl !== null && pnl > 0

  // Can share if: closed or resolved with PnL calculated
  const canShare = (position.status === 'Resolved' || position.status === 'Closed') && pnl !== null

  const handleRedeem = async () => {
    if (!sessionToken || isRedeeming) return

    setIsRedeeming(true)
    setRedeemError(null)

    try {
      const result = await redeemPosition(sessionToken, position.id)
      if (result.success) {
        onRedeemed?.()
      } else {
        setRedeemError(result.message || 'Redeem failed')
      }
    } catch (err) {
      setRedeemError(err instanceof Error ? err.message : 'Redeem failed')
    } finally {
      setIsRedeeming(false)
    }
  }

  // Polymarket URL - prefer slug for user-friendly URLs
  const polymarketUrl = position.slug
    ? `https://polymarket.com/event/${position.slug}`
    : `https://polymarket.com/event/${position.market_id}`

  const statusColors = {
    Open: 'bg-blue-500/20 text-blue-400',
    PendingResolution: 'bg-yellow-500/20 text-yellow-400',
    Resolved: pnl && pnl > 0 ? 'bg-poly-green/20 text-poly-green' : 'bg-poly-red/20 text-poly-red',
    Closed: 'bg-gray-500/20 text-gray-400',
  }

  return (
    <div className="bg-poly-card rounded-xl border border-poly-border p-3 sm:p-4">
      {/* Header with badges and question */}
      <div className="flex items-start justify-between gap-2 sm:gap-3 mb-3">
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 sm:gap-2 mb-1.5 flex-wrap">
            <span className={`text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded ${statusColors[position.status]}`}>
              {position.status === 'PendingResolution' ? 'Pending' : position.status}
            </span>
            <span className={`text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded ${
              position.strategy === 'ResolutionSniper'
                ? 'bg-yellow-500/20 text-yellow-400'
                : 'bg-blue-500/20 text-blue-400'
            }`}>
              {position.strategy === 'ResolutionSniper' ? 'Sniper' : 'NO Bias'}
            </span>
            {position.is_paper && (
              <span className="text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded bg-purple-500/20 text-purple-400 flex items-center gap-1">
                <FileText className="w-3 h-3" />
                Paper
              </span>
            )}
            {position.status === 'Resolved' && pnl !== null && (
              <span className={`text-xs font-medium px-1.5 sm:px-2 py-0.5 rounded flex items-center gap-1 ${
                pnl > 0
                  ? 'bg-poly-green/20 text-poly-green'
                  : 'bg-poly-red/20 text-poly-red'
              }`}>
                {pnl > 0 ? (
                  <>
                    <TrendingUp className="w-3 h-3" />
                    Winner
                  </>
                ) : (
                  <>
                    <TrendingDown className="w-3 h-3" />
                    Loser
                  </>
                )}
              </span>
            )}
          </div>
          <a
            href={polymarketUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="font-medium text-sm leading-tight line-clamp-2 hover:text-poly-green active:text-poly-green transition flex items-start gap-1 group"
          >
            {position.question}
            <ExternalLink className="w-3 h-3 flex-shrink-0 mt-0.5 opacity-50 sm:opacity-0 group-hover:opacity-100 transition-opacity" />
          </a>
        </div>
        {pnl !== null && (
          <div className={`flex items-center gap-1 flex-shrink-0 ${isProfitable ? 'text-poly-green' : 'text-poly-red'}`}>
            {isProfitable ? (
              <TrendingUp className="w-4 h-4" />
            ) : (
              <TrendingDown className="w-4 h-4" />
            )}
            <span className="font-bold text-sm sm:text-base">
              {isProfitable ? '+' : ''}{pnl.toFixed(2)}
            </span>
          </div>
        )}
      </div>

      {/* Stats Grid - Responsive layout */}
      <div className={`grid ${position.status === 'Open' && livePrice ? 'grid-cols-2 sm:grid-cols-4' : 'grid-cols-3'} gap-2 sm:gap-3 text-center`}>
        <div className="p-2 sm:p-0 bg-poly-dark/30 sm:bg-transparent rounded-lg">
          <div className={`text-base sm:text-lg font-bold ${
            position.side === 'Yes' ? 'text-poly-green' : 'text-poly-red'
          }`}>
            {position.side}
          </div>
          <div className="text-xs text-gray-500">Side</div>
        </div>
        <div className="p-2 sm:p-0 bg-poly-dark/30 sm:bg-transparent rounded-lg">
          <div className="text-base sm:text-lg font-bold">{(entryPrice * 100).toFixed(0)}c</div>
          <div className="text-xs text-gray-500">Entry</div>
        </div>
        {/* Show live price for open positions */}
        {position.status === 'Open' && livePrice && (
          <div className="p-2 sm:p-0 bg-poly-dark/30 sm:bg-transparent rounded-lg">
            <div className={`text-base sm:text-lg font-bold flex items-center justify-center gap-1 transition-colors duration-300 ${
              priceFlash === 'up' ? 'text-poly-green' :
              priceFlash === 'down' ? 'text-poly-red' : ''
            }`}>
              <Zap className="w-3 h-3 text-yellow-400" />
              {(currentPrice * 100).toFixed(0)}c
            </div>
            <div className="text-xs text-gray-500">Live</div>
          </div>
        )}
        <div className="p-2 sm:p-0 bg-poly-dark/30 sm:bg-transparent rounded-lg">
          <div className="text-base sm:text-lg font-bold">
            {isPartialPosition
              ? `${remainingShares.toFixed(1)}`
              : `$${size.toFixed(2)}`
            }
          </div>
          <div className="text-xs text-gray-500">
            {isPartialPosition ? 'Remaining' : 'Size'}
          </div>
        </div>
      </div>

      {/* Partial sell info - show when position has been partially sold */}
      {isPartialPosition && position.status === 'Open' && (
        <div className="mt-2 p-2 rounded-lg bg-blue-500/10 border border-blue-500/20">
          <div className="flex justify-between items-center text-xs sm:text-sm">
            <span className="text-blue-400">
              {remainingPercent.toFixed(0)}% remaining ({remainingShares.toFixed(1)} shares)
            </span>
            <span className={`font-medium ${realizedPnl >= 0 ? 'text-poly-green' : 'text-poly-red'}`}>
              Realized: {realizedPnl >= 0 ? '+' : ''}{realizedPnl.toFixed(2)} USDC
            </span>
          </div>
        </div>
      )}

      {/* Unrealized PnL for open positions with live price */}
      {position.status === 'Open' && livePrice && unrealizedPnl !== null && (
        <div className={`mt-2 p-2 rounded-lg text-center ${
          isUnrealizedProfitable ? 'bg-poly-green/10' : 'bg-poly-red/10'
        }`}>
          <div className={`text-sm font-medium flex items-center justify-center gap-1 ${
            isUnrealizedProfitable ? 'text-poly-green' : 'text-poly-red'
          }`}>
            {isUnrealizedProfitable ? (
              <TrendingUp className="w-3.5 h-3.5" />
            ) : (
              <TrendingDown className="w-3.5 h-3.5" />
            )}
            Unrealized: {isUnrealizedProfitable ? '+' : ''}{unrealizedPnl.toFixed(2)} USDC
            {/* Show combined total for partial positions */}
            {isPartialPosition && totalUnrealizedAndRealized !== null && (
              <span className="text-gray-400 ml-1">
                (Total: {totalUnrealizedAndRealized >= 0 ? '+' : ''}{totalUnrealizedAndRealized.toFixed(2)})
              </span>
            )}
          </div>
        </div>
      )}

      {/* Time Info - Stack on very small screens */}
      <div className="flex flex-col xs:flex-row items-start xs:items-center justify-between text-xs text-gray-500 mt-3 gap-1 xs:gap-2">
        <div className="flex items-center gap-1">
          <Clock className="w-3.5 h-3.5 flex-shrink-0" />
          <span className="truncate">{formatDate(position.opened_at)}</span>
        </div>
        {timeRemaining && position.status === 'Open' && (
          <div className={`flex items-center gap-1 ${
            timeRemaining === 'Ended' ? 'text-poly-red' :
            timeRemaining.includes('h') && parseFloat(timeRemaining) < 4 ? 'text-yellow-400' :
            'text-gray-400'
          }`}>
            <Timer className="w-3.5 h-3.5 flex-shrink-0" />
            <span>{timeRemaining === 'Ended' ? 'Ended' : `${timeRemaining} left`}</span>
          </div>
        )}
      </div>

      {/* Sell button for live positions */}
      {position.status === 'Open' && !position.is_paper && onSell && (
        <>
          {isLoadingTokenId ? (
            <div className="w-full mt-3 py-2.5 sm:py-2 text-gray-400 text-sm flex items-center justify-center gap-2">
              <Loader2 className="w-4 h-4 animate-spin" />
              Loading...
            </div>
          ) : canSell ? (
            <button
              onClick={() => onSell({ ...position, token_id: effectiveTokenId })}
              className="w-full mt-3 py-3 sm:py-2 bg-poly-red/20 hover:bg-poly-red/30 active:bg-poly-red/30 text-poly-red font-medium rounded-lg transition flex items-center justify-center gap-2 touch-target active:scale-[0.98]"
            >
              <DollarSign className="w-4 h-4" />
              Sell Position
            </button>
          ) : (
            <div className="w-full mt-3 py-2 text-gray-500 text-xs text-center">
              Token data unavailable
            </div>
          )}
        </>
      )}

      {/* Claim button for resolved positions */}
      {canRedeem && (
        <div className="mt-3">
          <button
            onClick={handleRedeem}
            disabled={isRedeeming}
            className={`w-full py-3 sm:py-2 font-medium rounded-lg transition flex items-center justify-center gap-2 touch-target active:scale-[0.98] disabled:opacity-50 disabled:cursor-not-allowed ${
              isWinner
                ? 'bg-poly-green/20 hover:bg-poly-green/30 active:bg-poly-green/30 text-poly-green'
                : 'bg-gray-500/20 hover:bg-gray-500/30 active:bg-gray-500/30 text-gray-300'
            }`}
          >
            {isRedeeming ? (
              <>
                <Loader2 className="w-4 h-4 animate-spin" />
                Claiming...
              </>
            ) : isWinner ? (
              <>
                <Gift className="w-4 h-4" />
                Claim ${pnl?.toFixed(2)} USDC
              </>
            ) : (
              <>
                <Gift className="w-4 h-4" />
                Redeem Position
              </>
            )}
          </button>
          {redeemError && (
            <div className="mt-2 text-xs text-poly-red text-center">
              {redeemError}
            </div>
          )}
        </div>
      )}

      {/* Share PnL button for closed/resolved positions */}
      {canShare && (
        <button
          onClick={() => setShowPnlCard(true)}
          className="w-full mt-3 py-2 bg-poly-card hover:bg-poly-border border border-poly-border rounded-lg transition flex items-center justify-center gap-2 text-sm text-gray-300"
        >
          <Share2 className="w-4 h-4" />
          Share Result
        </button>
      )}

      {/* PnL Card Modal */}
      <PnlCard
        position={position}
        isOpen={showPnlCard}
        onClose={() => setShowPnlCard(false)}
      />
    </div>
  )
}, (prev, next) => {
  // Custom comparison - only re-render if these key fields change
  const p1 = prev.position
  const p2 = next.position
  return (
    p1.id === p2.id &&
    p1.status === p2.status &&
    p1.entry_price === p2.entry_price &&
    p1.size === p2.size &&
    p1.pnl === p2.pnl &&
    p1.token_id === p2.token_id &&
    p1.remaining_size === p2.remaining_size &&
    p1.realized_pnl === p2.realized_pnl &&
    p1.end_date === p2.end_date &&
    prev.onSell === next.onSell
  )
})
