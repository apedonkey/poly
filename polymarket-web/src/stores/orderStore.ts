import { create } from 'zustand'
import type { OrderEvent } from '../types'

interface OrderState {
  // Track pending/live orders by market_id
  pendingOrderMarkets: Set<string>
  // Track order events by order_id
  orderEvents: Map<string, OrderEvent>
  // Record an order placed for a market
  addPendingOrder: (marketId: string, orderId: string) => void
  // Process an order event from WebSocket
  handleOrderEvent: (event: OrderEvent) => void
  // Check if a market has a pending order
  hasPendingOrder: (marketId: string) => boolean
}

// Map from order_id -> market_id for reverse lookups
const orderToMarket = new Map<string, string>()

export const useOrderStore = create<OrderState>()((set, get) => ({
  pendingOrderMarkets: new Set(),
  orderEvents: new Map(),

  addPendingOrder: (marketId, orderId) => {
    orderToMarket.set(orderId, marketId)
    set((state) => {
      const newSet = new Set(state.pendingOrderMarkets)
      newSet.add(marketId)
      return { pendingOrderMarkets: newSet }
    })
  },

  handleOrderEvent: (event) => {
    set((state) => {
      const newEvents = new Map(state.orderEvents)
      newEvents.set(event.order_id, event)

      // If order is terminal (confirmed, cancelled, failed), remove from pending
      const terminalStatuses = ['CONFIRMED', 'confirmed', 'CANCELLED', 'cancelled', 'FAILED', 'failed']
      if (terminalStatuses.includes(event.status)) {
        const marketId = orderToMarket.get(event.order_id)
        if (marketId) {
          const newPending = new Set(state.pendingOrderMarkets)
          newPending.delete(marketId)
          orderToMarket.delete(event.order_id)
          return { orderEvents: newEvents, pendingOrderMarkets: newPending }
        }
      }

      return { orderEvents: newEvents }
    })
  },

  hasPendingOrder: (marketId) => {
    return get().pendingOrderMarkets.has(marketId)
  },
}))
