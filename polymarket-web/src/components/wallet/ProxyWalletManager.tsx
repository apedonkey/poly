import { useState, useEffect, useCallback } from 'react'
import { Wallet, ArrowDownToLine, ArrowUpFromLine, Copy, ExternalLink, RefreshCw, CheckCircle } from 'lucide-react'
import { useAccount, useChainId, useSwitchChain, useWriteContract, useWaitForTransactionReceipt } from 'wagmi'
import { parseUnits, formatUnits, type Address } from 'viem'
import { keccak256, encodeAbiParameters, getCreate2Address } from 'viem'
import { Modal } from '../Modal'
import { useWithdraw } from '../../hooks/useWithdraw'
import { rpcCallWithRetry } from '../../utils/rpc'

// USDC.e (bridged) on Polygon - used by Polymarket
const USDC_ADDRESS = '0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174' as const
const USDC_DECIMALS = 6

// Safe Proxy Factory for deriving proxy wallet address
const SAFE_FACTORY = '0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b' as const
const SAFE_INIT_CODE_HASH = '0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf' as const

// ERC20 ABI for transfer
const ERC20_ABI = [
  {
    name: 'transfer',
    type: 'function',
    inputs: [
      { name: 'to', type: 'address' },
      { name: 'amount', type: 'uint256' },
    ],
    outputs: [{ name: '', type: 'bool' }],
  },
  {
    name: 'balanceOf',
    type: 'function',
    inputs: [{ name: 'account', type: 'address' }],
    outputs: [{ name: '', type: 'uint256' }],
    stateMutability: 'view',
  },
] as const

// Derive the Polymarket Safe proxy wallet address from an EOA
function deriveSafeWallet(eoaAddress: Address): Address {
  const salt = keccak256(encodeAbiParameters([{ type: 'address' }], [eoaAddress]))
  return getCreate2Address({
    from: SAFE_FACTORY,
    salt,
    bytecodeHash: SAFE_INIT_CODE_HASH,
  })
}

interface ProxyWalletManagerProps {
  isOpen: boolean
  onClose: () => void
}

