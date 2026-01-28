import { useState, useCallback, useRef, useEffect } from 'react'
import { useWalletClient, useChainId } from 'wagmi'
import { RelayClient, SafeTransaction, OperationType, RelayerTransactionState } from '@polymarket/relayer-client'
import { keccak256, encodeAbiParameters, getCreate2Address, encodeFunctionData, parseAbi } from 'viem'
import type { Address } from 'viem'
import { useWalletStore } from '../stores/walletStore'
import { rpcCallWithRetry } from '../utils/rpc'

// Safe Proxy Factory for deriving proxy wallet address
const SAFE_FACTORY = '0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b' as const
const SAFE_INIT_CODE_HASH = '0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf' as const

const POLYGON_CHAIN_ID = 137
// Use our backend proxy for relay calls - it adds builder auth headers
const RELAY_PROXY_URL = '/api/relay'

// Contract addresses on Polygon
const USDC_E = '0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174' as const // USDC.e (bridged)
const CTF = '0x4D97DCd97eC945f40cF65F87097ACe5EA0476045' as const // Conditional Token Framework
const CTF_EXCHANGE = '0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E' as const
const NEG_RISK_CTF_EXCHANGE = '0xC5d563A36AE78145C45a50134d48A1215220f80a' as const
const NEG_RISK_ADAPTER = '0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296' as const

// Max uint256 for unlimited approval
const MAX_APPROVAL = '115792089237316195423570985008687907853269984665640564039457584007913129639935'

// Derive the Polymarket Safe proxy wallet address from an EOA
function deriveSafeWallet(eoaAddress: Address): Address {
  const salt = keccak256(encodeAbiParameters([{ type: 'address' }], [eoaAddress]))
  return getCreate2Address({
    from: SAFE_FACTORY,
    salt,
    bytecodeHash: SAFE_INIT_CODE_HASH,
  })
}

// Build ERC20 approve transaction data
function buildApproveData(spender: Address, amount: string): string {
  return encodeFunctionData({
    abi: parseAbi(['function approve(address spender, uint256 amount) returns (bool)']),
    functionName: 'approve',
    args: [spender, BigInt(amount)],
  })
}

// Build ERC1155 setApprovalForAll transaction data
function buildSetApprovalForAllData(operator: Address, approved: boolean): string {
  return encodeFunctionData({
    abi: parseAbi(['function setApprovalForAll(address operator, bool approved)']),
    functionName: 'setApprovalForAll',
    args: [operator, approved],
  })
}

export interface ActivationStatus {
  isDeployed: boolean
  hasAllowances: boolean // USDC.e allowance for buying
  hasCtfApproval: boolean // CTF approval for selling
}


// Check if Safe is deployed via relay API (through our backend proxy)
async function checkSafeDeployed(safeAddress: string): Promise<boolean> {
  try {
    // Use our backend proxy which adds builder auth headers
    const response = await fetch(`${RELAY_PROXY_URL}/deployed?address=${safeAddress}`, {
      method: 'GET',
    })

    if (!response.ok) {
      console.error('Deployment check failed:', await response.text())
      return false
    }

    const data = await response.json()
    return data.deployed === true
  } catch (err) {
    console.error('Failed to check deployment:', err)
    return false
  }
}

// Check USDC.e allowance for a spender (with retry/fallback)
async function checkAllowance(owner: string, spender: string): Promise<bigint> {
  try {
    // allowance(address,address) selector = 0xdd62ed3e
    const data = encodeFunctionData({
      abi: parseAbi(['function allowance(address owner, address spender) view returns (uint256)']),
      functionName: 'allowance',
      args: [owner as Address, spender as Address],
    })

    const result = await rpcCallWithRetry({
      jsonrpc: '2.0',
      id: 1,
      method: 'eth_call',
      params: [{ to: USDC_E, data }, 'latest'],
    })

    if (result.result) {
      return BigInt(result.result)
    }
    return BigInt(0)
  } catch (err) {
    console.error('Failed to check allowance:', err)
    return BigInt(0)
  }
}

