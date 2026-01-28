import type {
  GenerateWalletResponse,
  ImportWalletResponse,
  UnlockWalletResponse,
  Opportunity,
  Position,
  BotStats,
  ExecuteTradeRequest,
  AutoTradingSettings,
  AutoTradeLog,
  AutoTradingStats,
} from '../types'

const API_BASE = '/api'

class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message)
    this.name = 'ApiError'
  }
}

async function fetchJson<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await fetch(url, {
    ...options,
    headers: {
      'Content-Type': 'application/json',
      ...options?.headers,
    },
  })

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Unknown error' }))
    throw new ApiError(response.status, error.error || 'Request failed')
  }

  return response.json()
}

// Wallet endpoints
export async function generateWallet(password?: string): Promise<GenerateWalletResponse> {
  return fetchJson(`${API_BASE}/wallet/generate`, {
    method: 'POST',
    body: JSON.stringify({ password }),
  })
}

export async function importWallet(privateKey: string, password?: string): Promise<ImportWalletResponse> {
  return fetchJson(`${API_BASE}/wallet/import`, {
    method: 'POST',
    body: JSON.stringify({ private_key: privateKey, password }),
  })
}

export async function unlockWallet(address: string, password: string): Promise<UnlockWalletResponse> {
  return fetchJson(`${API_BASE}/wallet/unlock`, {
    method: 'POST',
    body: JSON.stringify({ address, password }),
  })
}

// Connect external wallet (MetaMask, etc.)
export async function connectExternalWallet(address: string): Promise<{ address: string; session_token: string }> {
  return fetchJson(`${API_BASE}/wallet/connect`, {
    method: 'POST',
    body: JSON.stringify({ address }),
  })
}

// Get wallet balance
export interface WalletBalance {
  address: string
  usdc_balance: string
  matic_balance: string
}

export async function getWalletBalance(sessionToken?: string, address?: string): Promise<WalletBalance> {
  const params = new URLSearchParams()
  if (address) params.set('address', address)

  const headers: Record<string, string> = {}
  if (sessionToken) headers.Authorization = `Bearer ${sessionToken}`

  return fetchJson(`${API_BASE}/wallet/balance?${params.toString()}`, { headers })
}

// Opportunities endpoint
export async function getOpportunities(strategy?: string): Promise<Opportunity[]> {
  const url = strategy
    ? `${API_BASE}/opportunities?strategy=${strategy}`
    : `${API_BASE}/opportunities`
  return fetchJson(url)
}

// Positions endpoints
interface PositionsResponse {
  positions: Position[]
  total: number
}

export async function getPositions(sessionToken: string): Promise<Position[]> {
  const response = await fetchJson<PositionsResponse>(`${API_BASE}/positions`, {
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
  })
  return response.positions
}

interface StatsResponse {
  stats: BotStats
}

export async function getStats(sessionToken: string): Promise<BotStats> {
  const response = await fetchJson<StatsResponse>(`${API_BASE}/positions/stats`, {
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
  })
  return response.stats
}

// Trade endpoints
export async function executeTrade(
  sessionToken: string,
  request: ExecuteTradeRequest
): Promise<{ success: boolean; position_id?: number; message?: string }> {
  return fetchJson(`${API_BASE}/trades/execute`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify(request),
  })
}

// Signed order request for external wallet live trading
export interface SignedOrderRequest {
  market_id: string
  question: string
  slug?: string
  side: 'Yes' | 'No'
  size_usdc: string
  entry_price: string
  token_id: string
  signed_order: {
    salt: string
    maker: string
    signer: string
    taker: string
    tokenId: string
    makerAmount: string
    takerAmount: string
    expiration: string
    nonce: string
    feeRateBps: string
    side: number
    signatureType: number
    signature: string
  }
  end_date?: string // ISO8601 timestamp when market ends
}

export async function executeSignedTrade(
  sessionToken: string,
  request: SignedOrderRequest
): Promise<{ success: boolean; position_id?: number; order_id?: string; message?: string }> {
  return fetchJson(`${API_BASE}/trades/signed`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify(request),
  })
}

