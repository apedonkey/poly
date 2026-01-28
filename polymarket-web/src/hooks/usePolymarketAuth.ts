import { useSignTypedData } from 'wagmi'
import { useCallback, useState } from 'react'
import { useWalletStore } from '../stores/walletStore'

// EIP-712 Domain for Polymarket CLOB authentication
const CLOB_AUTH_DOMAIN = {
  name: 'ClobAuthDomain',
  version: '1',
  chainId: 137, // Polygon
} as const

// EIP-712 Types for ClobAuth message
const CLOB_AUTH_TYPES = {
  ClobAuth: [
    { name: 'address', type: 'address' },
    { name: 'timestamp', type: 'string' },
    { name: 'nonce', type: 'uint256' },
    { name: 'message', type: 'string' },
  ],
} as const

export interface ApiCredentials {
  api_key: string
  api_secret: string
  api_passphrase: string
}

// Store credentials in memory (per session)
let cachedCredentials: ApiCredentials | null = null
let credentialsWallet: string | null = null

// Get cached credentials (for use by other hooks)
export function getCachedCredentials(): ApiCredentials | null {
  return cachedCredentials
}

export function usePolymarketAuth() {
  const { address, sessionToken } = useWalletStore()
  const { signTypedDataAsync } = useSignTypedData()
  const [isAuthenticating, setIsAuthenticating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const authenticate = useCallback(async (): Promise<boolean> => {
    // Return cached credentials if available for this wallet
    if (cachedCredentials && credentialsWallet === address) {
      return true
    }

    if (!address || !sessionToken) {
      setError('Wallet not connected')
      return false
    }

    setIsAuthenticating(true)
    setError(null)

    try {
      // Step 1: Get server timestamp for synchronization
      const timeResponse = await fetch('/api/auth/time')
      let timestamp: string

      if (timeResponse.ok) {
        const timeData = await timeResponse.json()
        timestamp = timeData.toString()
      } else {
        // Fallback to local time if server time fails
        timestamp = Math.floor(Date.now() / 1000).toString()
      }

      const nonce = 0 // Use 0 for first auth

      // Step 2: Create and sign EIP-712 message
      const message = {
        address: address as `0x${string}`,
        timestamp,
        nonce: BigInt(nonce),
        message: 'This message attests that I control the given wallet',
      }

      const signature = await signTypedDataAsync({
        domain: CLOB_AUTH_DOMAIN,
        types: CLOB_AUTH_TYPES,
        primaryType: 'ClobAuth',
        message,
      })

      // Step 3: Send to backend to get API credentials from Polymarket
      const deriveResponse = await fetch('/api/auth/derive-api-key', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${sessionToken}`,
        },
        body: JSON.stringify({
          address,
          signature,
          timestamp,
          nonce,
        }),
      })

      if (!deriveResponse.ok) {
        const errData = await deriveResponse.json().catch(() => ({}))
        throw new Error(errData.error || `Failed to derive API key: ${deriveResponse.status}`)
      }

      const credentials: ApiCredentials = await deriveResponse.json()

      // Cache credentials
      cachedCredentials = credentials
      credentialsWallet = address

      return true
    } catch (err) {
      console.error('Auth error:', err)
      if ((err as { code?: number })?.code === 4001) {
        setError('Authentication rejected by user')
      } else {
        setError(err instanceof Error ? err.message : 'Failed to authenticate')
      }
      return false
    } finally {
      setIsAuthenticating(false)
    }
  }, [address, sessionToken, signTypedDataAsync])

  const clearCredentials = useCallback(() => {
    cachedCredentials = null
    credentialsWallet = null
  }, [])

  const hasCredentials = cachedCredentials !== null && credentialsWallet === address

  const getCredentials = useCallback(() => {
    if (cachedCredentials && credentialsWallet === address) {
      return cachedCredentials
    }
    return null
  }, [address])

  return {
    authenticate,
    clearCredentials,
    getCredentials,
    isAuthenticating,
    error,
    clearError: () => setError(null),
    hasCredentials,
  }
}
