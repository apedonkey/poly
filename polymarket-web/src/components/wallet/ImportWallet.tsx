import { useState } from 'react'
import { Key } from 'lucide-react'
import { importWallet } from '../../api/client'
import { useWalletStore } from '../../stores/walletStore'

interface Props {
  onClose: () => void
}

export function ImportWallet({ onClose }: Props) {
  const [privateKey, setPrivateKey] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const setWallet = useWalletStore((s) => s.setWallet)

  const handleImport = async () => {
    if (!privateKey.trim()) {
      setError('Private key is required')
      return
    }

    setLoading(true)
    setError(null)

    try {
      const result = await importWallet(privateKey.trim(), password || undefined)
      setWallet(result.address, result.session_token)
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to import wallet')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <Key className="w-5 h-5 text-poly-green" />
        <span className="font-semibold">Import Existing Wallet</span>
      </div>
      <div>
        <label className="block text-sm text-gray-400 mb-1">
          Private Key
        </label>
        <input
          type="password"
          value={privateKey}
          onChange={(e) => setPrivateKey(e.target.value)}
          placeholder="0x..."
          className="w-full px-3 py-2 bg-poly-dark border border-poly-border rounded focus:outline-none focus:border-poly-green font-mono"
        />
      </div>
      <div>
        <label className="block text-sm text-gray-400 mb-1">
          Password (optional, for encrypted storage)
        </label>
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          placeholder="Enter password..."
          className="w-full px-3 py-2 bg-poly-dark border border-poly-border rounded focus:outline-none focus:border-poly-green"
        />
      </div>
      {error && (
        <div className="text-poly-red text-sm">{error}</div>
      )}
      <div className="flex gap-2">
        <button
          onClick={onClose}
          className="flex-1 py-2 border border-poly-border rounded hover:bg-poly-card transition"
        >
          Cancel
        </button>
        <button
          onClick={handleImport}
          disabled={loading}
          className="flex-1 py-2 bg-poly-green text-black font-semibold rounded hover:bg-poly-green/90 transition disabled:opacity-50"
        >
          {loading ? 'Importing...' : 'Import'}
        </button>
      </div>
    </div>
  )
}
