import { create } from 'zustand'
import type { MintMakerStatus } from '../types'

interface MintMakerState {
  status: MintMakerStatus | null
  setStatus: (status: MintMakerStatus) => void
}

export const useMintMakerStore = create<MintMakerState>()((set) => ({
  status: null,
  setStatus: (status) => set({ status }),
}))
