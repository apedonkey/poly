import { useSignTypedData, useAccount, useChainId } from 'wagmi'
import { useCallback, useState } from 'react'
import type { Address } from 'viem'
import { keccak256, encodeAbiParameters, getCreate2Address } from 'viem'

// Polymarket CTF Exchange contract on Polygon
const CTF_EXCHANGE_ADDRESS = '0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E' as const

// Safe Proxy Factory for deriving proxy wallet address
const SAFE_FACTORY = '0xaacFeEa03eb1561C4e67d661e40682Bd20E3541b' as const
const SAFE_INIT_CODE_HASH = '0x2bce2127ff07fb632d16c8347c4ebf501f4841168bed00d9e6ef715ddb6fcecf' as const

// Derive the Polymarket Safe proxy wallet address from an EOA
function deriveSafeWallet(eoaAddress: Address): Address {
  // Salt = keccak256(abi.encode(address)) - address padded to 32 bytes
  const salt = keccak256(encodeAbiParameters([{ type: 'address' }], [eoaAddress]))
  return getCreate2Address({
    from: SAFE_FACTORY,
    salt,
    bytecodeHash: SAFE_INIT_CODE_HASH,
  })
}

// EIP-712 Domain for Polymarket orders
const POLYMARKET_DOMAIN = {
  name: 'Polymarket CTF Exchange',
  version: '1',
  chainId: 137, // Polygon
  verifyingContract: CTF_EXCHANGE_ADDRESS,
} as const

// EIP-712 Order type definition
const ORDER_TYPES = {
  Order: [
    { name: 'salt', type: 'uint256' },
    { name: 'maker', type: 'address' },
    { name: 'signer', type: 'address' },
    { name: 'taker', type: 'address' },
    { name: 'tokenId', type: 'uint256' },
    { name: 'makerAmount', type: 'uint256' },
    { name: 'takerAmount', type: 'uint256' },
    { name: 'expiration', type: 'uint256' },
    { name: 'nonce', type: 'uint256' },
    { name: 'feeRateBps', type: 'uint256' },
    { name: 'side', type: 'uint8' },
    { name: 'signatureType', type: 'uint8' },
  ],
} as const

// Order sides
export const OrderSide = {
  BUY: 0,
  SELL: 1,
} as const

// Signature types
export const SignatureType = {
  EOA: 0,
  POLY_PROXY: 1,
  POLY_GNOSIS_SAFE: 2,
} as const

export interface OrderParams {
  tokenId: string
  side: 'Yes' | 'No'
  sizeUsdc: string
  price: string
}

export interface SignedOrder {
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

// Generate a random salt for order uniqueness
// Use a smaller salt that fits in JavaScript's safe integer range (2^53)
// The official TS SDK uses parseInt() so salt must be a safe integer
function generateSalt(): bigint {
  const array = new Uint8Array(6) // 48 bits is safely under 2^53
  crypto.getRandomValues(array)
  let salt = BigInt(0)
  for (const byte of array) {
    salt = (salt << BigInt(8)) | BigInt(byte)
  }
  return salt
}

// Convert USDC amount to wei (6 decimals)
function usdcToWei(amount: string): bigint {
  const [whole, decimal = ''] = amount.split('.')
  const paddedDecimal = decimal.padEnd(6, '0').slice(0, 6)
  return BigInt(whole + paddedDecimal)
}

export function usePolymarketSigning() {
  const { address } = useAccount()
  const chainId = useChainId()
  const { signTypedDataAsync } = useSignTypedData()
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const createAndSignOrder = useCallback(async (params: OrderParams): Promise<SignedOrder | null> => {
    if (!address) {
      setError('Wallet not connected')
      return null
    }

    if (chainId !== 137) {
      setError('Please switch to Polygon network')
      return null
    }

    setIsLoading(true)
    setError(null)

    try {
      const salt = generateSalt()
      const now = Math.floor(Date.now() / 1000)
      const expiration = BigInt(now + 60 * 60 * 24) // 24 hours from now

      // Convert USDC size to wei (6 decimals)
      const sizeWei = usdcToWei(params.sizeUsdc)

      // Convert price to calculate token amounts
      // Price is the YES token price (e.g., 0.95 = 95 cents)
      const priceFloat = parseFloat(params.price)

      // For a BUY order:
      // - makerAmount = USDC you're paying (in wei, 6 decimals)
      // - takerAmount = tokens you're receiving (in wei, 6 decimals)
      // If buying YES at $0.95, you pay 0.95 USDC per share
      // If buying NO, the price is (1 - YES_price)

      const effectivePrice = params.side === 'Yes' ? priceFloat : (1 - priceFloat)

      // makerAmount = how much USDC we're spending
      // Round to 2 decimal places (units of 10000 in 6-decimal representation)
      // e.g., 1000000 (1 USDC) -> stays 1000000, 1234567 -> 1230000
      const rawMakerAmount = sizeWei
      const makerAmount = (rawMakerAmount / BigInt(10000)) * BigInt(10000)

      // takerAmount = how many shares we receive (size / price)
      // Round to 4 decimal places (units of 100 in 6-decimal representation)
      // e.g., 1428571 -> 1428500
      const rawSharesWei = BigInt(Math.floor(Number(sizeWei) / effectivePrice))
      const sharesWei = (rawSharesWei / BigInt(100)) * BigInt(100)

      // console.log('Amounts (rounded for precision): maker', makerAmount.toString(), 'taker', sharesWei.toString())

      // Derive proxy wallet address (Polymarket uses Safe proxies for browser wallets)
      const proxyAddress = deriveSafeWallet(address as Address)
      // console.log('EOA:', address, '-> Proxy:', proxyAddress)

      const orderMessage = {
        salt: salt,
        maker: proxyAddress, // Proxy wallet holds the funds
        signer: address as Address, // EOA signs the order
        taker: '0x0000000000000000000000000000000000000000' as Address, // Open order
        tokenId: BigInt(params.tokenId),
        makerAmount: makerAmount,
        takerAmount: sharesWei,
        expiration: expiration,
        nonce: BigInt(0), // Use 0 for first order, backend can track
        feeRateBps: BigInt(0), // No additional fee
        side: OrderSide.BUY, // Always buying
        signatureType: SignatureType.POLY_GNOSIS_SAFE, // Browser wallet with Safe proxy
      }

      // Sign the typed data
      const signature = await signTypedDataAsync({
        domain: POLYMARKET_DOMAIN,
        types: ORDER_TYPES,
        primaryType: 'Order',
        message: orderMessage,
      })

      return {
        salt: salt.toString(),
        maker: proxyAddress,
        signer: address,
        taker: '0x0000000000000000000000000000000000000000',
        tokenId: params.tokenId,
        makerAmount: makerAmount.toString(),
        takerAmount: sharesWei.toString(),
        expiration: expiration.toString(),
        nonce: '0',
        feeRateBps: '0',
        side: OrderSide.BUY,
        signatureType: SignatureType.POLY_GNOSIS_SAFE,
        signature,
      }
    } catch (err) {
      console.error('Signing error:', err)
      if ((err as { code?: number })?.code === 4001) {
        setError('Signature rejected by user')
      } else {
        setError(err instanceof Error ? err.message : 'Failed to sign order')
      }
      return null
    } finally {
      setIsLoading(false)
    }
  }, [address, chainId, signTypedDataAsync])

  return {
    createAndSignOrder,
    isLoading,
    error,
    clearError: () => setError(null),
  }
}
