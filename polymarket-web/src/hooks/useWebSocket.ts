import { useEffect, useRef, useCallback } from 'react'
import { useOpportunityStore } from '../stores/opportunityStore'
import type { WsMessage } from '../types'

export function useWebSocket() {
  const wsRef = useRef<WebSocket | null>(null)
  const reconnectTimeoutRef = useRef<number | null>(null)
  const setOpportunities = useOpportunityStore((s) => s.setOpportunities)

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      return
    }

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsUrl = `${protocol}//${window.location.host}/ws`

    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.onopen = () => {
      console.log('WebSocket connected')
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
            console.log('Server:', message.data.message)
            break
          case 'opportunities':
            setOpportunities(message.data)
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
      console.log('WebSocket disconnected, reconnecting...')
      wsRef.current = null
      // Reconnect after 3 seconds
      reconnectTimeoutRef.current = window.setTimeout(connect, 3000)
    }

    ws.onerror = (error) => {
      console.error('WebSocket error:', error)
    }
  }, [setOpportunities])

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