export function ProxyWalletManager({ isOpen, onClose }: ProxyWalletManagerProps) {
  const { address: eoaAddress } = useAccount()
  const chainId = useChainId()
  const { switchChain } = useSwitchChain()

  const [proxyAddress, setProxyAddress] = useState<Address | null>(null)
  const [eoaBalance, setEoaBalance] = useState<string>('0')
  const [proxyBalance, setProxyBalance] = useState<string>('0')
  const [isLoadingBalances, setIsLoadingBalances] = useState(false)
  const [amount, setAmount] = useState('')
  const [mode, setMode] = useState<'deposit' | 'withdraw'>('deposit')
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [withdrawSuccess, setWithdrawSuccess] = useState(false)

  // Contract write for deposits
  const { writeContract, data: txHash, isPending: isWriting } = useWriteContract()
  const { isLoading: isConfirming, isSuccess: isConfirmed } = useWaitForTransactionReceipt({
    hash: txHash,
  })

  // Withdrawal hook
  const { withdraw, isWithdrawing, error: withdrawError } = useWithdraw()

  // Derive proxy address when EOA changes
  useEffect(() => {
    if (eoaAddress) {
      const proxy = deriveSafeWallet(eoaAddress)
      setProxyAddress(proxy)
    }
  }, [eoaAddress])

  // Fetch balances
  const fetchBalances = useCallback(async () => {
    if (!eoaAddress || !proxyAddress) {
      // console.log('ProxyWalletManager: Cannot fetch - eoaAddress:', eoaAddress, 'proxyAddress:', proxyAddress)
      return
    }

    // console.log('ProxyWalletManager: Fetching balances for EOA:', eoaAddress, 'Proxy:', proxyAddress)
    // console.log('ProxyWalletManager: Using USDC contract:', USDC_ADDRESS)

    setIsLoadingBalances(true)
    try {
      // Fetch both balances via RPC (individual requests for reliability)
      const [eoaResult, proxyResult] = await Promise.all([
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 1,
          method: 'eth_call',
          params: [{
            to: USDC_ADDRESS,
            data: `0x70a08231000000000000000000000000${eoaAddress.slice(2)}`,
          }, 'latest'],
        }),
        rpcCallWithRetry({
          jsonrpc: '2.0',
          id: 2,
          method: 'eth_call',
          params: [{
            to: USDC_ADDRESS,
            data: `0x70a08231000000000000000000000000${proxyAddress.slice(2)}`,
          }, 'latest'],
        }),
      ])

      if (eoaResult?.result) {
        const eoaBal = BigInt(eoaResult.result)
        setEoaBalance(formatUnits(eoaBal, USDC_DECIMALS))
      }
      if (proxyResult?.result) {
        const proxyBal = BigInt(proxyResult.result)
        setProxyBalance(formatUnits(proxyBal, USDC_DECIMALS))
      }
    } catch (err) {
      console.error('Failed to fetch balances:', err)
    } finally {
      setIsLoadingBalances(false)
    }
  }, [eoaAddress, proxyAddress])

  // Fetch balances on mount, when addresses change, or when modal opens
  useEffect(() => {
    if (isOpen) {
      fetchBalances()
    }
  }, [fetchBalances, isOpen])

  // Refetch after transaction confirms
  useEffect(() => {
    if (isConfirmed) {
      fetchBalances()
      setAmount('')
    }
  }, [isConfirmed, fetchBalances])

  // Clear success states when switching modes
  useEffect(() => {
    setWithdrawSuccess(false)
    setError(null)
  }, [mode])

  const handleCopyAddress = async () => {
    if (proxyAddress) {
      await navigator.clipboard.writeText(proxyAddress)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    }
  }

  const handleDeposit = async () => {
    if (!proxyAddress || !amount) return
    setError(null)

    if (chainId !== 137) {
      try {
        await switchChain({ chainId: 137 })
      } catch {
        setError('Please switch to Polygon network')
        return
      }
    }

    try {
      const amountWei = parseUnits(amount, USDC_DECIMALS)

      writeContract({
        address: USDC_ADDRESS,
        abi: ERC20_ABI,
        functionName: 'transfer',
        args: [proxyAddress, amountWei],
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Deposit failed')
    }
  }

  const handleWithdraw = async () => {
    if (!amount) return
    setError(null)
    setWithdrawSuccess(false)

    if (chainId !== 137) {
      try {
        await switchChain({ chainId: 137 })
      } catch {
        setError('Please switch to Polygon network')
        return
      }
    }

    const success = await withdraw(amount)
    if (success) {
      setWithdrawSuccess(true)
      setAmount('')
      // Refresh balances after withdrawal
      setTimeout(() => fetchBalances(), 2000)
    }
  }

  const handleMaxAmount = () => {
    if (mode === 'deposit') {
      setAmount(eoaBalance)
    } else {
      setAmount(proxyBalance)
    }
  }

  const isProcessing = isWriting || isConfirming || isWithdrawing
  const needsNetworkSwitch = chainId !== 137

  // Combine errors from deposit and withdraw
  const displayError = error || withdrawError

  return (
    <Modal isOpen={isOpen} onClose={onClose} title="Trading Wallet">
      <div className="space-y-4">
        {/* Proxy wallet info */}
        <div className="bg-poly-dark rounded-lg p-3 sm:p-4 border border-poly-border">
          <div className="flex items-center justify-between mb-2">
            <div className="flex items-center gap-2">
              <Wallet className="w-4 h-4 text-poly-green flex-shrink-0" />
              <span className="text-xs sm:text-sm text-gray-400">Trading Wallet</span>
            </div>
            <button
              onClick={fetchBalances}
              disabled={isLoadingBalances}
              className="p-2 sm:p-1 hover:bg-poly-card active:bg-poly-card rounded transition touch-target"
            >
              <RefreshCw className={`w-4 h-4 text-gray-400 ${isLoadingBalances ? 'animate-spin' : ''}`} />
            </button>
          </div>

          {proxyAddress && (
            <div className="flex items-center gap-1.5 sm:gap-2 mb-3">
              <code className="text-xs sm:text-sm font-mono bg-poly-card px-2 py-1.5 rounded flex-1 truncate">
                {proxyAddress.slice(0, 10)}...{proxyAddress.slice(-8)}
              </code>
              <button
                onClick={handleCopyAddress}
                className="p-2 sm:p-1.5 hover:bg-poly-card active:bg-poly-card rounded transition touch-target"
                title="Copy address"
              >
                {copied ? (
                  <CheckCircle className="w-4 h-4 text-poly-green" />
                ) : (
                  <Copy className="w-4 h-4 text-gray-400" />
                )}
              </button>
              <a
                href={`https://polygonscan.com/address/${proxyAddress}`}
                target="_blank"
                rel="noopener noreferrer"
                className="p-2 sm:p-1.5 hover:bg-poly-card active:bg-poly-card rounded transition touch-target"
                title="View on Polygonscan"
              >
                <ExternalLink className="w-4 h-4 text-gray-400" />
              </a>
            </div>
          )}

          <div className="flex items-center justify-between">
            <span className="text-gray-400 text-sm">Balance:</span>
            <span className="text-lg sm:text-xl font-bold text-poly-green">
              ${parseFloat(proxyBalance).toFixed(2)}
            </span>
          </div>
        </div>

        {/* EOA wallet info */}
        <div className="bg-poly-dark rounded-lg p-3 border border-poly-border">
          <div className="flex items-center justify-between flex-wrap gap-2">
            <div className="flex items-center gap-2">
              <div className="w-3 h-3 rounded-full bg-gradient-to-br from-purple-500 to-blue-500 flex-shrink-0" />
              <span className="text-xs sm:text-sm text-gray-400">Your Wallet</span>
              <code className="text-xs font-mono text-gray-500 hidden xs:inline">
                {eoaAddress?.slice(0, 6)}...{eoaAddress?.slice(-4)}
              </code>
            </div>
            <span className="font-semibold text-sm sm:text-base">${parseFloat(eoaBalance).toFixed(2)}</span>
          </div>
        </div>

        {/* Mode selector */}
        <div className="flex gap-3">
          <button
            onClick={() => setMode('deposit')}
            className={`flex-1 flex items-center justify-center gap-2 py-4 rounded-xl font-medium transition text-base ${
              mode === 'deposit'
                ? 'bg-poly-green text-black'
                : 'bg-poly-dark border-2 border-poly-border hover:border-poly-green active:border-poly-green'
            }`}
          >
            <ArrowDownToLine className="w-5 h-5" />
            Deposit
          </button>
          <button
            onClick={() => setMode('withdraw')}
            className={`flex-1 flex items-center justify-center gap-2 py-4 rounded-xl font-medium transition text-base ${
              mode === 'withdraw'
                ? 'bg-poly-green text-black'
                : 'bg-poly-dark border-2 border-poly-border hover:border-poly-green active:border-poly-green'
            }`}
          >
            <ArrowUpFromLine className="w-5 h-5" />
            Withdraw
          </button>
        </div>

        {/* Amount input */}
        <div>
          <div className="flex items-center justify-between mb-2">
            <label className="text-sm text-gray-400">Amount (USDC)</label>
            <button
              onClick={handleMaxAmount}
              className="text-sm text-poly-green hover:underline active:underline py-2 px-3 -mr-2 font-medium"
            >
              Max: ${mode === 'deposit' ? parseFloat(eoaBalance).toFixed(2) : parseFloat(proxyBalance).toFixed(2)}
            </button>
          </div>
          <input
            type="number"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            placeholder="0.00"
            min="0"
            step="0.01"
            inputMode="decimal"
            className="w-full px-4 py-4 bg-poly-dark border-2 border-poly-border rounded-xl focus:outline-none focus:border-poly-green text-lg"
          />
        </div>

        {/* Network warning */}
        {needsNetworkSwitch && (
          <div className="text-xs sm:text-sm text-yellow-400 bg-yellow-500/10 px-3 py-2 rounded">
            Please switch to Polygon network
          </div>
        )}

        {/* Error display */}
        {displayError && (
          <div className="text-xs sm:text-sm text-poly-red bg-poly-red/10 px-3 py-2 rounded">
            {displayError}
          </div>
        )}

        {/* Success message */}
        {(isConfirmed || withdrawSuccess) && (
          <div className="text-xs sm:text-sm text-poly-green bg-poly-green/10 px-3 py-2 rounded flex items-center gap-2">
            <CheckCircle className="w-4 h-4 flex-shrink-0" />
            {withdrawSuccess ? 'Withdrawal submitted!' : 'Transaction confirmed!'}
          </div>
        )}

        {/* Action button */}
        <button
          onClick={mode === 'deposit' ? handleDeposit : handleWithdraw}
          disabled={!amount || parseFloat(amount) <= 0 || isProcessing}
          className="w-full py-4 bg-poly-green text-black font-bold rounded-xl text-lg hover:bg-poly-green/90 active:bg-poly-green/80 transition disabled:opacity-50 disabled:cursor-not-allowed active:scale-[0.98]"
        >
          {isProcessing
            ? (isWithdrawing ? 'Withdrawing...' : isConfirming ? 'Confirming...' : 'Processing...')
            : mode === 'deposit'
              ? 'Deposit USDC'
              : 'Withdraw USDC'}
        </button>

        <p className="text-xs text-gray-500 text-center">
          {mode === 'deposit'
            ? 'Transfer USDC to your trading wallet'
            : 'Withdraw USDC to your wallet'}
        </p>
      </div>
    </Modal>
  )
}