// Helper to decode base64url to Uint8Array
function base64urlDecode(str: string): Uint8Array {
  // Convert base64url to regular base64
  let base64 = str.replace(/-/g, '+').replace(/_/g, '/')
  // Add padding if needed
  while (base64.length % 4) {
    base64 += '='
  }
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes
}

// Helper to encode to base64url (with padding to match Polymarket's Python client)
function base64urlEncode(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer)
  let binary = ''
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i])
  }
  // Keep padding (=) - Polymarket expects padded base64url
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_')
}

// Submit order directly to Polymarket CLOB from browser (bypasses Cloudflare blocking)
export async function submitOrderToClob(
  signedOrder: SignedOrderRequest['signed_order'],
  credentials: { api_key: string; api_secret: string; api_passphrase: string },
  walletAddress: string
): Promise<{ success: boolean; orderId?: string; error?: string }> {
  const CLOB_API = 'https://clob.polymarket.com'

  // Build the order payload per Polymarket docs
  const payload = {
    order: {
      salt: signedOrder.salt,
      maker: signedOrder.maker,
      signer: signedOrder.signer,
      taker: signedOrder.taker,
      tokenId: signedOrder.tokenId,
      makerAmount: signedOrder.makerAmount,
      takerAmount: signedOrder.takerAmount,
      expiration: signedOrder.expiration,
      nonce: signedOrder.nonce,
      feeRateBps: signedOrder.feeRateBps,
      side: signedOrder.side === 0 ? 'BUY' : 'SELL',
      signatureType: signedOrder.signatureType,
      signature: signedOrder.signature,
    },
    owner: signedOrder.maker,  // Use maker (proxy) address as owner
    orderType: 'FOK',
  }

  const body = JSON.stringify(payload)
  const path = '/order'
  const method = 'POST'
  const timestamp = Math.floor(Date.now() / 1000).toString()

  // Create HMAC-SHA256 signature for L2 auth
  // The secret is base64url encoded, so we need to decode it first
  const sigPayload = `${timestamp}${method}${path}${body}`
  const encoder = new TextEncoder()

  // Decode the base64url secret
  const keyData = base64urlDecode(credentials.api_secret)
  const msgData = encoder.encode(sigPayload)

  // Convert to ArrayBuffer for crypto.subtle compatibility
  const keyBuffer = new Uint8Array(keyData).buffer
  const msgBuffer = new Uint8Array(msgData).buffer

  const cryptoKey = await crypto.subtle.importKey(
    'raw',
    keyBuffer,
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign']
  )

  const signatureBuffer = await crypto.subtle.sign('HMAC', cryptoKey, msgBuffer)
  const signature = base64urlEncode(signatureBuffer)

  try {
    const response = await fetch(`${CLOB_API}/order`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'POLY_ADDRESS': walletAddress,
        'POLY_SIGNATURE': signature,
        'POLY_TIMESTAMP': timestamp,
        'POLY_API_KEY': credentials.api_key,
        'POLY_PASSPHRASE': credentials.api_passphrase,
      },
      body,
    })

    if (response.ok) {
      const result = await response.json()
      const orderId = result.orderId || result.orderID || (result.success ? 'submitted' : undefined)
      return { success: true, orderId }
    } else {
      const errorText = await response.text()
      console.error('CLOB order submission failed:', response.status, errorText)
      return { success: false, error: `CLOB API error ${response.status}: ${errorText.slice(0, 200)}` }
    }
  } catch (err) {
    console.error('CLOB order submission error:', err)
    return { success: false, error: err instanceof Error ? err.message : 'Network error' }
  }
}

// Record a position after browser-side CLOB submission
export async function recordPosition(
  sessionToken: string,
  request: Omit<SignedOrderRequest, 'signed_order'> & { order_id?: string; end_date?: string; slug?: string }
): Promise<{ success: boolean; position_id?: number; message?: string }> {
  return fetchJson(`${API_BASE}/trades/record`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify(request),
  })
}

