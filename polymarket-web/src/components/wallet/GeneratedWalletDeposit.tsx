import { useState, useEffect, useCallback } from 'react'
import { Wallet, ArrowDownToLine, ArrowUpFromLine, Copy, ExternalLink, RefreshCw, CheckCircle, Lock } from 'lucide-react'
import { Modal } from '../Modal'
import { useWalletStore } from '../../stores/walletStore'
import { useBalance } from '../../hooks/useBalance'
import { depositToSafe, withdrawFromSafe } from '../../api/client'

interface Props {
  isOpen: boolean
  onClose: () => void
}

export function GeneratedWalletDeposit({ isOpen, onClose }: Props) {
  const { address, sessionToken } = useWalletStore()
  const { balance, refetch } = useBalance()

  const [mode, setMode] = useState<'deposit' | 'withdraw'>('deposit')
  const [amount, setAmount] = useState('')
  const [password, setPassword] = useState('')
  const [isProcessing, setIsProcessing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  const eoaBalance = balance?.usdc_balance || '0.00'
  const safeBalance = balance?.safe_usdc_balance || '0.00'
  const safeAddress = balance?.safe_address || null

  // Refresh balances when modal opens
  useEffect(() => {
    if (isOpen) {
      refetch()
    }
  }, [isOpen, refetch])

  // Clear state when switching modes
  useEffect(() => {
    setError(null)
    setSuccess(null)
  }, [mode])

  const handleCopyAddress = useCallback(async () => {
    if (safeAddress) {
      try {
        await navigator.clipboard.writeText(safeAddress)
      } catch {
        // Fallback
        const textarea = document.createElement('textarea')
        textarea.value = safeAddress
        textarea.style.position = 'fixed'
        textarea.style.opacity = '0'
        document.body.appendChild(textarea)
        textarea.select()
        document.execCommand('copy')
        document.body.removeChild(textarea)
      }
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    }
  }, [safeAddress])

  const handleMaxAmount = () => {
    if (mode === 'deposit') {
      setAmount(eoaBalance)
    } else {
      setAmount(safeBalance)
    }
  }

  const handleSubmit = async () => {
    if (!amount || !password || !sessionToken) return
    const amountNum = parseFloat(amount)
    if (isNaN(amountNum) || amountNum <= 0) {
      setError('Enter a valid amount')
      return
    }

    setIsProcessing(true)
    setError(null)
    setSuccess(null)

    try {
      if (mode === 'deposit') {
        const result = await depositToSafe(sessionToken, password, amount)
        setSuccess(`Deposit sent! Tx: ${result.tx_hash.slice(0, 10)}...`)
      } else {
        const result = await withdrawFromSafe(sessionToken, password, amount)
        setSuccess(`Withdrawal submitted! ID: ${result.transaction_id.slice(0, 10)}...`)
      }
      setAmount('')
      setPassword('')

      // Auto-refresh balances after delays
      setTimeout(() => refetch(), 3000)
      setTimeout(() => refetch(), 10000)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Transaction failed')
    } finally {
      setIsProcessing(false)
    }
  }

  const handleClose = () => {
    setAmount('')
    setPassword('')
    setError(null)
    setSuccess(null)
    onClose()
  }

  return (
    <Modal isOpen={isOpen} onClose={handleClose} title="Trading Wallet">
      <div className="space-y-4">
        {/* Trading Wallet (Safe) info */}
        <div className="bg-poly-dark rounded-lg p-3 sm:p-4 border border-poly-border">
          <div className="flex items-center justify-between mb-2">
            <div className="flex items-center gap-2">
              <Wallet className="w-4 h-4 text-poly-green flex-shrink-0" />
              <span className="text-xs sm:text-sm text-gray-400">Trading Wallet</span>
            </div>
            <button
              onClick={() => refetch()}
              className="p-2 sm:p-1 hover:bg-poly-card active:bg-poly-card rounded transition touch-target"
            >
              <RefreshCw className="w-4 h-4 text-gray-400" />
            </button>
          </div>

          {safeAddress && (
            <div className="flex items-center gap-1.5 sm:gap-2 mb-3">
              <code className="text-xs sm:text-sm font-mono bg-poly-card px-2 py-1.5 rounded flex-1 truncate">
                {safeAddress.slice(0, 10)}...{safeAddress.slice(-8)}
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
                href={`https://polygonscan.com/address/${safeAddress}`}
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
              ${parseFloat(safeBalance).toFixed(2)}
            </span>
          </div>
        </div>

        {/* EOA Wallet info */}
        <div className="bg-poly-dark rounded-lg p-3 border border-poly-border">
          <div className="flex items-center justify-between flex-wrap gap-2">
            <div className="flex items-center gap-2">
              <Wallet className="w-3 h-3 text-poly-green flex-shrink-0" />
              <span className="text-xs sm:text-sm text-gray-400">Your Wallet</span>
              <code className="text-xs font-mono text-gray-500 hidden xs:inline">
                {address?.slice(0, 6)}...{address?.slice(-4)}
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
              Max: ${mode === 'deposit' ? parseFloat(eoaBalance).toFixed(2) : parseFloat(safeBalance).toFixed(2)}
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

        {/* Password input */}
        <div>
          <label className="text-sm text-gray-400 mb-2 flex items-center gap-1.5">
            <Lock className="w-3.5 h-3.5" />
            Wallet Password
          </label>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="Enter your wallet password"
            className="w-full px-4 py-3 bg-poly-dark border-2 border-poly-border rounded-xl focus:outline-none focus:border-poly-green text-sm"
          />
        </div>

        {/* Error display */}
        {error && (
          <div className="text-xs sm:text-sm text-poly-red bg-poly-red/10 px-3 py-2 rounded">
            {error}
          </div>
        )}

        {/* Success message */}
        {success && (
          <div className="text-xs sm:text-sm text-poly-green bg-poly-green/10 px-3 py-2 rounded flex items-center gap-2">
            <CheckCircle className="w-4 h-4 flex-shrink-0" />
            {success}
          </div>
        )}

        {/* Action button */}
        <button
          onClick={handleSubmit}
          disabled={!amount || parseFloat(amount) <= 0 || !password || isProcessing}
          className="w-full py-4 bg-poly-green text-black font-bold rounded-xl text-lg hover:bg-poly-green/90 active:bg-poly-green/80 transition disabled:opacity-50 disabled:cursor-not-allowed active:scale-[0.98]"
        >
          {isProcessing
            ? (mode === 'deposit' ? 'Depositing...' : 'Withdrawing...')
            : mode === 'deposit'
              ? 'Deposit USDC'
              : 'Withdraw USDC'}
        </button>

        <p className="text-xs text-gray-500 text-center">
          {mode === 'deposit'
            ? 'Transfer USDC from your wallet to the trading wallet (Safe)'
            : 'Withdraw USDC from the trading wallet (Safe) to your wallet'}
        </p>
      </div>
    </Modal>
  )
}
