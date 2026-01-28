import { useEffect, useRef } from 'react'
import { useOpportunityStore, isCrypto, isSports } from '../stores/opportunityStore'
import type { Opportunity } from '../types'

// Filter for sniper opportunities (same logic as opportunityStore)
function filterSniperOpportunities(opportunities: Opportunity[]): Opportunity[] {
  return opportunities.filter((o) =>
    o.strategy === 'ResolutionSniper' &&
    !isCrypto(o) &&
    !isSports(o) &&
    o.time_to_close_hours !== null &&
    o.time_to_close_hours <= 12
  )
}

// Load sent alerts from localStorage
function loadSentAlerts(): Set<string> {
  try {
    const stored = localStorage.getItem('discord-sent-alerts')
    if (stored) {
      const parsed = JSON.parse(stored)
      // Only keep alerts from last 24 hours
      const cutoff = Date.now() - 24 * 60 * 60 * 1000
      const recent = parsed.filter((entry: { id: string; ts: number }) => entry.ts > cutoff)
      return new Set(recent.map((e: { id: string }) => e.id))
    }
  } catch (e) {
    console.error('Failed to load sent alerts:', e)
  }
  return new Set()
}

// Save sent alerts to localStorage
function saveSentAlerts(sentSet: Set<string>) {
  const entries = Array.from(sentSet).map(id => ({ id, ts: Date.now() }))
  localStorage.setItem('discord-sent-alerts', JSON.stringify(entries))
}

export function useDiscordAlerts() {
  const opportunities = useOpportunityStore((s) => s.opportunities)
  const sentAlertsRef = useRef<Set<string>>(loadSentAlerts())
  const isSendingRef = useRef(false)

  useEffect(() => {
    if (opportunities.length === 0) return

    const sendNewAlerts = async () => {
      if (isSendingRef.current) return

      // Filter for sniper opportunities
      const sniperOpps = filterSniperOpportunities(opportunities)
      console.log(`Discord: ${opportunities.length} total opps, ${sniperOpps.length} pass sniper filter`)

      // Find new opportunities that haven't been sent yet
      const newOpps = sniperOpps.filter(o => !sentAlertsRef.current.has(o.market_id))
      console.log(`Discord: ${newOpps.length} new (${sentAlertsRef.current.size} already sent)`)

      if (newOpps.length === 0) return

      isSendingRef.current = true

      try {
        // Build the request payload
        const payload = {
          opportunities: newOpps.map(o => ({
            market_id: o.market_id,
            question: o.question,
            side: o.side,
            entry_price: o.entry_price,
            edge: o.edge,
            expected_return: o.expected_return,
            confidence: o.confidence,
            time_to_close_hours: o.time_to_close_hours,
            liquidity: o.liquidity,
            slug: o.slug,
          }))
        }

        const response = await fetch('/api/discord/alerts', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(payload),
        })

        if (response.ok) {
          const result = await response.json()
          console.log(`Discord: sent ${result.sent} alerts`)

          // Mark these opportunities as sent
          newOpps.forEach(o => sentAlertsRef.current.add(o.market_id))
          saveSentAlerts(sentAlertsRef.current)
        } else {
          const errorText = await response.text()
          console.error('Failed to send Discord alerts:', errorText)
        }
      } catch (err) {
        console.error('Error sending Discord alerts:', err)
      } finally {
        isSendingRef.current = false
      }
    }

    sendNewAlerts()
  }, [opportunities])
}
