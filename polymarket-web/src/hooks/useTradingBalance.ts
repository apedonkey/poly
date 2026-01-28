import { useState, useCallback, useEffect, useRef } from 'react'
import { keccak256, encodeAbiParameters, getCreate2Address, formatUnits } from 'viem'
import type { Address } from 'viem'
import { useWalletStore } from '../stores/walletStore'
import { rpcCallWithRetry } from '../utils/rpc'

// Safe Proxy Factory for deriving proxy wallet address
const SAFE_FACTORY = '0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b' as const
const SAFE_INIT_CODE_HASH = '0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf' as const

// Rate limiting - minimum 5 seconds between fetches
const MIN_FETCH_INTERVAL = 5000

// USDC.e (bridged) on Polygon - used by Polymarket
const USDC_ADDRESS = '0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174' as const
const USDC_DECIMALS = 6

// Derive the Polymarket Safe proxy wallet address from an EOA
function deriveSafeWallet(eoaAddress: Address): Address {
  const salt = keccak256(encodeAbiParameters([{ type: 'address' }], [eoaAddress]))
  return getCreate2Address({
    from: SAFE_FACTORY,
    salt,
    bytecodeHash: SAFE_INIT_CODE_HASH,
  })
}

export interface TradingBalanceInfo {
  proxyAddress: string
  usdcBalance: string
  usdcFormatted: string
}

export function useTradingBalance() {
  const { address, isExternal } = useWalletStore()
  const [balance, setBalance] = useState<TradingBalanceInfo | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const lastFetchRef = useRef<number>(0)
  const fetchingRef = useRef<boolean>(false)

  const fetchBalance = useCallback(async () => {
    // Only fetch for external wallets (they use proxy wallets)
    if (!address || !isExternal) {
      setBalance(null)
      return null
    }

    // Rate limiting - prevent excessive fetches
    const now = Date.now()
    if (fetchingRef.current || now - lastFetchRef.current < MIN_FETCH_INTERVAL) {
      return balance
    }
    fetchingRef.current = true
    lastFetchRef.current = now

    setIsLoading(true)
    setError(null)

    try {
      const proxyAddress = deriveSafeWallet(address as Address)
      // console.log('Fetching trading balance - EOA:', address, '-> Proxy:', proxyAddress)

      // Fetch USDC balance of proxy wallet (with retry/fallback for rate limiting)
      const result = await rpcCallWithRetry({
        jsonrpc: '2.0',
        id: 1,
        method: 'eth_call',
        params: [{
          to: USDC_ADDRESS,
          data: `0x70a08231000000000000000000000000${proxyAddress.slice(2)}`,
        }, 'latest'],
      })
      // console.log('Trading balance RPC result:', result)

      if (result.result) {
        const rawBalance = BigInt(result.result)
        const formatted = formatUnits(rawBalance, USDC_DECIMALS)

        const info: TradingBalanceInfo = {
          proxyAddress,
          usdcBalance: rawBalance.toString(),
          usdcFormatted: parseFloat(formatted).toFixed(2),
        }

        // console.log('Trading balance:', info)
        setBalance(info)
        return info
      }

      // console.log('No balance result returned')
      return null
    } catch (err) {
      console.error('Failed to fetch trading balance:', err)
      setError('Failed to fetch balance')
      return null
    } finally {
      setIsLoading(false)
      fetchingRef.current = false
    }
  }, [address, isExternal, balance])

  // Fetch on mount and when address changes
  useEffect(() => {
    fetchBalance()
  }, [fetchBalance])

  return {
    balance,
    isLoading,
    error,
    refetch: fetchBalance,
    proxyAddress: balance?.proxyAddress || null,
  }
}
