import { useState, useEffect } from 'react'
import { Wallet, LogOut, RefreshCw, ArrowLeftRight } from 'lucide-react'
import { useAccount, useDisconnect } from 'wagmi'
import { ConnectButton } from '@rainbow-me/rainbowkit'
import { useWalletStore } from '../../stores/walletStore'
import { useBalance } from '../../hooks/useBalance'
import { useTradingBalance } from '../../hooks/useTradingBalance'
import { ProxyWalletManager } from './ProxyWalletManager'
import { Modal } from '../Modal'
import { connectExternalWallet } from '../../api/client'

export function WalletConnect() {
  const { address: wagmiAddress, isConnected: wagmiConnected } = useAccount()
  const { disconnect: wagmiDisconnect } = useDisconnect()

  const { address, clearWallet, isConnected, isExternal, balance, setWallet } = useWalletStore()
  const { isLoading: balanceLoading, refetch } = useBalance()
  const { balance: tradingBalance, isLoading: tradingBalanceLoading, refetch: refetchTrading } = useTradingBalance()
  const [modalOpen, setModalOpen] = useState(false)
  const [proxyWalletOpen, setProxyWalletOpen] = useState(false)

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
  }

  // Show connected state
  if (isConnected()) {
    return (
      <>
        <div className="flex items-center gap-1.5 sm:gap-3">
          {/* Mobile compact balance - shows only on small screens */}
          <div className="flex sm:hidden items-center gap-1.5 px-2 py-1.5 bg-poly-card rounded-lg border border-poly-border">
            <span className="text-xs font-semibold text-poly-green">
              ${isExternal && tradingBalance ? tradingBalance.usdcFormatted : (balance?.usdc || '0.00')}
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

      <Modal isOpen={modalOpen} onClose={closeModal} title="Connect Wallet">
        <div className="space-y-4">
          {/* RainbowKit wallet connection */}
          <div className="w-full">
            <ConnectButton.Custom>
              {({ openConnectModal }) => (
                <button
                  onClick={() => {
                    closeModal()
                    openConnectModal()
                  }}
                  className="w-full flex items-center gap-3 p-4 bg-poly-dark border border-poly-border rounded-lg hover:border-purple-500 active:border-purple-500 transition touch-target"
                >
                  <div className="w-6 h-6 rounded-full bg-gradient-to-br from-purple-500 to-blue-500 flex-shrink-0" />
                  <div className="text-left min-w-0">
                    <div className="font-semibold">Connect Wallet</div>
                    <div className="text-sm text-gray-400 truncate">MetaMask, Coinbase, WalletConnect & more</div>
                  </div>
                </button>
              )}
            </ConnectButton.Custom>
          </div>

          <p className="text-xs text-gray-500 text-center px-2">
            Connect your wallet to start trading. You'll need USDC.e on Polygon in your trading wallet.
          </p>
        </div>
      </Modal>
    </>
  )
}
