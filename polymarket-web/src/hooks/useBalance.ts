import { useEffect, useCallback, useState, useRef } from 'react'
import { useAccount } from 'wagmi'
import { formatUnits } from 'viem'
import { useWalletStore } from '../stores/walletStore'
import { rpcCallWithRetry } from '../utils/rpc'
import { getWalletBalance } from '../api/client'

// USDC.e (bridged) on Polygon - used by Polymarket
const USDC_ADDRESS = '0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174'
const USDC_DECIMALS = 6

interface BalanceData {
  address: string
  usdc_balance: string
  matic_balance: string
  safe_address?: string
  safe_usdc_balance?: string
}

export function useBalance() {
  const { address: wagmiAddress } = useAccount()
  const { address: storedAddress, sessionToken, setBalance, isConnected, isExternal } = useWalletStore()
  const [data, setData] = useState<BalanceData | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const lastFetchRef = useRef<number>(0)

  // Fetch balance for generated wallets via backend API
  const fetchGeneratedBalance = useCallback(async () => {
    if (!sessionToken || !storedAddress || isExternal) return

    const now = Date.now()
    if (now - lastFetchRef.current < 5000) return
    lastFetchRef.current = now

    setIsLoading(true)
    try {
      const result = await getWalletBalance(sessionToken)
      const balanceData: BalanceData = {
        address: result.address,
        usdc_balance: result.usdc_balance,
        matic_balance: result.matic_balance,
        safe_address: result.safe_address,
        safe_usdc_balance: result.safe_usdc_balance,
      }
      setData(balanceData)
      setBalance({
        usdc: result.usdc_balance,
        matic: result.matic_balance,
        safe_usdc_balance: result.safe_usdc_balance,
      })
    } catch (err) {
      console.error('Failed to fetch generated wallet balance:', err)
    } finally {
      setIsLoading(false)
    }
  }, [sessionToken, storedAddress, isExternal, setBalance])

  // Fetch balance for external wallets via RPC
  const fetchExternalBalance = useCallback(async () => {
    if (!wagmiAddress || !isExternal) return

    const now = Date.now()
    if (now - lastFetchRef.current < 5000) return
    lastFetchRef.current = now

    setIsLoading(true)

    try {
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

  const fetchBalance = useCallback(() => {
    if (isExternal) {
      return fetchExternalBalance()
    } else {
      return fetchGeneratedBalance()
    }
  }, [isExternal, fetchExternalBalance, fetchGeneratedBalance])

  // Fetch on mount and when wallet changes
  useEffect(() => {
    if (isConnected()) {
      fetchBalance()
    }
  }, [isConnected, fetchBalance])

  // Poll every 30 seconds
  useEffect(() => {
    if (!isConnected()) return

    const interval = setInterval(fetchBalance, 30000)
    return () => clearInterval(interval)
  }, [isConnected, fetchBalance])

  // Listen for WebSocket wallet_balance events
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail
      if (detail?.usdc_balance) {
        setData((prev) => prev ? { ...prev, usdc_balance: detail.usdc_balance } : null)
        setBalance({
          usdc: detail.usdc_balance,
          matic: data?.matic_balance || '0.00',
        })
      }
    }
    window.addEventListener('wallet-balance', handler)
    return () => window.removeEventListener('wallet-balance', handler)
  }, [setBalance, data?.matic_balance])

  return {
    balance: data,
    isLoading,
    refetch: fetchBalance,
  }
}