// Check CTF (ERC1155) approval for a spender (with retry/fallback)
async function checkCtfApproval(owner: string, operator: string): Promise<boolean> {
  try {
    const data = encodeFunctionData({
      abi: parseAbi(['function isApprovedForAll(address account, address operator) view returns (bool)']),
      functionName: 'isApprovedForAll',
      args: [owner as Address, operator as Address],
    })

    const result = await rpcCallWithRetry({
      jsonrpc: '2.0',
      id: 1,
      method: 'eth_call',
      params: [{ to: CTF, data }, 'latest'],
    })

    if (result.result) {
      // Returns 0x...01 for true, 0x...00 for false
      return result.result !== '0x0000000000000000000000000000000000000000000000000000000000000000'
    }
    return false
  } catch (err) {
    console.error('Failed to check CTF approval:', err)
    return false
  }
}

export function useActivation() {
  const { data: walletClient } = useWalletClient()
  const chainId = useChainId()
  const { address } = useWalletStore()

  const [isChecking, setIsChecking] = useState(false)
  const [isActivating, setIsActivating] = useState(false)
  const [status, setStatus] = useState<ActivationStatus | null>(null)
  const [error, setError] = useState<string | null>(null)

  const relayClientRef = useRef<RelayClient | null>(null)

  // Clear cached client when address changes or disconnects
  useEffect(() => {
    relayClientRef.current = null
    setStatus(null)
    setError(null)
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
      // console.log('Initializing RelayClient with viem WalletClient...')
      // console.log('Using relay proxy URL:', RELAY_PROXY_URL)

      // Create relay client pointing to our backend proxy
      // Our proxy adds builder auth headers before forwarding to Polymarket's relay
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

  // Check activation status
  const checkStatus = useCallback(async (): Promise<ActivationStatus | null> => {
    if (!address) return null

    setIsChecking(true)
    setError(null)

    try {
      const safeAddress = deriveSafeWallet(address as Address)
      // console.log('Checking deployment for Safe:', safeAddress)

      const isDeployed = await checkSafeDeployed(safeAddress)

      // Check if allowances are set
      let hasAllowances = false
      let hasCtfApproval = false
      if (isDeployed) {
        // Check USDC.e allowance for buying
        const allowance = await checkAllowance(safeAddress, CTF_EXCHANGE)
        hasAllowances = allowance > BigInt(0)

        // Check CTF approval for selling
        hasCtfApproval = await checkCtfApproval(safeAddress, CTF_EXCHANGE)
      }

      const result = {
        isDeployed,
        hasAllowances,
        hasCtfApproval,
      }

      // console.log('Activation status:', result)
      setStatus(result)
      return result
    } catch (err) {
      console.error('Failed to check status:', err)
      setError(err instanceof Error ? err.message : 'Failed to check activation status')
      return null
    } finally {
      setIsChecking(false)
    }
  }, [address])

  // Activate wallet (deploy Safe and set allowances)
  const activate = useCallback(async (): Promise<boolean> => {
    if (!address || !walletClient) {
      setError('Wallet not connected')
      return false
    }

    if (chainId !== POLYGON_CHAIN_ID) {
      setError('Please switch to Polygon network')
      return false
    }

    setIsActivating(true)
    setError(null)

    try {
      // Clear cached client to ensure fresh nonce
      relayClientRef.current = null

      const client = await initializeRelayClient()
      if (!client) return false

      const safeAddress = deriveSafeWallet(address as Address)
      // console.log('Activating wallet - EOA:', address, 'Safe:', safeAddress)

      // Step 1: Check if already deployed
      let isDeployed = await checkSafeDeployed(safeAddress)

      if (!isDeployed) {
        // console.log('Safe not deployed, deploying...')

        // Deploy the Safe
        const deployResponse = await client.deploySafe()
        // console.log('Deploy response:', deployResponse)

        // Try to wait for deployment, but don't fail if polling doesn't work
        // (Polymarket's relay doesn't expose the transaction status endpoint publicly)
        try {
          const deployResult = await deployResponse.wait()
          // console.log('Deploy result:', deployResult)

          if (deployResult?.state === RelayerTransactionState.STATE_FAILED) {
            throw new Error('Safe deployment failed')
          }
        } catch (waitErr) {
          // console.log('Transaction status polling failed (expected), checking deployment directly...')
          // Poll deployment status directly instead
          for (let i = 0; i < 30; i++) {
            await new Promise(resolve => setTimeout(resolve, 2000))
            const deployed = await checkSafeDeployed(safeAddress)
            // console.log(`Deployment check ${i + 1}/30:`, deployed)
            if (deployed) {
              break
            }
          }
        }

        // Verify deployment succeeded
        isDeployed = await checkSafeDeployed(safeAddress)
        if (!isDeployed) {
          throw new Error('Safe deployment may still be pending. Please try again in a moment.')
        }

        // Add delay after deployment before setting approvals
        console.log('Safe deployed, waiting 10s before setting approvals...')
        await new Promise(resolve => setTimeout(resolve, 10000))
      }

      // Step 2: Set all approvals in one batch (single signature)
      console.log('Setting all token approvals...')

      const approvalTxs: SafeTransaction[] = [
        // USDC.e approvals
        {
          to: USDC_E,
          value: '0',
          data: buildApproveData(CTF_EXCHANGE, MAX_APPROVAL),
          operation: OperationType.Call,
        },
        {
          to: USDC_E,
          value: '0',
          data: buildApproveData(NEG_RISK_CTF_EXCHANGE, MAX_APPROVAL),
          operation: OperationType.Call,
        },
        // CTF (ERC1155) approvals
        {
          to: CTF,
          value: '0',
          data: buildSetApprovalForAllData(CTF_EXCHANGE, true),
          operation: OperationType.Call,
        },
        {
          to: CTF,
          value: '0',
          data: buildSetApprovalForAllData(NEG_RISK_CTF_EXCHANGE, true),
          operation: OperationType.Call,
        },
        {
          to: CTF,
          value: '0',
          data: buildSetApprovalForAllData(NEG_RISK_ADAPTER, true),
          operation: OperationType.Call,
        },
      ]

      const approvalResponse = await client.executeSafeTransactions(
        approvalTxs,
        'Set all token approvals for trading'
      )
      console.log('Approval transaction submitted, waiting for confirmation...')

      // Try to wait via relay, but it usually fails with 404
      try {
        const approvalResult = await approvalResponse.wait()
        if (approvalResult?.state === RelayerTransactionState.STATE_FAILED) {
          throw new Error('Approval transactions failed')
        }
      } catch (waitErr) {
        // Polling failed (expected), poll allowance directly with longer timeout
        console.log('Polling allowance on-chain...')
      }

      // Poll for allowance with extended timeout (up to 2 minutes)
      let confirmed = false
      for (let i = 0; i < 40; i++) {
        await new Promise(resolve => setTimeout(resolve, 3000))
        const allowance = await checkAllowance(safeAddress, CTF_EXCHANGE)
        console.log(`Allowance check ${i + 1}/40:`, allowance.toString())
        if (allowance > BigInt(0)) {
          console.log('Allowance confirmed!')
          confirmed = true
          break
        }
      }

      if (!confirmed) {
        throw new Error('Approval transaction may still be pending. Please wait a moment and try trading.')
      }

      console.log('All approvals complete!')

      // Update status
      setStatus({
        isDeployed: true,
        hasAllowances: true,
        hasCtfApproval: true,
      })

      return true
    } catch (err) {
      console.error('Activation failed:', err)
      setError(err instanceof Error ? err.message : 'Activation failed')
      return false
    } finally {
      setIsActivating(false)
    }
  }, [address, walletClient, chainId, initializeRelayClient])

  // Clear client on disconnect
  const clearClient = useCallback(() => {
    relayClientRef.current = null
    setStatus(null)
  }, [])

  return {
    checkStatus,
    activate,
    clearClient,
    isChecking,
    isActivating,
    status,
    error,
    clearError: () => setError(null),
    safeAddress: address ? deriveSafeWallet(address as Address) : null,
  }
}
