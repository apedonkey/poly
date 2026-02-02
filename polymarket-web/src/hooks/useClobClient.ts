import { useState, useCallback, useRef, useEffect } from 'react'
import { useWalletClient, useChainId } from 'wagmi'
import { ClobClient, Side } from '@polymarket/clob-client'
import { ethers } from 'ethers'
import { keccak256, encodeAbiParameters, getCreate2Address } from 'viem'
import type { Address } from 'viem'
import { useWalletStore } from '../stores/walletStore'
import { rpcCallWithRetry } from '../utils/rpc'

const CLOB_HOST = 'https://clob.polymarket.com'
const POLYGON_CHAIN_ID = 137

// Signature type for browser wallets with Safe proxy
const SIGNATURE_TYPE_POLY_GNOSIS_SAFE = 2

// Safe Proxy Factory for deriving proxy wallet address
const SAFE_FACTORY = '0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b' as const
const SAFE_INIT_CODE_HASH = '0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf' as const

// Polymarket contract addresses on Polygon
const USDC_E_ADDRESS = '0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174' // USDC.e (bridged) - used by Polymarket
const CTF_EXCHANGE = '0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E' // CTF Exchange
const NEG_RISK_CTF_EXCHANGE = '0xC5d563A36AE78145C45a50134d48A1215220f80a' // Neg Risk CTF Exchange

// Derive the Polymarket Safe proxy wallet address from an EOA
function deriveSafeWallet(eoaAddress: Address): Address {
  const salt = keccak256(encodeAbiParameters([{ type: 'address' }], [eoaAddress]))
  return getCreate2Address({
    from: SAFE_FACTORY,
    salt,
    bytecodeHash: SAFE_INIT_CODE_HASH,
  })
}

interface ApiCreds {
  key: string
  secret: string
  passphrase: string
}

export interface OrderParams {
  tokenId: string
  side: 'buy' | 'sell'
  size: number  // For buy: USDC amount, For sell: number of shares
  price: number // 0-1
}

export interface AllowanceStatus {
  hasAllowance: boolean
  usdcApproved: boolean
  ctfApproved: boolean
}

