import { useEffect, useRef, useCallback } from 'react'
import { useOpportunityStore } from '../stores/opportunityStore'
import { useClarificationStore } from '../stores/clarificationStore'
import { useDisputeStore } from '../stores/disputeStore'
import type { WsMessage } from '../types'

export function useWebSocket() {
  const wsRef = useRef<WebSocket | null>(null)
  const reconnectTimeoutRef = useRef<number | null>(null)
  const priceUpdatesRef = useRef<Map<string, string>>(new Map())
  const priceUpdateTimeoutRef = useRef<number | null>(null)
  const setOpportunities = useOpportunityStore((s) => s.setOpportunities)
  const setScanStatus = useOpportunityStore((s) => s.setScanStatus)
  const updatePrices = useOpportunityStore((s) => s.updatePrices)
  const setClarifications = useClarificationStore((s) => s.setClarifications)
  const setDisputes = useDisputeStore((s) => s.setDisputes)

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      return
    }

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${protocol}//${window.location.host}/ws`

    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.onopen = () => {
      // console.log('WebSocket connected')
      // Clear any pending reconnect
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current)
        reconnectTimeoutRef.current = null
      }
    }

    ws.onmessage = (event) => {
      try {
        const message: WsMessage = JSON.parse(event.data)

        switch (message.type) {
          case 'connected':
            // console.log('Server:', message.data.message)
            break
          case 'opportunities':
            setOpportunities(message.data)
            break
          case 'price_update':
            // Batch price updates to reduce re-renders
            priceUpdatesRef.current.set(message.data.token_id, message.data.price)

            // Dispatch event for positions immediately (they use React Query)
            window.dispatchEvent(
              new CustomEvent('price-update', {
                detail: { token_id: message.data.token_id, price: message.data.price },
              })
            )

            // Debounce the store update - apply all batched updates every 500ms
            if (!priceUpdateTimeoutRef.current) {
              priceUpdateTimeoutRef.current = window.setTimeout(() => {
                if (priceUpdatesRef.current.size > 0) {
                  updatePrices(priceUpdatesRef.current)
                  priceUpdatesRef.current = new Map()
                }
                priceUpdateTimeoutRef.current = null
              }, 500)
            }
            break
          case 'scan_status':
            // Update scan timing for progress bar
            setScanStatus(message.data.last_scan_at, message.data.scan_interval_seconds)
            break
          case 'clarifications':
            // Update clarification alerts
            setClarifications(message.data)
            break
          case 'disputes':
            // Update dispute alerts
            setDisputes(message.data)
            break
          case 'error':
            console.error('WebSocket error:', message.data.message)
            break
        }
      } catch (err) {
        console.error('Failed to parse WebSocket message:', err)
      }
    }

    ws.onclose = () => {
      // console.log('WebSocket disconnected, reconnecting...')
      wsRef.current = null
      // Reconnect after 3 seconds
      reconnectTimeoutRef.current = window.setTimeout(connect, 3000)
    }

    ws.onerror = (error) => {
      console.error('WebSocket error:', error)
    }
  }, [setOpportunities, setScanStatus, updatePrices, setClarifications, setDisputes])

  const disconnect = useCallback(() => {
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current)
      reconnectTimeoutRef.current = null
    }
    if (wsRef.current) {
      wsRef.current.close()
      wsRef.current = null
    }
  }, [])

  useEffect(() => {
    connect()
    return () => disconnect()
  }, [connect, disconnect])

  return { connect, disconnect }
}
