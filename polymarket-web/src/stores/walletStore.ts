import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface WalletBalance {
  usdc: string
  matic: string
  safe_usdc_balance?: string
}

interface WalletState {
  address: string | null
  sessionToken: string | null
  isExternal: boolean // true if connected via MetaMask
  balance: WalletBalance | null
  setWallet: (address: string, sessionToken: string, isExternal?: boolean) => void
  setBalance: (balance: WalletBalance) => void
  clearWallet: () => void
  isConnected: () => boolean
}

export const useWalletStore = create<WalletState>()(
  persist(
    (set, get) => ({
      address: null,
      sessionToken: null,
      isExternal: false,
      balance: null,
      setWallet: (address, sessionToken, isExternal = false) =>
        set({ address, sessionToken, isExternal }),
      setBalance: (balance) => set({ balance }),
      clearWallet: () =>
        set({ address: null, sessionToken: null, isExternal: false, balance: null }),
      isConnected: () => !!get().sessionToken,
    }),
    {
      name: 'polymarket-wallet',
      partialize: (state) => ({
        address: state.address,
        sessionToken: state.sessionToken,
        isExternal: state.isExternal,
      }),
    }
  )
)