export function useClobClient() {
  const { data: walletClient } = useWalletClient()
  const chainId = useChainId()
  const { address, sessionToken } = useWalletStore()

  const [isInitializing, setIsInitializing] = useState(false)
  const [isPlacingOrder, setIsPlacingOrder] = useState(false)
  const [isCheckingAllowance, setIsCheckingAllowance] = useState(false)
  const [isApproving, setIsApproving] = useState(false)
  const [allowanceStatus, setAllowanceStatus] = useState<AllowanceStatus | null>(null)
  const [error, setError] = useState<string | null>(null)

  // Cache the initialized client
  const clientRef = useRef<ClobClient | null>(null)
  const credsRef = useRef<ApiCreds | null>(null)

  // Clear cached client when address changes or disconnects
  useEffect(() => {
    clientRef.current = null
    credsRef.current = null
    setAllowanceStatus(null)
    setError(null)
  }, [address])

  // Initialize the CLOB client with user's wallet
  const initializeClient = useCallback(async (): Promise<ClobClient | null> => {
    // Return cached client if available
    if (clientRef.current && credsRef.current) {
      return clientRef.current
    }

    // console.log('initializeClient called - walletClient:', !!walletClient, 'address:', address, 'chainId:', chainId)

    if (!address) {
      setError('Wallet not connected - no address')
      return null
    }

    // walletClient might be undefined initially, but we can still proceed with window.ethereum
    if (!window.ethereum) {
      setError('No wallet provider found')
      return null
    }

    if (chainId !== POLYGON_CHAIN_ID) {
      setError('Please switch to Polygon network')
      return null
    }

    setIsInitializing(true)
    setError(null)

    try {
      // Create ethers provider from window.ethereum
      if (!window.ethereum) {
        throw new Error('No ethereum provider found')
      }

      const provider = new ethers.providers.Web3Provider(window.ethereum as ethers.providers.ExternalProvider)
      const signer = provider.getSigner()

      // Derive the Safe proxy address (funder) from the EOA
      const funderAddress = deriveSafeWallet(address as Address)
      // console.log('Creating CLOB client - EOA:', address, '-> Safe (funder):', funderAddress)

      // Create temporary client to derive API credentials
      // Note: For API key derivation, we don't need the funder yet
      const tempClient = new ClobClient(
        CLOB_HOST,
        POLYGON_CHAIN_ID,
        signer
      )

      console.log('Deriving API credentials...')

      // Try to derive existing credentials, or create new ones
      let creds: ApiCreds | null = null

      // First try to derive existing credentials
      try {
        console.log('Attempting to derive existing API key...')
        creds = await tempClient.deriveApiKey() as ApiCreds
        console.log('Successfully derived existing API credentials')
      } catch (deriveError: any) {
        console.log('Derive API key failed (expected for new wallets):', deriveError?.message || deriveError)
      }

      // If derive failed, create new credentials
      if (!creds || !creds.key) {
        try {
          console.log('Creating new API credentials...')
          creds = await tempClient.createApiKey() as ApiCreds
          console.log('Successfully created new API credentials')
        } catch (createError: any) {
          console.error('Failed to create API key:', createError)
          throw new Error('Failed to create API credentials. Please try again.')
        }
      }

      if (!creds || !creds.key) {
        throw new Error('Could not obtain API credentials')
      }

      credsRef.current = creds
      console.log('Got API credentials:', { key: creds.key?.slice(0, 8) + '...' })

      // Create authenticated client with credentials AND funder address
      // Use signature type 2 for browser wallet with Safe proxy
      // The funder is the Safe proxy that holds the funds
      const client = new ClobClient(
        CLOB_HOST,
        POLYGON_CHAIN_ID,
        signer,
        creds,
        SIGNATURE_TYPE_POLY_GNOSIS_SAFE,
        funderAddress  // This is the Safe proxy address
      )

      clientRef.current = client
      console.log('CLOB client initialized successfully')

      return client
    } catch (err) {
      console.error('Failed to initialize CLOB client:', err)
      setError(err instanceof Error ? err.message : 'Failed to initialize trading client')
      return null
    } finally {
      setIsInitializing(false)
    }
  }, [walletClient, address, chainId, sessionToken])

  // Place a limit order using the SDK (GTC - Good Till Cancelled)
  const placeOrder = useCallback(async (params: OrderParams): Promise<string | null> => {
    setIsPlacingOrder(true)
    setError(null)

    try {
      const client = await initializeClient()
      if (!client) {
        return null
      }

      // console.log('Creating limit order:', params)

      // Create the order using the SDK
      const order = await client.createOrder({
        tokenID: params.tokenId,
        side: params.side === 'buy' ? Side.BUY : Side.SELL,
        size: params.size,
        price: params.price,
      })

      // console.log('Order created, submitting...', order)

      // Submit the order
      const response = await client.postOrder(order)

      // console.log('Order response:', response)

      if (response.success) {
        return response.orderID || 'submitted'
      } else {
        throw new Error(response.errorMsg || 'Order submission failed')
      }
    } catch (err) {
      console.error('Order failed:', err)
      setError(err instanceof Error ? err.message : 'Order failed')
      return null
    } finally {
      setIsPlacingOrder(false)
    }
  }, [initializeClient])

  // Place a limit order via backend (handles L2 auth for HTTP contexts)
  const placeLimitOrder = useCallback(async (params: OrderParams): Promise<string | null> => {
    setIsPlacingOrder(true)
    setError(null)

    try {
      const client = await initializeClient()
      if (!client) {
        return null
      }

      const creds = credsRef.current
      if (!creds || !creds.key || !creds.secret || !creds.passphrase) {
        throw new Error('No API credentials available. Please try reconnecting your wallet.')
      }

      console.log('Creating limit order with params:', params)

      // Create the signed limit order using the SDK (this handles EIP-712 signing)
      const signedOrder = await client.createOrder({
        tokenID: params.tokenId,
        side: params.side === 'buy' ? Side.BUY : Side.SELL,
        size: params.size,
        price: params.price,
      })

      console.log('Signed limit order created:', JSON.stringify(signedOrder, null, 2))

      // Submit via backend (which handles L2 HMAC auth)
      const response = await fetch('/api/trades/submit-order', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${sessionToken}`,
        },
        body: JSON.stringify({
          signed_order: signedOrder,
          api_key: creds.key,
          api_secret: creds.secret,
          api_passphrase: creds.passphrase,
          order_type: 'GTC', // Good Till Cancelled for limit orders
        }),
      })

      if (!response.ok) {
        const errData = await response.json().catch(() => ({}))
        throw new Error(errData.error || `Submission failed: ${response.status}`)
      }

      const result = await response.json()
      console.log('Limit order response:', result)

      if (result.success || result.order_id) {
        return result.order_id || 'submitted'
      } else {
        throw new Error(result.error || 'Limit order submission failed')
      }
    } catch (err) {
      console.error('Limit order failed:', err)
      setError(err instanceof Error ? err.message : 'Limit order failed')
      return null
    } finally {
      setIsPlacingOrder(false)
    }
  }, [initializeClient, sessionToken])

  // Create a market order (FOK)
  // Note: Due to crypto.subtle not being available over HTTP, we create the order
  // with the SDK but submit via our backend which handles L2 authentication
  const placeMarketOrder = useCallback(async (params: OrderParams): Promise<string | null> => {
    setIsPlacingOrder(true)
    setError(null)

    try {
      const client = await initializeClient()
      if (!client) {
        return null
      }

      const creds = credsRef.current
      if (!creds || !creds.key || !creds.secret || !creds.passphrase) {
        throw new Error('No API credentials available. Please try reconnecting your wallet.')
      }

      console.log('Creating market order with creds:', { key: creds.key?.slice(0, 8) + '...' })

      // Debug: Check allowance before placing order
      const proxyAddress = deriveSafeWallet(address as Address)
      console.log('Checking allowance for proxy:', proxyAddress)

      // Make individual RPC calls (batch requests not reliably supported)
      const [ctfResult, balanceResult, negRiskResult] = await Promise.all([
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 1,
          method: 'eth_call',
          params: [{
            to: USDC_E_ADDRESS,
            data: `0xdd62ed3e000000000000000000000000${proxyAddress.slice(2)}000000000000000000000000${CTF_EXCHANGE.slice(2)}`,
          }, 'latest'],
        }),
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 2,
          method: 'eth_call',
          params: [{
            to: USDC_E_ADDRESS,
            data: `0x70a08231000000000000000000000000${proxyAddress.slice(2)}`,
          }, 'latest'],
        }),
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 3,
          method: 'eth_call',
          params: [{
            to: USDC_E_ADDRESS,
            data: `0xdd62ed3e000000000000000000000000${proxyAddress.slice(2)}000000000000000000000000${NEG_RISK_CTF_EXCHANGE.slice(2)}`,
          }, 'latest'],
        }),
      ])
      const ctfAllowance = ctfResult?.result ? BigInt(ctfResult.result) : 0n
      const usdcBalance = balanceResult?.result ? BigInt(balanceResult.result) : 0n
      const negRiskAllowance = negRiskResult?.result ? BigInt(negRiskResult.result) : 0n
      console.log('CTF Exchange allowance:', ctfAllowance.toString())
      console.log('Neg Risk Exchange allowance:', negRiskAllowance.toString())
      console.log('USDC balance in proxy:', usdcBalance.toString(), '(', Number(usdcBalance) / 1e6, 'USDC )')

      if (ctfAllowance === 0n) {
        throw new Error('No USDC allowance set for CTF Exchange. Please activate your trading wallet first.')
      }
      if (usdcBalance === 0n) {
        throw new Error('No USDC balance in trading wallet. Please deposit funds first.')
      }

      // Create market order using the SDK (this handles EIP-712 signing)
      console.log('Creating order with params:', params)
      const signedOrder = await client.createMarketOrder({
        tokenID: params.tokenId,
        side: params.side === 'buy' ? Side.BUY : Side.SELL,
        amount: params.size,
      })
      console.log('Signed order created:', JSON.stringify(signedOrder, null, 2))
      console.log('Order maker:', signedOrder.maker, '(should match proxy:', proxyAddress, ')')

      // console.log('Market order created:', signedOrder)

      // Submit via backend (which handles L2 HMAC auth since crypto.subtle isn't available over HTTP)
      const response = await fetch('/api/trades/submit-order', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${sessionToken}`,
        },
        body: JSON.stringify({
          signed_order: signedOrder,
          api_key: creds.key,
          api_secret: creds.secret,
          api_passphrase: creds.passphrase,
          order_type: 'FOK',
        }),
      })

      if (!response.ok) {
        const errData = await response.json().catch(() => ({}))
        throw new Error(errData.error || `Submission failed: ${response.status}`)
      }

      const result = await response.json()
      // console.log('Order response:', result)

      if (result.success || result.order_id) {
        return result.order_id || 'submitted'
      } else {
        throw new Error(result.error || 'Order submission failed')
      }
    } catch (err) {
      console.error('Market order failed:', err)
      setError(err instanceof Error ? err.message : 'Market order failed')
      return null
    } finally {
      setIsPlacingOrder(false)
    }
  }, [initializeClient, sessionToken])

  // Check if the trading wallet has the necessary allowances
  const checkAllowance = useCallback(async (): Promise<AllowanceStatus | null> => {
    if (!address) {
      return null
    }

    setIsCheckingAllowance(true)
    setError(null)

    try {
      const proxyAddress = deriveSafeWallet(address as Address)

      // Check USDC allowance for both exchanges (individual requests for reliability)
      const [ctfResult, negRiskResult] = await Promise.all([
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 1,
          method: 'eth_call',
          params: [{
            to: USDC_E_ADDRESS,
            // allowance(owner, spender) for CTF Exchange
            data: `0xdd62ed3e000000000000000000000000${proxyAddress.slice(2)}000000000000000000000000${CTF_EXCHANGE.slice(2)}`,
          }, 'latest'],
        }),
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 2,
          method: 'eth_call',
          params: [{
            to: USDC_E_ADDRESS,
            // allowance(owner, spender) for Neg Risk CTF Exchange
            data: `0xdd62ed3e000000000000000000000000${proxyAddress.slice(2)}000000000000000000000000${NEG_RISK_CTF_EXCHANGE.slice(2)}`,
          }, 'latest'],
        }),
      ])

      // Check if allowances are sufficient (> 0 means approved)
      const ctfAllowance = ctfResult?.result ? BigInt(ctfResult.result) : 0n
      const negRiskAllowance = negRiskResult?.result ? BigInt(negRiskResult.result) : 0n

      // Consider approved if allowance is greater than 1000 USDC (reasonable threshold)
      const threshold = BigInt(1000 * 1e6) // 1000 USDC
      const usdcApproved = ctfAllowance > threshold && negRiskAllowance > threshold

      const status: AllowanceStatus = {
        hasAllowance: usdcApproved,
        usdcApproved,
        ctfApproved: true, // CTF tokens are typically pre-approved
      }

      setAllowanceStatus(status)
      return status
    } catch (err) {
      console.error('Failed to check allowance:', err)
      setError('Failed to check trading allowance')
      return null
    } finally {
      setIsCheckingAllowance(false)
    }
  }, [address])

  // Enable trading by approving USDC for the exchange contracts
  // Routes through backend to handle L2 HMAC auth (crypto.subtle not available over HTTP)
  const enableTrading = useCallback(async (): Promise<boolean> => {
    setIsApproving(true)
    setError(null)

    try {
      // First, ensure we have API credentials
      const client = await initializeClient()
      if (!client) {
        return false
      }

      const creds = credsRef.current
      if (!creds) {
        setError('No API credentials available')
        return false
      }

      // console.log('Enabling trading via backend...')

      // Call our backend which handles L2 auth
      const response = await fetch('/api/trades/enable', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${sessionToken}`,
        },
        body: JSON.stringify({
          api_key: creds.key,
          api_secret: creds.secret,
          api_passphrase: creds.passphrase,
        }),
      })

      if (!response.ok) {
        const errData = await response.json().catch(() => ({}))
        throw new Error(errData.error || `Enable trading failed: ${response.status}`)
      }

      const result = await response.json()
      // console.log('Enable trading response:', result)

      if (!result.success) {
        throw new Error(result.error || 'Failed to enable trading')
      }

      // console.log('Allowances set successfully')

      // Recheck allowance status
      await checkAllowance()

      return true
    } catch (err) {
      console.error('Failed to enable trading:', err)
      setError(err instanceof Error ? err.message : 'Failed to enable trading')
      return false
    } finally {
      setIsApproving(false)
    }
  }, [initializeClient, checkAllowance, sessionToken])

  // Clear cached client (e.g., on wallet disconnect)
  const clearClient = useCallback(() => {
    clientRef.current = null
    credsRef.current = null
    setAllowanceStatus(null)
  }, [])

  return {
    initializeClient,
    placeOrder,
    placeMarketOrder,
    placeLimitOrder,
    checkAllowance,
    enableTrading,
    clearClient,
    isInitializing,
    isPlacingOrder,
    isCheckingAllowance,
    isApproving,
    allowanceStatus,
    error,
    clearError: () => setError(null),
    hasClient: !!clientRef.current,
  }
}
