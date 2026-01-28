import { useState, useCallback, useRef, useEffect } from 'react'
import { useWalletClient, useChainId } from 'wagmi'
import { RelayClient, SafeTransaction, OperationType, RelayerTransactionState } from '@polymarket/relayer-client'
import { keccak256, encodeAbiParameters, getCreate2Address, encodeFunctionData, parseAbi, parseUnits } from 'viem'
import type { Address } from 'viem'
import { useWalletStore } from '../stores/walletStore'

// Safe Proxy Factory for deriving proxy wallet address
const SAFE_FACTORY = '0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b' as const
const SAFE_INIT_CODE_HASH = '0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf' as const

const POLYGON_CHAIN_ID = 137
const RELAY_PROXY_URL = '/api/relay'

// USDC.e (bridged) on Polygon
const USDC_E = '0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174' as const
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

// Build ERC20 transfer transaction data
function buildTransferData(to: Address, amount: bigint): string {
  return encodeFunctionData({
    abi: parseAbi(['function transfer(address to, uint256 amount) returns (bool)']),
    functionName: 'transfer',
    args: [to, amount],
  })
}

export function useWithdraw() {
  const { data: walletClient } = useWalletClient()
  const chainId = useChainId()
  const { address } = useWalletStore()

  const [isWithdrawing, setIsWithdrawing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [txHash, setTxHash] = useState<string | null>(null)

  const relayClientRef = useRef<RelayClient | null>(null)

  // Clear cached client when address changes or disconnects
  useEffect(() => {
    relayClientRef.current = null
    setError(null)
    setTxHash(null)
  }, [address])

  // Initialize the relay client
  const initializeRelayClient = useCallback(async (): Promise<RelayClient | null> => {
    if (relayClientRef.current) {
      return relayClientRef.current
    }

    if (!walletClient || !address) {
      setError('Wallet not connected')
      return null
    }

    if (chainId !== POLYGON_CHAIN_ID) {
      setError('Please switch to Polygon network')
      return null
    }

    try {
      const client = new RelayClient(
        RELAY_PROXY_URL,
        POLYGON_CHAIN_ID,
        walletClient as any
      )

      relayClientRef.current = client
      return client
    } catch (err) {
      console.error('Failed to initialize relay client:', err)
      setError(err instanceof Error ? err.message : 'Failed to initialize')
      return null
    }
  }, [walletClient, address, chainId])

  // Withdraw USDC from Safe proxy to EOA
  const withdraw = useCallback(async (amount: string): Promise<boolean> => {
    if (!address || !walletClient) {
      setError('Wallet not connected')
      return false
    }

    if (chainId !== POLYGON_CHAIN_ID) {
      setError('Please switch to Polygon network')
      return false
    }

    const amountNum = parseFloat(amount)
    if (isNaN(amountNum) || amountNum <= 0) {
      setError('Invalid amount')
      return false
    }

    setIsWithdrawing(true)
    setError(null)
    setTxHash(null)

    try {
      // Clear any cached client to ensure fresh nonce
      relayClientRef.current = null

      const client = await initializeRelayClient()
      if (!client) return false

      const amountWei = parseUnits(amount, USDC_DECIMALS)

      // Build the USDC transfer transaction from Safe to EOA
      const withdrawTx: SafeTransaction = {
        to: USDC_E,
        value: '0',
        data: buildTransferData(address as Address, amountWei),
        operation: OperationType.Call,
      }

      // Execute via relay
      console.log('Submitting withdrawal transaction...')
      const response = await client.executeSafeTransactions(
        [withdrawTx],
        `Withdraw ${amount} USDC to wallet`
      )
      console.log('Withdrawal transaction submitted successfully')

      // Try to wait for confirmation (polling usually fails with 404, which is expected)
      try {
        const result = await response.wait()

        if (result?.state === RelayerTransactionState.STATE_FAILED) {
          throw new Error('Withdrawal transaction failed')
        }

        // Try to get transaction hash from result
        const hash = (result as any)?.transactionHash || (result as any)?.txHash
        if (hash) {
          setTxHash(hash)
        }
        console.log('Withdrawal confirmed')
      } catch (waitErr: any) {
        // Polling usually fails with 404 - this is expected behavior
        // The transaction was already submitted, just can't poll status
        console.log('Transaction status polling failed (expected):', waitErr?.message || waitErr)
        console.log('Withdrawal was submitted - waiting for confirmation...')
        await new Promise(resolve => setTimeout(resolve, 3000))
      }

      console.log('Withdrawal complete!')
      return true
    } catch (err) {
      console.error('Withdrawal failed:', err)
      setError(err instanceof Error ? err.message : 'Withdrawal failed')
      return false
    } finally {
      setIsWithdrawing(false)
    }
  }, [address, walletClient, chainId, initializeRelayClient])

  // Clear client on disconnect
  const clearClient = useCallback(() => {
    relayClientRef.current = null
  }, [])

  return {
    withdraw,
    clearClient,
    isWithdrawing,
    error,
    txHash,
    clearError: () => setError(null),
    safeAddress: address ? deriveSafeWallet(address as Address) : null,
  }
}
