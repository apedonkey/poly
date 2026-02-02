import { useState, useEffect } from 'react'
import { Wallet, LogOut, RefreshCw, ArrowLeftRight, Plus, Key, ChevronLeft, Zap, AlertTriangle, Copy, Check, Eye, EyeOff } from 'lucide-react'
import { useAccount, useDisconnect } from 'wagmi'
import { ConnectButton } from '@rainbow-me/rainbowkit'
import { useWalletStore } from '../../stores/walletStore'
import { useBalance } from '../../hooks/useBalance'
import { useTradingBalance } from '../../hooks/useTradingBalance'
import { ProxyWalletManager } from './ProxyWalletManager'
import { GeneratedWalletManager } from './GeneratedWalletManager'
import { GeneratedWalletDeposit } from './GeneratedWalletDeposit'
import { Modal } from '../Modal'
import { connectExternalWallet, generateWallet, unlockWallet } from '../../api/client'

type ModalView = 'main' | 'generate' | 'unlock'

export function WalletConnect() {
  const { address: wagmiAddress, isConnected: wagmiConnected } = useAccount()
  const { disconnect: wagmiDisconnect } = useDisconnect()

  const { address, clearWallet, isConnected, isExternal, balance, setWallet } = useWalletStore()
  const { isLoading: balanceLoading, refetch } = useBalance()
  const { balance: tradingBalance, isLoading: tradingBalanceLoading, refetch: refetchTrading } = useTradingBalance()
  const [modalOpen, setModalOpen] = useState(false)
  const [modalView, setModalView] = useState<ModalView>('main')
  const [proxyWalletOpen, setProxyWalletOpen] = useState(false)
  const [walletManagerOpen, setWalletManagerOpen] = useState(false)
  const [depositOpen, setDepositOpen] = useState(false)

  // Form states
  const [password, setPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [unlockAddress, setUnlockAddress] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState('')
  const [generatedAddress, setGeneratedAddress] = useState('')
  const [generatedPrivateKey, setGeneratedPrivateKey] = useState('')
  const [showPrivateKey, setShowPrivateKey] = useState(false)
  const [copied, setCopied] = useState(false)
  const [copiedKey, setCopiedKey] = useState(false)

  const handleRefreshAll = () => {
    refetch()
    if (isExternal) {
      refetchTrading()
    }
  }

  // Sync wagmi wallet state with our store
  useEffect(() => {
    const syncWallet = async () => {
      if (wagmiConnected && wagmiAddress && !isConnected()) {
        try {
          // Create session for external wallet
          const result = await connectExternalWallet(wagmiAddress)
          setWallet(result.address, result.session_token, true)
        } catch (err) {
          console.error('Failed to sync wallet:', err)
        }
      }
    }
    syncWallet()
  }, [wagmiConnected, wagmiAddress, isConnected, setWallet])

  const handleDisconnect = () => {
    if (isExternal) {
      wagmiDisconnect()
    }
    clearWallet()
  }

  const closeModal = () => {
    setModalOpen(false)
    // Reset state after animation
    setTimeout(() => {
      setModalView('main')
      setPassword('')
      setConfirmPassword('')
      setUnlockAddress('')
      setError('')
      setGeneratedAddress('')
      setGeneratedPrivateKey('')
      setShowPrivateKey(false)
    }, 200)
  }

  const handleGenerate = async () => {
    if (password.length < 6) {
      setError('Password must be at least 6 characters')
      return
    }
    if (password !== confirmPassword) {
      setError('Passwords do not match')
      return
    }

    setIsLoading(true)
    setError('')

    try {
      const result = await generateWallet(password)
      setGeneratedAddress(result.address)
      setGeneratedPrivateKey(result.private_key)
      setWallet(result.address, result.session_token, false)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to generate wallet')
    } finally {
      setIsLoading(false)
    }
  }

  const copyPrivateKey = () => {
    navigator.clipboard.writeText(generatedPrivateKey)
    setCopiedKey(true)
    setTimeout(() => setCopiedKey(false), 2000)
  }

  const handleUnlock = async () => {
    if (!unlockAddress) {
      setError('Please enter your wallet address')
      return
    }
    if (!password) {
      setError('Please enter your password')
      return
    }

    setIsLoading(true)
    setError('')

    try {
      const result = await unlockWallet(unlockAddress, password)
      setWallet(unlockAddress.toLowerCase(), result.session_token, false)
      closeModal()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Invalid address or password')
    } finally {
      setIsLoading(false)
    }
  }

  const copyAddress = () => {
    navigator.clipboard.writeText(generatedAddress)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  // Show connected state
  if (isConnected()) {
    return (
      <>
        <div className="flex items-center gap-1.5 sm:gap-3">
          {/* Mobile compact balance - shows only on small screens */}
          <div className="flex sm:hidden items-center gap-1.5 px-2 py-1.5 bg-poly-card rounded-lg border border-poly-border">
            <span className="text-xs font-semibold text-poly-green">
              ${isExternal && tradingBalance ? tradingBalance.usdcFormatted : !isExternal && balance?.safe_usdc_balance ? balance.safe_usdc_balance : (balance?.usdc || '0.00')}
            </span>
            <button
              onClick={handleRefreshAll}
              disabled={balanceLoading || tradingBalanceLoading}
              className="p-0.5 hover:bg-poly-dark active:bg-poly-dark rounded transition"
              title="Refresh balance"
            >
              <RefreshCw className={`w-3 h-3 text-gray-500 ${(balanceLoading || tradingBalanceLoading) ? 'animate-spin' : ''}`} />
            </button>
          </div>

          {/* Desktop balance display */}
          <div className="hidden sm:flex items-center gap-3 px-3 py-2 bg-poly-card rounded-lg border border-poly-border">
            {/* For external wallets, show trading wallet balance prominently */}
            {isExternal && tradingBalance ? (
              <>
                <div className="text-right">
                  <div className="text-xs text-gray-500">Trading</div>
                  <div className="text-sm font-semibold text-poly-green">
                    ${tradingBalance.usdcFormatted}
                  </div>
                </div>
                <div className="w-px h-8 bg-poly-border" />
              </>
            ) : null}
            {/* For generated wallets, show Safe/Trading balance */}
            {!isExternal && balance?.safe_usdc_balance ? (
              <>
                <div className="text-right">
                  <div className="text-xs text-gray-500">Trading</div>
                  <div className="text-sm font-semibold text-poly-green">
                    ${balance.safe_usdc_balance}
                  </div>
                </div>
                <div className="w-px h-8 bg-poly-border" />
              </>
            ) : null}
            <div className="text-right">
              <div className="text-xs text-gray-500">{isExternal ? 'Wallet' : 'USDC'}</div>
              <div className="text-sm font-semibold text-poly-green">
                ${balance?.usdc || '0.00'}
              </div>
            </div>
            {!isExternal && (
              <>
                <div className="w-px h-8 bg-poly-border" />
                <div className="text-right">
                  <div className="text-xs text-gray-500">POL</div>
                  <div className="text-sm font-semibold">
                    {balance?.matic || '0.00'}
                  </div>
                </div>
              </>
            )}
            <button
              onClick={handleRefreshAll}
              disabled={balanceLoading || tradingBalanceLoading}
              className="p-1 hover:bg-poly-dark rounded transition"
              title="Refresh balance"
            >
              <RefreshCw className={`w-3.5 h-3.5 text-gray-500 ${(balanceLoading || tradingBalanceLoading) ? 'animate-spin' : ''}`} />
            </button>
          </div>

          {/* Deposit/Withdraw button for external wallets */}
          {isExternal && (
            <button
              onClick={() => setProxyWalletOpen(true)}
              className="flex items-center gap-1.5 px-2.5 sm:px-3 py-2 bg-poly-card rounded-lg border border-poly-border hover:border-poly-green active:border-poly-green transition touch-target"
              title="Manage Trading Wallet"
            >
              <ArrowLeftRight className="w-4 h-4 text-poly-green" />
              <span className="text-sm font-medium hidden md:inline">Deposit</span>
            </button>
          )}

          {/* Deposit + Wallet Manager buttons for generated wallets */}
          {!isExternal && (
            <>
              <button
                onClick={() => setDepositOpen(true)}
                className="flex items-center gap-1.5 px-2.5 sm:px-3 py-2 bg-poly-card rounded-lg border border-poly-border hover:border-poly-green active:border-poly-green transition touch-target"
                title="Deposit / Withdraw"
              >
                <ArrowLeftRight className="w-4 h-4 text-poly-green" />
                <span className="text-sm font-medium hidden md:inline">Deposit</span>
              </button>
              <button
                onClick={() => setWalletManagerOpen(true)}
                className="flex items-center gap-1.5 px-2.5 sm:px-3 py-2 bg-poly-card rounded-lg border border-poly-border hover:border-yellow-500 active:border-yellow-500 transition touch-target"
                title="Manage Wallet"
              >
                <Key className="w-4 h-4 text-yellow-500" />
                <span className="text-sm font-medium hidden md:inline">Manage</span>
              </button>
            </>
          )}

          {/* Address display - More compact on mobile */}
          <div className="flex items-center gap-1.5 sm:gap-2 px-2 sm:px-3 py-2 bg-poly-card rounded-lg border border-poly-border">
            {isExternal ? (
              <div className="w-4 h-4 rounded-full bg-gradient-to-br from-purple-500 to-blue-500 flex-shrink-0" />
            ) : (
              <Wallet className="w-4 h-4 text-poly-green flex-shrink-0" />
            )}
            <span className="font-mono text-xs sm:text-sm">
              {address?.slice(0, 4)}...{address?.slice(-3)}
            </span>
            {isExternal && (
              <span className="text-xs text-gray-500 bg-poly-dark px-1 sm:px-1.5 py-0.5 rounded hidden sm:inline">External</span>
            )}
          </div>
          <button
            onClick={handleDisconnect}
            className="p-2 hover:bg-poly-card active:bg-poly-card rounded-lg transition touch-target"
            title="Disconnect"
          >
            <LogOut className="w-4 h-4 sm:w-5 sm:h-5 text-gray-400 hover:text-poly-red active:text-poly-red" />
          </button>
        </div>

        {/* Proxy Wallet Manager Modal */}
        {isExternal && (
          <ProxyWalletManager
            isOpen={proxyWalletOpen}
            onClose={() => setProxyWalletOpen(false)}
          />
        )}

        {/* Generated Wallet Manager Modal */}
        {!isExternal && (
          <>
            <GeneratedWalletManager
              isOpen={walletManagerOpen}
              onClose={() => setWalletManagerOpen(false)}
            />
            <GeneratedWalletDeposit
              isOpen={depositOpen}
              onClose={() => setDepositOpen(false)}
            />
          </>
        )}
      </>
    )
  }

  return (
    <>
      <button
        onClick={() => setModalOpen(true)}
        className="flex items-center gap-1.5 sm:gap-2 px-3 sm:px-4 py-2 bg-poly-green text-black font-semibold rounded-lg hover:bg-poly-green/90 active:bg-poly-green/80 transition touch-target text-sm sm:text-base"
      >
        <Wallet className="w-4 h-4 sm:w-5 sm:h-5" />
        <span className="hidden xs:inline">Connect</span>
        <span className="xs:hidden">Connect</span>
      </button>

      <Modal
        isOpen={modalOpen}
        onClose={closeModal}
        title={
          modalView === 'main' ? 'Connect Wallet' :
          modalView === 'generate' ? (generatedAddress ? 'Wallet Created!' : 'Generate Wallet') :
          'Unlock Wallet'
        }
      >
        {/* Back button for sub-views */}
        {modalView !== 'main' && !generatedAddress && (
          <button
            onClick={() => {
              setModalView('main')
              setError('')
              setPassword('')
              setConfirmPassword('')
            }}
            className="flex items-center gap-1 text-sm text-gray-400 hover:text-white mb-4 -mt-2"
          >
            <ChevronLeft className="w-4 h-4" />
            Back
          </button>
        )}

        {/* Main view - wallet options */}
        {modalView === 'main' && (
          <div className="space-y-3">
            {/* External wallet (MetaMask) */}
            <ConnectButton.Custom>
              {({ openConnectModal }) => (
                <button
                  onClick={() => {
                    closeModal()
                    openConnectModal()
                  }}
                  className="w-full flex items-center gap-3 p-4 bg-poly-dark border border-poly-border rounded-lg hover:border-purple-500 transition"
                >
                  <div className="w-8 h-8 rounded-full bg-gradient-to-br from-purple-500 to-blue-500 flex items-center justify-center flex-shrink-0">
                    <Wallet className="w-4 h-4 text-white" />
                  </div>
                  <div className="text-left flex-1 min-w-0">
                    <div className="font-semibold">Connect External Wallet</div>
                    <div className="text-sm text-gray-400">MetaMask, Coinbase, WalletConnect</div>
                  </div>
                </button>
              )}
            </ConnectButton.Custom>

            <div className="relative">
              <div className="absolute inset-0 flex items-center">
                <div className="w-full border-t border-poly-border" />
              </div>
              <div className="relative flex justify-center text-xs">
                <span className="px-2 bg-poly-card text-gray-500">or for Auto-Trading</span>
              </div>
            </div>

            {/* Generate new wallet */}
            <button
              onClick={() => setModalView('generate')}
              className="w-full flex items-center gap-3 p-4 bg-poly-dark border border-poly-border rounded-lg hover:border-poly-green transition"
            >
              <div className="w-8 h-8 rounded-full bg-poly-green/20 flex items-center justify-center flex-shrink-0">
                <Plus className="w-4 h-4 text-poly-green" />
              </div>
              <div className="text-left flex-1 min-w-0">
                <div className="font-semibold flex items-center gap-2">
                  Generate New Wallet
                  <Zap className="w-3.5 h-3.5 text-poly-green" />
                </div>
                <div className="text-sm text-gray-400">Create a new wallet for auto-trading</div>
              </div>
            </button>

            {/* Unlock existing wallet */}
            <button
              onClick={() => setModalView('unlock')}
              className="w-full flex items-center gap-3 p-4 bg-poly-dark border border-poly-border rounded-lg hover:border-yellow-500 transition"
            >
              <div className="w-8 h-8 rounded-full bg-yellow-500/20 flex items-center justify-center flex-shrink-0">
                <Key className="w-4 h-4 text-yellow-500" />
              </div>
              <div className="text-left flex-1 min-w-0">
                <div className="font-semibold">Unlock Existing Wallet</div>
                <div className="text-sm text-gray-400">Return to a previously generated wallet</div>
              </div>
            </button>

            <p className="text-xs text-gray-500 text-center pt-2">
              Generated wallets support auto-trading. External wallets require manual signing.
            </p>
          </div>
        )}

        {/* Generate wallet view */}
        {modalView === 'generate' && !generatedAddress && (
          <div className="space-y-4">
            <p className="text-sm text-gray-400">
              Create a new wallet with a password. This enables auto-trading features.
            </p>

            <div>
              <label className="block text-sm text-gray-400 mb-1.5">Password</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="At least 6 characters"
                className="w-full px-3 py-2.5 bg-poly-dark border border-poly-border rounded-lg focus:outline-none focus:border-poly-green"
              />
            </div>

            <div>
              <label className="block text-sm text-gray-400 mb-1.5">Confirm Password</label>
              <input
                type="password"
                value={confirmPassword}
                onChange={(e) => setConfirmPassword(e.target.value)}
                placeholder="Re-enter password"
                className="w-full px-3 py-2.5 bg-poly-dark border border-poly-border rounded-lg focus:outline-none focus:border-poly-green"
              />
            </div>

            {error && (
              <div className="p-3 bg-poly-red/10 border border-poly-red/30 rounded-lg text-sm text-poly-red">
                {error}
              </div>
            )}

            <div className="p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
              <div className="flex gap-2">
                <AlertTriangle className="w-4 h-4 text-yellow-500 flex-shrink-0 mt-0.5" />
                <p className="text-xs text-yellow-200">
                  Save your wallet address and password! You'll need both to access your wallet again.
                </p>
              </div>
            </div>

            <button
              onClick={handleGenerate}
              disabled={isLoading || !password || !confirmPassword}
              className="w-full py-3 bg-poly-green text-black font-semibold rounded-lg hover:bg-poly-green/90 disabled:opacity-50 disabled:cursor-not-allowed transition"
            >
              {isLoading ? 'Creating...' : 'Create Wallet'}
            </button>
          </div>
        )}

        {/* Wallet created success view */}
        {modalView === 'generate' && generatedAddress && (
          <div className="space-y-4">
            <div className="text-center">
              <div className="w-16 h-16 bg-poly-green/20 rounded-full flex items-center justify-center mx-auto mb-4">
                <Check className="w-8 h-8 text-poly-green" />
              </div>
              <p className="text-gray-400 mb-4">Your wallet has been created and connected!</p>
            </div>

            <div className="p-4 bg-poly-dark rounded-lg border border-poly-border">
              <label className="block text-xs text-gray-500 mb-1">Your Wallet Address</label>
              <div className="flex items-center gap-2">
                <code className="flex-1 text-sm font-mono break-all">{generatedAddress}</code>
                <button
                  onClick={copyAddress}
                  className="p-2 hover:bg-poly-card rounded transition flex-shrink-0"
                  title="Copy address"
                >
                  {copied ? (
                    <Check className="w-4 h-4 text-poly-green" />
                  ) : (
                    <Copy className="w-4 h-4 text-gray-400" />
                  )}
                </button>
              </div>
            </div>

            {/* Private Key - IMPORTANT */}
            <div className="p-4 bg-poly-dark rounded-lg border border-yellow-500/50">
              <div className="flex items-center justify-between mb-2">
                <label className="text-xs text-yellow-400 font-semibold">Private Key (SAVE THIS!)</label>
                <button
                  onClick={() => setShowPrivateKey(!showPrivateKey)}
                  className="flex items-center gap-1 text-xs text-gray-400 hover:text-white"
                >
                  {showPrivateKey ? <EyeOff className="w-3 h-3" /> : <Eye className="w-3 h-3" />}
                  {showPrivateKey ? 'Hide' : 'Show'}
                </button>
              </div>
              <div className="flex items-center gap-2">
                <code className="flex-1 text-sm font-mono break-all">
                  {showPrivateKey ? generatedPrivateKey : '••••••••••••••••••••••••••••••••••••••••••••••••'}
                </code>
                <button
                  onClick={copyPrivateKey}
                  className="p-2 hover:bg-poly-card rounded transition flex-shrink-0"
                  title="Copy private key"
                >
                  {copiedKey ? (
                    <Check className="w-4 h-4 text-poly-green" />
                  ) : (
                    <Copy className="w-4 h-4 text-gray-400" />
                  )}
                </button>
              </div>
              <p className="text-xs text-yellow-200/70 mt-2">
                This is your ONLY chance to save this key. Store it securely!
              </p>
            </div>

            <div className="p-3 bg-red-500/10 border border-red-500/30 rounded-lg">
              <div className="flex gap-2">
                <AlertTriangle className="w-4 h-4 text-red-500 flex-shrink-0 mt-0.5" />
                <div className="text-xs text-red-200">
                  <p className="font-semibold mb-1">IMPORTANT - Save your credentials!</p>
                  <ul className="list-disc list-inside space-y-0.5">
                    <li>Address: {generatedAddress.slice(0, 10)}...{generatedAddress.slice(-6)}</li>
                    <li>Password: The one you just entered</li>
                  </ul>
                  <p className="mt-1">You need BOTH to access this wallet. There is no recovery!</p>
                </div>
              </div>
            </div>

            <div className="p-3 bg-poly-dark border border-poly-border rounded-lg">
              <p className="text-xs text-gray-400 mb-2">Next steps to start trading:</p>
              <ol className="text-xs text-gray-300 list-decimal list-inside space-y-1">
                <li>Send USDC (Polygon) to this address</li>
                <li>Send a small amount of POL for gas fees</li>
                <li>Go to Auto-Trade tab to enable auto-trading</li>
              </ol>
            </div>

            <button
              onClick={closeModal}
              className="w-full py-3 bg-poly-green text-black font-semibold rounded-lg hover:bg-poly-green/90 transition"
            >
              Done
            </button>
          </div>
        )}

        {/* Unlock wallet view */}
        {modalView === 'unlock' && (
          <div className="space-y-4">
            <p className="text-sm text-gray-400">
              Enter your wallet address and password to unlock.
            </p>

            <div>
              <label className="block text-sm text-gray-400 mb-1.5">Wallet Address</label>
              <input
                type="text"
                value={unlockAddress}
                onChange={(e) => setUnlockAddress(e.target.value)}
                placeholder="0x..."
                className="w-full px-3 py-2.5 bg-poly-dark border border-poly-border rounded-lg focus:outline-none focus:border-poly-green font-mono text-sm"
              />
            </div>

            <div>
              <label className="block text-sm text-gray-400 mb-1.5">Password</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Enter your password"
                className="w-full px-3 py-2.5 bg-poly-dark border border-poly-border rounded-lg focus:outline-none focus:border-poly-green"
              />
            </div>

            {error && (
              <div className="p-3 bg-poly-red/10 border border-poly-red/30 rounded-lg text-sm text-poly-red">
                {error}
              </div>
            )}

            <button
              onClick={handleUnlock}
              disabled={isLoading || !unlockAddress || !password}
              className="w-full py-3 bg-poly-green text-black font-semibold rounded-lg hover:bg-poly-green/90 disabled:opacity-50 disabled:cursor-not-allowed transition"
            >
              {isLoading ? 'Unlocking...' : 'Unlock Wallet'}
            </button>
          </div>
        )}
      </Modal>
    </>
  )
}
