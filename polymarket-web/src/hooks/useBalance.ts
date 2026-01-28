import { useEffect, useCallback, useState, useRef } from 'react'
import { useAccount } from 'wagmi'
import { formatUnits } from 'viem'
import { useWalletStore } from '../stores/walletStore'
import { rpcCallWithRetry } from '../utils/rpc'

// USDC.e (bridged) on Polygon - used by Polymarket
const USDC_ADDRESS = '0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174'
const USDC_DECIMALS = 6

interface BalanceData {
  address: string
  usdc_balance: string
  matic_balance: string
}

export function useBalance() {
  const { address: wagmiAddress } = useAccount()
  const { setBalance, isConnected, isExternal } = useWalletStore()
  const [data, setData] = useState<BalanceData | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const lastFetchRef = useRef<number>(0)

  const fetchBalance = useCallback(async () => {
    // Only fetch for external wallets with a valid address
    if (!wagmiAddress || !isExternal) {
      return
    }

    // Rate limit - minimum 5 seconds between fetches
    const now = Date.now()
    if (now - lastFetchRef.current < 5000) {
      return
    }
    lastFetchRef.current = now

    setIsLoading(true)

    try {
      // Fetch USDC.e and POL balances with retry/fallback for rate limiting
      const [usdcResult, maticResult] = await Promise.all([
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 1,
          method: 'eth_call',
          params: [{
            to: USDC_ADDRESS,
            data: `0x70a08231000000000000000000000000${wagmiAddress.slice(2).toLowerCase()}`,
          }, 'latest'],
        }),
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 2,
          method: 'eth_getBalance',
          params: [wagmiAddress, 'latest'],
        }),
      ])

      let usdcBalance = '0.00'
      let maticBalance = '0.00'

      if (usdcResult?.result) {
        const rawBalance = BigInt(usdcResult.result)
        const formatted = formatUnits(rawBalance, USDC_DECIMALS)
        usdcBalance = parseFloat(formatted).toFixed(2)
      }

      if (maticResult?.result) {
        const rawBalance = BigInt(maticResult.result)
        const formatted = formatUnits(rawBalance, 18)
        maticBalance = parseFloat(formatted).toFixed(4)
      }

      const balanceData: BalanceData = {
        address: wagmiAddress,
        usdc_balance: usdcBalance,
        matic_balance: maticBalance,
      }

      setData(balanceData)
      setBalance({
        usdc: usdcBalance,
        matic: maticBalance,
      })
    } catch (err) {
      console.error('Failed to fetch balance:', err)
    } finally {
      setIsLoading(false)
    }
  }, [wagmiAddress, isExternal, setBalance])

  // Fetch on mount and when address changes
  useEffect(() => {
    if (wagmiAddress && isConnected() && isExternal) {
      fetchBalance()
    }
  }, [wagmiAddress, isConnected, isExternal, fetchBalance])

  // Set up polling
  useEffect(() => {
    if (!wagmiAddress || !isConnected() || !isExternal) return

    const interval = setInterval(fetchBalance, 30000)
    return () => clearInterval(interval)
  }, [wagmiAddress, isConnected, isExternal, fetchBalance])

  return {
    balance: data,
    isLoading,
    refetch: fetchBalance,
  }
}
