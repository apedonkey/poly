import { useState } from 'react'
import { Key, Copy, Check, Eye, EyeOff, AlertTriangle } from 'lucide-react'
import { Modal } from '../Modal'
import { useWalletStore } from '../../stores/walletStore'
import { exportPrivateKey } from '../../api/client'

interface Props {
  isOpen: boolean
  onClose: () => void
}

export function GeneratedWalletManager({ isOpen, onClose }: Props) {
  const { address, sessionToken } = useWalletStore()
  const [password, setPassword] = useState('')
  const [privateKey, setPrivateKey] = useState('')
  const [showKey, setShowKey] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState('')
  const [copied, setCopied] = useState(false)
  const [copiedAddress, setCopiedAddress] = useState(false)

  const handleExport = async () => {
    if (!password) {
      setError('Please enter your password')
      return
    }

    setIsLoading(true)
    setError('')

    try {
      const result = await exportPrivateKey(sessionToken!, password)
      setPrivateKey(result.private_key)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Invalid password')
    } finally {
      setIsLoading(false)
    }
  }

  const handleClose = () => {
    setPassword('')
    setPrivateKey('')
    setShowKey(false)
    setError('')
    onClose()
  }

  const fallbackCopy = (text: string) => {
    const textarea = document.createElement('textarea')
    textarea.value = text
    textarea.style.position = 'fixed'
    textarea.style.opacity = '0'
    document.body.appendChild(textarea)
    textarea.select()
    document.execCommand('copy')
    document.body.removeChild(textarea)
  }

  const copyPrivateKey = () => {
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(privateKey).catch(() => fallbackCopy(privateKey))
    } else {
      fallbackCopy(privateKey)
    }
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  const copyAddress = () => {
    const text = address || ''
    if (navigator.clipboard?.writeText) {
      navigator.clipboard.writeText(text).catch(() => fallbackCopy(text))
    } else {
      fallbackCopy(text)
    }
    setCopiedAddress(true)
    setTimeout(() => setCopiedAddress(false), 2000)
  }

  return (
    <Modal isOpen={isOpen} onClose={handleClose} title="Wallet Manager">
      <div className="space-y-4">
        {/* Wallet Address */}
        <div className="p-4 bg-poly-dark rounded-lg border border-poly-border">
          <label className="block text-xs text-gray-500 mb-1">Wallet Address</label>
          <div className="flex items-center gap-2">
            <code className="flex-1 text-sm font-mono break-all">{address}</code>
            <button
              onClick={copyAddress}
              className="p-2 hover:bg-poly-card rounded transition flex-shrink-0"
              title="Copy address"
            >
              {copiedAddress ? (
                <Check className="w-4 h-4 text-poly-green" />
              ) : (
                <Copy className="w-4 h-4 text-gray-400" />
              )}
            </button>
          </div>
        </div>

        {/* Export Private Key Section */}
        <div className="p-4 bg-poly-dark rounded-lg border border-poly-border">
          <div className="flex items-center gap-2 mb-3">
            <Key className="w-4 h-4 text-yellow-500" />
            <h3 className="font-semibold text-sm">Export Private Key</h3>
          </div>

          {!privateKey ? (
            <>
              <p className="text-xs text-gray-400 mb-3">
                Enter your wallet password to reveal your private key.
              </p>

              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="Enter wallet password"
                className="w-full px-3 py-2 bg-gray-700 border border-gray-600 rounded-lg focus:outline-none focus:border-poly-green text-sm mb-3"
              />

              {error && (
                <p className="text-poly-red text-sm mb-3">{error}</p>
              )}

              <button
                onClick={handleExport}
                disabled={isLoading || !password}
                className="w-full py-2 bg-yellow-600 hover:bg-yellow-500 text-black font-semibold rounded-lg transition disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {isLoading ? 'Verifying...' : 'Show Private Key'}
              </button>
            </>
          ) : (
            <>
              <div className="p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg mb-3">
                <div className="flex gap-2">
                  <AlertTriangle className="w-4 h-4 text-yellow-500 flex-shrink-0 mt-0.5" />
                  <p className="text-xs text-yellow-200">
                    Never share your private key! Anyone with this key can access your funds.
                  </p>
                </div>
              </div>

              <div className="flex items-center justify-between mb-2">
                <label className="text-xs text-gray-500">Private Key</label>
                <button
                  onClick={() => setShowKey(!showKey)}
                  className="flex items-center gap-1 text-xs text-gray-400 hover:text-white"
                >
                  {showKey ? <EyeOff className="w-3 h-3" /> : <Eye className="w-3 h-3" />}
                  {showKey ? 'Hide' : 'Show'}
                </button>
              </div>

              <div className="flex items-center gap-2 p-2 bg-gray-800 rounded border border-gray-700">
                <code className="flex-1 text-xs font-mono break-all">
                  {showKey ? privateKey : '••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••••'}
                </code>
                <button
                  onClick={copyPrivateKey}
                  className="p-1.5 hover:bg-gray-700 rounded transition flex-shrink-0"
                  title="Copy private key"
                >
                  {copied ? (
                    <Check className="w-4 h-4 text-poly-green" />
                  ) : (
                    <Copy className="w-4 h-4 text-gray-400" />
                  )}
                </button>
              </div>

              <button
                onClick={() => {
                  setPrivateKey('')
                  setPassword('')
                }}
                className="w-full mt-3 py-2 bg-gray-600 hover:bg-gray-500 text-white font-medium rounded-lg transition text-sm"
              >
                Hide Key
              </button>
            </>
          )}
        </div>

        {/* Info */}
        <div className="text-xs text-gray-500 space-y-1">
          <p>This is a generated wallet. Your encrypted private key is stored on the server.</p>
          <p>To use this wallet elsewhere, export and import the private key.</p>
        </div>
      </div>
    </Modal>
  )
}