// Close position response (supports partial sells)
export interface ClosePositionResponse {
  success: boolean
  pnl?: string                // PnL from this specific sell
  remaining_shares?: string   // Shares remaining after this sell
  is_fully_closed: boolean    // Whether position is now fully closed
  total_realized_pnl?: string // Total realized PnL from all partial sells
}

// Close a position (mark as sold) - supports full or partial sells
export async function closePosition(
  sessionToken: string,
  positionId: number,
  exitPrice: string,
  orderId?: string,
  sellShares?: string  // If provided, sell only this many shares (partial sell)
): Promise<ClosePositionResponse> {
  return fetchJson(`${API_BASE}/positions/${positionId}/close`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify({
      exit_price: exitPrice,
      order_id: orderId,
      sell_shares: sellShares,
    }),
  })
}

// Update token_id for a position (backfill for existing positions)
export async function updatePositionTokenId(
  sessionToken: string,
  positionId: number,
  tokenId: string
): Promise<{ success: boolean }> {
  return fetchJson(`${API_BASE}/positions/${positionId}/token`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify({ token_id: tokenId }),
  })
}

// Update entry price for a position (fix incorrect entry prices)
export async function updatePositionEntryPrice(
  sessionToken: string,
  positionId: number,
  entryPrice: string
): Promise<{ success: boolean }> {
  return fetchJson(`${API_BASE}/positions/${positionId}/entry-price`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify({ entry_price: entryPrice }),
  })
}

// Fetch market data from Polymarket to get token IDs
export async function fetchMarketTokenIds(conditionId: string): Promise<{ yes_token_id: string; no_token_id: string } | null> {
  try {
    const response = await fetch(`https://clob.polymarket.com/markets/${conditionId}`)
    if (!response.ok) return null

    const data = await response.json()
    // The market response contains tokens array with YES and NO tokens
    if (data.tokens && data.tokens.length >= 2) {
      const yesToken = data.tokens.find((t: { outcome: string }) => t.outcome === 'Yes')
      const noToken = data.tokens.find((t: { outcome: string }) => t.outcome === 'No')
      if (yesToken && noToken) {
        return {
          yes_token_id: yesToken.token_id,
          no_token_id: noToken.token_id,
        }
      }
    }
    return null
  } catch {
    return null
  }
}

// Redeem a resolved winning position (claim USDC)
export async function redeemPosition(
  sessionToken: string,
  positionId: number
): Promise<{ success: boolean; transaction_id?: string; message?: string }> {
  return fetchJson(`${API_BASE}/positions/${positionId}/redeem`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
  })
}

// Auto-Trading endpoints
export async function getAutoTradingSettings(
  sessionToken: string
): Promise<{ settings: AutoTradingSettings }> {
  return fetchJson(`${API_BASE}/auto-trading/settings`, {
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
  })
}

export async function updateAutoTradingSettings(
  sessionToken: string,
  settings: Partial<AutoTradingSettings>
): Promise<{ success: boolean }> {
  return fetchJson(`${API_BASE}/auto-trading/settings`, {
    method: 'PUT',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify(settings),
  })
}

export async function enableAutoTrading(
  sessionToken: string,
  password: string
): Promise<{ success: boolean; message?: string }> {
  return fetchJson(`${API_BASE}/auto-trading/enable`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
    body: JSON.stringify({ password }),
  })
}

export async function disableAutoTrading(sessionToken: string): Promise<{ success: boolean }> {
  return fetchJson(`${API_BASE}/auto-trading/disable`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
  })
}

export async function getAutoTradingHistory(
  sessionToken: string,
  limit?: number
): Promise<{ history: AutoTradeLog[] }> {
  const params = limit ? `?limit=${limit}` : ''
  return fetchJson(`${API_BASE}/auto-trading/history${params}`, {
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
  })
}

export async function getAutoTradingStats(
  sessionToken: string
): Promise<{ stats: AutoTradingStats }> {
  return fetchJson(`${API_BASE}/auto-trading/stats`, {
    headers: {
      Authorization: `Bearer ${sessionToken}`,
    },
  })
}

export { ApiError }
