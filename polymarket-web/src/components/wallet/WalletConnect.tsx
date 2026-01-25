import { useState, useEffect } from 'react'
import { Wallet, LogOut, Plus, Key, RefreshCw } from 'lucide-react'
import { useAccount, useDisconnect } from 'wagmi'
import { ConnectButton } from '@rainbow-me/rainbowkit'
import { useWalletStore } from '../../stores/walletStore'
import { useBalance } from '../../hooks/useBalance'
import { GenerateWallet } from './GenerateWallet'
import { ImportWallet } from './ImportWallet'
import { Modal } from '../Modal'
import { connectExternalWallet } from '../../api/client'

export function WalletConnect() {
  const { address: wagmiAddress, isConnected: wagmiConnected } = useAccount()
  const { disconnect: wagmiDisconnect } = useDisconnect()

  const { address, clearWallet, isConnected, isExternal, balance, setWallet } = useWalletStore()
  const { isLoading: balanceLoading, refetch } = useBalance()
  const [modalOpen, setModalOpen] = useState(false)
  const [mode, setMode] = useState<'select' | 'generate' | 'import'>('select')

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
    setMode('select')
  }

  // Show connected state
  if (isConnected()) {
    return (
      <div className="flex items-center gap-3">
        {/* Balance display */}
        <div className="hidden sm:flex items-center gap-3 px-3 py-2 bg-poly-card rounded-lg border border-poly-border">
          <div className="text-right">
            <div className="text-xs text-gray-500">USDC</div>
            <div className="text-sm font-semibold text-poly-green">
              ${balance?.usdc || '0.00'}
            </div>
          </div>
          <div className="w-px h-8 bg-poly-border" />
          <div className="text-right">
            <div className="text-xs text-gray-500">POL</div>
            <div className="text-sm font-semibold">
              {balance?.matic || '0.00'}
            </div>
          </div>
          <button
            onClick={() => refetch()}
            disabled={balanceLoading}
            className="p-1 hover:bg-poly-dark rounded transition"
            title="Refresh balance"
          >
            <RefreshCw className={`w-3.5 h-3.5 text-gray-500 ${balanceLoading ? 'animate-spin' : ''}`} />
          </button>
        </div>

        {/* Address display */}
        <div className="flex items-center gap-2 px-3 py-2 bg-poly-card rounded-lg border border-poly-border">
          {isExternal ? (
            <div className="w-4 h-4 rounded-full bg-gradient-to-br from-purple-500 to-blue-500" />
          ) : (
            <Wallet className="w-4 h-4 text-poly-green" />
          )}
          <span className="font-mono text-sm">
            {address?.slice(0, 6)}...{address?.slice(-4)}
          </span>
          {isExternal && (
            <span className="text-xs text-gray-500 bg-poly-dark px-1.5 py-0.5 rounded">External</span>
          )}
        </div>
        <button
          onClick={handleDisconnect}
          className="p-2 hover:bg-poly-card rounded-lg transition"
          title="Disconnect"
        >
          <LogOut className="w-5 h-5 text-gray-400 hover:text-poly-red" />
        </button>
      </div>
    )
  }

  return (
    <>
      <button
        onClick={() => setModalOpen(true)}
        className="flex items-center gap-2 px-4 py-2 bg-poly-green text-black font-semibold rounded-lg hover:bg-poly-green/90 transition"
      >
        <Wallet className="w-5 h-5" />
        Connect Wallet
      </button>

      <Modal isOpen={modalOpen} onClose={closeModal} title="Connect Wallet">
        {mode === 'select' && (
          <div className="space-y-3">
            {/* RainbowKit wallet connection */}
            <div className="w-full">
              <ConnectButton.Custom>
                {({ openConnectModal }) => (
                  <button
                    onClick={() => {
                      closeModal()
                      openConnectModal()
                    }}
                    className="w-full flex items-center gap-3 p-4 bg-poly-dark border border-poly-border rounded-lg hover:border-purple-500 transition"
                  >
                    <div className="w-6 h-6 rounded-full bg-gradient-to-br from-purple-500 to-blue-500" />
                    <div className="text-left">
                      <div className="font-semibold">Connect Wallet</div>
                      <div className="text-sm text-gray-400">MetaMask, Coinbase, WalletConnect & more</div>
                    </div>
                  </button>
                )}
              </ConnectButton.Custom>
            </div>

            <div className="relative">
              <div className="absolute inset-0 flex items-center">
                <div className="w-full border-t border-poly-border" />
              </div>
              <div className="relative flex justify-center text-xs">
                <span className="px-2 bg-poly-card text-gray-500">or use a bot-managed wallet</span>
              </div>
            </div>

            <button
              onClick={() => setMode('generate')}
              className="w-full flex items-center gap-3 p-4 bg-poly-dark border border-poly-border rounded-lg hover:border-poly-green transition"
            >
              <Plus className="w-6 h-6 text-poly-green" />
              <div className="text-left">
                <div className="font-semibold">Generate New Wallet</div>
                <div className="text-sm text-gray-400">Create a wallet for automated trading</div>
              </div>
            </button>
            <button
              onClick={() => setMode('import')}
              className="w-full flex items-center gap-3 p-4 bg-poly-dark border border-poly-border rounded-lg hover:border-poly-green transition"
            >
              <Key className="w-6 h-6 text-poly-green" />
              <div className="text-left">
                <div className="font-semibold">Import Existing Wallet</div>
                <div className="text-sm text-gray-400">Use your private key</div>
              </div>
            </button>

            <p className="text-xs text-gray-500 text-center pt-2">
              External wallets can paper trade. Generated wallets support live trading.
            </p>
          </div>
        )}
        {mode === 'generate' && <GenerateWallet onClose={closeModal} />}
        {mode === 'import' && <ImportWallet onClose={closeModal} />}
      </Modal>
    </>
  )
}
