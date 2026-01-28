import { create } from 'zustand'
import type { ClarificationAlert } from '../types'

interface ClarificationState {
  clarifications: ClarificationAlert[]
  setClarifications: (clarifications: ClarificationAlert[]) => void
  addClarification: (alert: ClarificationAlert) => void
  clearOld: (maxAgeHours?: number) => void
}

export const useClarificationStore = create<ClarificationState>((set) => ({
  clarifications: [],

  setClarifications: (clarifications) => set({ clarifications }),

  addClarification: (alert) =>
    set((state) => {
      // Avoid duplicates by market_id
      const exists = state.clarifications.some((c) => c.market_id === alert.market_id)
      if (exists) {
        // Update existing
        return {
          clarifications: state.clarifications.map((c) =>
            c.market_id === alert.market_id ? alert : c
          ),
        }
      }
      return { clarifications: [alert, ...state.clarifications] }
    }),

  clearOld: (maxAgeHours = 24) => {
    const cutoff = Date.now() / 1000 - maxAgeHours * 3600
    set((state) => ({
      clarifications: state.clarifications.filter((c) => c.detected_at > cutoff),
    }))
  },
}))
