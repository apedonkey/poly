import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { Opportunity } from '../types'

export type FilterType = 'all' | 'sniper' | 'nobias' | 'crypto' | 'sports'
export type SortType = 'time' | 'edge' | 'return' | 'liquidity'

// Use word boundary matching for precise detection
const matchesWord = (text: string, word: string) => {
  const regex = new RegExp(`\\b${word}\\b`, 'i')
  return regex.test(text)
}

// Export these for use in components
export const isCrypto = (o: Opportunity) => {
  const q = o.question
  const cat = o.category?.toLowerCase() || ''

  // Check category first
  if (cat === 'crypto' || cat === 'cryptocurrency') return true

  // Match specific crypto coins/tokens with word boundaries
  return (
    matchesWord(q, 'bitcoin') ||
    matchesWord(q, 'btc') ||
    matchesWord(q, 'ethereum') ||
    matchesWord(q, 'eth') ||
    matchesWord(q, 'solana') ||
    matchesWord(q, 'sol') ||
    matchesWord(q, 'xrp') ||
    matchesWord(q, 'ripple') ||
    matchesWord(q, 'dogecoin') ||
    matchesWord(q, 'doge') ||
    matchesWord(q, 'cardano') ||
    matchesWord(q, 'ada') ||
    matchesWord(q, 'polkadot') ||
    matchesWord(q, 'dot') ||
    matchesWord(q, 'avalanche') ||
    matchesWord(q, 'avax') ||
    matchesWord(q, 'chainlink') ||
    matchesWord(q, 'link') ||
    matchesWord(q, 'polygon') ||
    matchesWord(q, 'matic') ||
    matchesWord(q, 'litecoin') ||
    matchesWord(q, 'ltc') ||
    /\b(crypto|cryptocurrency)\b/i.test(q) ||
    // Price patterns like "BTC above $100,000"
    /\$[\d,]+.*\b(btc|eth|sol|xrp|bitcoin|ethereum)\b/i.test(q) ||
    /\b(btc|eth|sol|xrp|bitcoin|ethereum).*\$[\d,]+/i.test(q)
  )
}

export const isSports = (o: Opportunity) => {
  const q = o.question
  const cat = o.category?.toLowerCase() || ''

  // Check category first
  if (cat === 'sports') return true

  return (
    // Betting terms
    q.toLowerCase().includes('spread:') ||
    matchesWord(q, 'moneyline') ||
    /\bo\/u\b/i.test(q) ||  // O/U for over/under
    matchesWord(q, 'over/under') ||
    // Fighting/MMA terms
    matchesWord(q, 'fight') ||
    matchesWord(q, 'fighter') ||
    matchesWord(q, 'knockout') ||
    /\bKO\b/.test(q) ||
    /\bTKO\b/.test(q) ||
    matchesWord(q, 'submission') ||
    matchesWord(q, 'rounds') ||
    matchesWord(q, 'decision') ||
    matchesWord(q, 'unanimous') ||
    // Major leagues
    matchesWord(q, 'nba') ||
    matchesWord(q, 'nfl') ||
    matchesWord(q, 'mlb') ||
    matchesWord(q, 'nhl') ||
    matchesWord(q, 'mls') ||
    matchesWord(q, 'ufc') ||
    matchesWord(q, 'bellator') ||
    matchesWord(q, 'pga') ||
    matchesWord(q, 'atp') ||
    matchesWord(q, 'wta') ||
    // Soccer leagues
    matchesWord(q, 'premier league') ||
    matchesWord(q, 'la liga') ||
    matchesWord(q, 'serie a') ||
    matchesWord(q, 'bundesliga') ||
    matchesWord(q, 'ligue 1') ||
    matchesWord(q, 'champions league') ||
    matchesWord(q, 'europa league') ||
    // Major events
    matchesWord(q, 'super bowl') ||
    matchesWord(q, 'world series') ||
    matchesWord(q, 'stanley cup') ||
    matchesWord(q, 'world cup') ||
    // Team patterns - only match clear sports context
    /\b(lakers|celtics|warriors|knicks|bulls|heat|nets|76ers|bucks|suns)\b/i.test(q) ||
    /\b(yankees|dodgers|mets|red sox|cubs|astros|braves|phillies)\b/i.test(q) ||
    /\b(chiefs|eagles|cowboys|49ers|bills|ravens|bengals|dolphins|lions|packers)\b/i.test(q) ||
    /\b(real madrid|barcelona|man city|manchester united|liverpool fc|chelsea fc|arsenal fc|tottenham)\b/i.test(q) ||
    // Match "Team vs Team" or "Team (-X.X)" spread patterns
    /\b[A-Z][a-z]+\s+\(-?\d+\.?\d*\)\s*$/i.test(q) ||
    /\bvs\.?\s+[A-Z]/i.test(q) ||
    // "win on DATE" pattern for sports
    /\bwin on \d{4}-\d{2}-\d{2}\b/i.test(q) ||
    /\bwin on 202\d\b/i.test(q)
  )
}

