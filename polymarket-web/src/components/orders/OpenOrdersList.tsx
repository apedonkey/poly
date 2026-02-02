import { useState, useEffect, useRef, useCallback } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Clock, X, AlertTriangle, RefreshCw, ShoppingCart, Zap } from 'lucide-react'
import { useWalletStore } from '../../stores/walletStore'
import { getOpenOrders, cancelOrder, cancelAllOrders, type OpenOrder } from '../../api/client'

export function OpenOrdersList() {
  const { sessionToken, isConnected } = useWalletStore()
  const queryClient = useQueryClient()
  const [cancellingId, setCancellingId] = useState<string | null>(null)
  const ordersRef = useRef<OpenOrder[]>([])
  // Track current market prices for each token
  const [marketPrices, setMarketPrices] = useState<Map<string, number>>(new Map())

  // Fetch open orders
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ['open-orders', sessionToken],
    queryFn: () => getOpenOrders(sessionToken!),
    enabled: isConnected(),
    refetchInterval: 5000, // Refresh every 5 seconds for near real-time
  })

  // Keep a ref of current orders for price comparison
  useEffect(() => {
    if (data?.orders) {
      ordersRef.current = data.orders
    }
  }, [data?.orders])

  // Listen for order events from User Channel WebSocket - immediate updates
  const handleOrderEvent = useCallback((event: CustomEvent<{ order_id: string; status: string; event_type: string }>) => {
    const { status } = event.detail
    // On any terminal or fill event, refetch immediately
    const importantStatuses = ['MATCHED', 'matched', 'MINED', 'mined', 'CONFIRMED', 'confirmed', 'CANCELLED', 'cancelled', 'FAILED', 'failed']
    if (importantStatuses.includes(status)) {
      refetch()
    }
  }, [refetch])

  useEffect(() => {
    window.addEventListener('order-event', handleOrderEvent as EventListener)
    return () => {
      window.removeEventListener('order-event', handleOrderEvent as EventListener)
    }
  }, [handleOrderEvent])

  // Listen for price updates - track prices and refetch if order might have filled
  const handlePriceUpdate = useCallback((event: CustomEvent<{ token_id: string; price: string }>) => {
    const { token_id, price } = event.detail
    const currentPrice = parseFloat(price)

    // Update market price for this token
    setMarketPrices(prev => {
      const next = new Map(prev)
      next.set(token_id, currentPrice)
      return next
    })

    // Check if any of our orders might have been affected
    const affectedOrder = ordersRef.current.find(order => {
      if (order.asset_id !== token_id) return false

      const orderPrice = parseFloat(order.price)
      const isBuy = order.side === 'BUY'

      // Buy order fills when market price <= limit price
      // Sell order fills when market price >= limit price
      if (isBuy && currentPrice <= orderPrice) return true
      if (!isBuy && currentPrice >= orderPrice) return true

      return false
    })

    if (affectedOrder) {
      // Price crossed our limit - order might have filled, refetch!
      refetch()
    }
  }, [refetch])

  useEffect(() => {
    window.addEventListener('price-update', handlePriceUpdate as EventListener)
    return () => {
      window.removeEventListener('price-update', handlePriceUpdate as EventListener)
    }
  }, [handlePriceUpdate])

  // Cancel mutation
  const cancelMutation = useMutation({
    mutationFn: (orderId: string) => cancelOrder(sessionToken!, orderId),
    onMutate: (orderId) => {
      setCancellingId(orderId)
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['open-orders'] })
    },
    onSettled: () => {
      setCancellingId(null)
    },
  })

  // Cancel all orders mutation
  const cancelAllMutation = useMutation({
    mutationFn: () => cancelAllOrders(sessionToken!),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['open-orders'] })
    },
  })

  const handleCancel = (orderId: string) => {
    if (window.confirm('Are you sure you want to cancel this order?')) {
      cancelMutation.mutate(orderId)
    }
  }

  const handleCancelAll = () => {
    if (window.confirm('Are you sure you want to cancel ALL open orders?')) {
      cancelAllMutation.mutate()
    }
  }

  const formatPrice = (price: string) => {
    const num = parseFloat(price)
    return `${(num * 100).toFixed(1)}c`
  }

  const formatSize = (size: string) => {
    const num = parseFloat(size)
    return `$${num.toFixed(2)}`
  }

  const formatDate = (dateStr?: string) => {
    if (!dateStr) return '-'
    const date = new Date(dateStr)
    return date.toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  if (!isConnected()) {
    return null
  }

  if (isLoading) {
    return (
      <div className="bg-poly-card rounded-xl border border-poly-border p-4">
        <div className="flex items-center gap-2 mb-4">
          <Clock className="w-5 h-5 text-yellow-400" />
          <h3 className="font-semibold">Limit Orders</h3>
        </div>
        <div className="text-center text-gray-400 py-4">Loading limit orders...</div>
      </div>
    )
  }

  if (error) {
    return (
      <div className="bg-poly-card rounded-xl border border-poly-border p-4">
        <div className="flex items-center gap-2 mb-4">
          <Clock className="w-5 h-5 text-yellow-400" />
          <h3 className="font-semibold">Limit Orders</h3>
        </div>
        <div className="text-center text-poly-red py-4">
          <AlertTriangle className="w-6 h-6 mx-auto mb-2" />
          Failed to load limit orders
        </div>
      </div>
    )
  }

  const orders = data?.orders || []

  return (
    <div className="bg-poly-card rounded-xl border border-poly-border p-4">
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <Clock className="w-5 h-5 text-yellow-400" />
          <h3 className="font-semibold">Limit Orders</h3>
          {orders.length > 0 && (
            <span className="bg-yellow-400/20 text-yellow-400 text-xs px-2 py-0.5 rounded-full">
              {orders.length}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {orders.length > 0 && (
            <button
              onClick={handleCancelAll}
              disabled={cancelAllMutation.isPending}
              className="text-xs px-2 py-1 text-poly-red hover:bg-poly-red/10 rounded transition disabled:opacity-50"
              title="Cancel all open orders"
            >
              {cancelAllMutation.isPending ? 'Cancelling...' : 'Cancel All'}
            </button>
          )}
          <button
            onClick={() => refetch()}
            className="p-1.5 hover:bg-poly-dark rounded-lg transition"
            title="Refresh limit orders"
          >
            <RefreshCw className="w-4 h-4 text-gray-400" />
          </button>
        </div>
      </div>

      {orders.length === 0 ? (
        <div className="text-center text-gray-500 py-6">
          <ShoppingCart className="w-8 h-8 mx-auto mb-2 opacity-50" />
          <p>No limit orders found</p>
          <p className="text-xs mt-1">Place a limit order from the trading modal</p>
        </div>
      ) : (
        <div className="space-y-3">
          {orders.map((order) => (
            <OrderCard
              key={order.id}
              order={order}
              onCancel={() => handleCancel(order.id)}
              isCancelling={cancellingId === order.id}
              marketPrice={marketPrices.get(order.asset_id)}
              formatPrice={formatPrice}
              formatSize={formatSize}
              formatDate={formatDate}
            />
          ))}
        </div>
      )}

      {cancelMutation.error && (
        <div className="mt-3 p-2 bg-poly-red/10 border border-poly-red/30 rounded text-sm text-poly-red">
          Failed to cancel order: {cancelMutation.error.message}
        </div>
      )}
    </div>
  )
}

interface OrderCardProps {
  order: OpenOrder
  onCancel: () => void
  isCancelling: boolean
  marketPrice?: number
  formatPrice: (price: string) => string
  formatSize: (size: string) => string
  formatDate: (date?: string) => string
}

function OrderCard({
  order,
  onCancel,
  isCancelling,
  marketPrice,
  formatPrice,
  formatSize,
  formatDate: _formatDate,
}: OrderCardProps) {
  const isBuy = order.side === 'BUY'
  const filled = parseFloat(order.size_matched)
  const total = parseFloat(order.original_size)
  const fillPercent = total > 0 ? (filled / total) * 100 : 0
  const orderPrice = parseFloat(order.price)

  // Calculate how close the market price is to the limit
  let proximityStatus: 'far' | 'approaching' | 'very-close' | 'crossed' = 'far'
  let distancePercent = 100

  if (marketPrice !== undefined) {
    // For buy orders: we want price to go DOWN to our limit
    // For sell orders: we want price to go UP to our limit
    if (isBuy) {
      distancePercent = ((marketPrice - orderPrice) / orderPrice) * 100
      if (marketPrice <= orderPrice) {
        proximityStatus = 'crossed'
      } else if (distancePercent <= 2) {
        proximityStatus = 'very-close'
      } else if (distancePercent <= 5) {
        proximityStatus = 'approaching'
      }
    } else {
      distancePercent = ((orderPrice - marketPrice) / orderPrice) * 100
      if (marketPrice >= orderPrice) {
        proximityStatus = 'crossed'
      } else if (distancePercent <= 2) {
        proximityStatus = 'very-close'
      } else if (distancePercent <= 5) {
        proximityStatus = 'approaching'
      }
    }
  }

  const getBorderClass = () => {
    switch (proximityStatus) {
      case 'crossed':
        return 'border-poly-green animate-pulse'
      case 'very-close':
        return 'border-yellow-400'
      case 'approaching':
        return 'border-yellow-400/50'
      default:
        return 'border-poly-border'
    }
  }

  return (
    <div className={`bg-poly-dark rounded-lg p-3 border ${getBorderClass()} transition-colors`}>
      {/* Header */}
      <div className="flex items-start justify-between gap-2 mb-2">
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium truncate">
            {order.market_question || `Token: ${order.asset_id.slice(0, 12)}...`}
          </p>
          <div className="flex items-center gap-2 mt-1">
            <span
              className={`text-xs font-semibold px-1.5 py-0.5 rounded ${
                isBuy ? 'bg-poly-green/20 text-poly-green' : 'bg-poly-red/20 text-poly-red'
              }`}
            >
              {order.side}
            </span>
            <span className="text-xs text-gray-400">{order.order_type}</span>
            {/* Proximity indicator */}
            {proximityStatus === 'crossed' && (
              <span className="flex items-center gap-1 text-xs font-semibold text-poly-green animate-pulse">
                <Zap className="w-3 h-3" />
                Filling!
              </span>
            )}
            {proximityStatus === 'very-close' && (
              <span className="text-xs font-medium text-yellow-400">Almost there!</span>
            )}
            {proximityStatus === 'approaching' && (
              <span className="text-xs text-yellow-400/70">Approaching</span>
            )}
          </div>
        </div>
        <button
          onClick={onCancel}
          disabled={isCancelling}
          className="p-1.5 hover:bg-poly-red/20 rounded transition text-gray-400 hover:text-poly-red disabled:opacity-50"
          title="Cancel order"
        >
          <X className="w-4 h-4" />
        </button>
      </div>

      {/* Details */}
      <div className="grid grid-cols-3 gap-2 text-sm">
        <div>
          <span className="text-gray-500 text-xs">Limit</span>
          <p className="font-medium">{formatPrice(order.price)}</p>
        </div>
        <div>
          <span className="text-gray-500 text-xs">Market</span>
          <p className={`font-medium ${
            proximityStatus === 'crossed' ? 'text-poly-green' :
            proximityStatus === 'very-close' ? 'text-yellow-400' :
            proximityStatus === 'approaching' ? 'text-yellow-400/70' : ''
          }`}>
            {marketPrice !== undefined ? `${(marketPrice * 100).toFixed(1)}c` : '-'}
          </p>
        </div>
        <div>
          <span className="text-gray-500 text-xs">Size</span>
          <p className="font-medium">{formatSize(order.original_size)}</p>
        </div>
      </div>

      {/* Distance indicator */}
      {marketPrice !== undefined && proximityStatus !== 'far' && (
        <div className="mt-2 text-xs">
          <span className={`${
            proximityStatus === 'crossed' ? 'text-poly-green' : 'text-yellow-400'
          }`}>
            {proximityStatus === 'crossed'
              ? `Price ${isBuy ? 'at or below' : 'at or above'} your limit!`
              : `${Math.abs(distancePercent).toFixed(1)}% ${isBuy ? 'above' : 'below'} your limit`
            }
          </span>
        </div>
      )}

      {/* Fill progress */}
      {fillPercent > 0 && (
        <div className="mt-2">
          <div className="flex justify-between text-xs text-gray-400 mb-1">
            <span>Filled</span>
            <span>{fillPercent.toFixed(1)}%</span>
          </div>
          <div className="h-1.5 bg-gray-700 rounded-full overflow-hidden">
            <div
              className="h-full bg-poly-green rounded-full"
              style={{ width: `${fillPercent}%` }}
            />
          </div>
        </div>
      )}
    </div>
  )
}
