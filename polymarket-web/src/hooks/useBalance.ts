import { useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { getWalletBalance } from '../api/client'
import { useWalletStore } from '../stores/walletStore'

export function useBalance() {
  const { address, sessionToken, setBalance, isConnected } = useWalletStore()

  const { data, isLoading, refetch } = useQuery({
    queryKey: ['balance', address],
    queryFn: () => getWalletBalance(sessionToken || undefined, address || undefined),
    enabled: isConnected() && !!address,
    refetchInterval: 30000, // Refresh every 30 seconds
    staleTime: 10000,
  })

  useEffect(() => {
    if (data) {
      setBalance({
        usdc: data.usdc_balance,
        matic: data.matic_balance,
      })
    }
  }, [data, setBalance])

  return {
    balance: data,
    isLoading,
    refetch,
  }
}