interface OpportunityState {
  opportunities: Opportunity[]
  filter: FilterType
  sortBy: SortType
  setOpportunities: (opportunities: Opportunity[]) => void
  setFilter: (filter: FilterType) => void
  setSortBy: (sortBy: SortType) => void
  getFiltered: () => Opportunity[]
  getCounts: () => Record<FilterType, number>
}

export const useOpportunityStore = create<OpportunityState>()(
  persist(
    (set, get) => ({
      opportunities: [],
      filter: 'all',
      sortBy: 'time',
      setOpportunities: (opportunities) => set({ opportunities }),
      setFilter: (filter) => set({ filter }),
      setSortBy: (sortBy) => set({ sortBy }),
  getFiltered: () => {
    const { opportunities, filter } = get()

    const { sortBy } = get()

    // Helper to sort opportunities based on current sortBy setting
    const sortOpportunities = (arr: Opportunity[]) => {
      return [...arr].sort((a, b) => {
        switch (sortBy) {
          case 'time':
            const aTime = a.time_to_close_hours ?? Infinity
            const bTime = b.time_to_close_hours ?? Infinity
            return aTime - bTime
          case 'edge':
            return b.edge - a.edge
          case 'return':
            return b.expected_return - a.expected_return
          case 'liquidity':
            return parseFloat(b.liquidity) - parseFloat(a.liquidity)
          default:
            return 0
        }
      })
    }

    switch (filter) {
      case 'sniper':
        return sortOpportunities(opportunities.filter((o) =>
          o.strategy === 'ResolutionSniper' &&
          !isCrypto(o) &&
          !isSports(o) &&
          o.time_to_close_hours !== null &&
          o.time_to_close_hours <= 12
        ))
      case 'nobias':
        return sortOpportunities(opportunities.filter((o) => o.strategy === 'NoBias'))
      case 'crypto':
        return sortOpportunities(opportunities.filter(isCrypto))
      case 'sports':
        return sortOpportunities(opportunities.filter(isSports))
      default:
        return sortOpportunities(opportunities)
    }
  },
  getCounts: () => {
    const { opportunities } = get()

    return {
      all: opportunities.length,
      sniper: opportunities.filter((o) =>
        o.strategy === 'ResolutionSniper' &&
        !isCrypto(o) &&
        !isSports(o) &&
        o.time_to_close_hours !== null &&
        o.time_to_close_hours <= 12
      ).length,
      nobias: opportunities.filter((o) => o.strategy === 'NoBias').length,
      crypto: opportunities.filter(isCrypto).length,
      sports: opportunities.filter(isSports).length,
    }
  },
    }),
    {
      name: 'opportunity-preferences',
      partialize: (state) => ({ filter: state.filter, sortBy: state.sortBy }),
    }
  )
)
