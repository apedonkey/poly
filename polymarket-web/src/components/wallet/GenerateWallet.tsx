import { useState } from 'react'
import { Wallet, Copy, Check, AlertTriangle } from 'lucide-react'
import { generateWallet } from '../../api/client'
import { useWalletStore } from '../../stores/walletStore'

interface Props {
  onClose: () => void
}

export function GenerateWallet({ onClose }: Props) {
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [generatedKey, setGeneratedKey] = useState<string | null>(null)
  const [generatedWallet, setGeneratedWallet] = useState<{ address: string; sessionToken: string } | null>(null)
  const [copied, setCopied] = useState(false)
  const setWallet = useWalletStore((s) => s.setWallet)

  const handleGenerate = async () => {
    setLoading(true)
    setError(null)

    try {
      const result = await generateWallet(password || undefined)
      // Don't set wallet yet - wait for user to acknowledge they saved the key
      setGeneratedWallet({ address: result.address, sessionToken: result.session_token })
      setGeneratedKey(result.private_key)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to generate wallet')
    } finally {
      setLoading(false)
    }
  }

  const handleAcknowledge = () => {
    if (generatedWallet) {
      // Now connect the wallet after user has seen and saved the key
      setWallet(generatedWallet.address, generatedWallet.sessionToken)
    }
    onClose()
  }

  const copyToClipboard = async () => {
    if (!generatedKey) return
    await navigator.clipboard.writeText(generatedKey)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  if (generatedKey) {
    return (
      <div className="space-y-4">
        <div className="flex items-center gap-2 text-yellow-400">
          <AlertTriangle className="w-5 h-5" />
          <span className="font-semibold">Save Your Private Key!</span>
        </div>
        <p className="text-sm text-gray-400">
          This private key will only be shown <strong>once</strong>.
          Store it securely - you'll need it to trade.
        </p>
        <div className="relative">
          <div className="bg-poly-dark p-3 rounded font-mono text-sm break-all border border-yellow-500/50">
            {generatedKey}
          </div>
          <button
            onClick={copyToClipboard}
            className="absolute top-2 right-2 p-1.5 bg-poly-card rounded hover:bg-poly-border transition"
          >
            {copied ? (
              <Check className="w-4 h-4 text-poly-green" />
            ) : (
              <Copy className="w-4 h-4" />
            )}
          </button>
        </div>
        <button
          onClick={handleAcknowledge}
          className="w-full py-2 bg-poly-green text-black font-semibold rounded hover:bg-poly-green/90 transition"
        >
          I've Saved My Key
        </button>
      </div>
    )
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <Wallet className="w-5 h-5 text-poly-green" />
        <span className="font-semibold">Generate New Wallet</span>
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
          onClick={handleGenerate}
          disabled={loading}
          className="flex-1 py-2 bg-poly-green text-black font-semibold rounded hover:bg-poly-green/90 transition disabled:opacity-50"
        >
          {loading ? 'Generating...' : 'Generate'}
        </button>
      </div>
    </div>
  )
}
