import { create } from 'zustand'
import type { DisputeAlert } from '../types'

interface DisputeState {
  disputes: DisputeAlert[]
  setDisputes: (disputes: DisputeAlert[]) => void
  addDispute: (alert: DisputeAlert) => void
  removeDispute: (marketId: string) => void
  clearResolved: () => void
}

export const useDisputeStore = create<DisputeState>((set) => ({
  disputes: [],

  setDisputes: (disputes) => set({ disputes }),

  addDispute: (alert) =>
    set((state) => {
      // Avoid duplicates by market_id
      const exists = state.disputes.some((d) => d.assertion_id === alert.assertion_id)
      if (exists) {
        // Update existing
        return {
          disputes: state.disputes.map((d) =>
            d.assertion_id === alert.assertion_id ? alert : d
          ),
        }
      }
      return { disputes: [alert, ...state.disputes] }
    }),

  removeDispute: (assertionId) =>
    set((state) => ({
      disputes: state.disputes.filter((d) => d.assertion_id !== assertionId),
    })),

  clearResolved: () => {
    // Clear disputes that have passed their estimated resolution time
    const now = Date.now() / 1000
    set((state) => ({
      disputes: state.disputes.filter((d) => d.estimated_resolution > now),
    }))
  },
}))
