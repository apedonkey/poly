const POLYGON_RPC = 'https://polygon-rpc.com'

// Simple request queue to prevent overwhelming the RPC
let requestQueue: Promise<any> = Promise.resolve()
let lastRequestTime = 0
const MIN_REQUEST_INTERVAL = 100 // 100ms between requests

// Helper to make RPC calls with retry and request queuing
export async function rpcCallWithRetry(body: any, maxRetries = 3): Promise<any> {
  // Queue requests to prevent simultaneous calls
  const executeRequest = async (): Promise<any> => {
    // Enforce minimum interval between requests
    const now = Date.now()
    const timeSinceLastRequest = now - lastRequestTime
    if (timeSinceLastRequest < MIN_REQUEST_INTERVAL) {
      await new Promise(resolve => setTimeout(resolve, MIN_REQUEST_INTERVAL - timeSinceLastRequest))
    }
    lastRequestTime = Date.now()

    let lastError: Error | null = null

    for (let attempt = 0; attempt < maxRetries; attempt++) {
      try {
        const controller = new AbortController()
        const timeoutId = setTimeout(() => controller.abort(), 10000) // 10s timeout

        const response = await fetch(POLYGON_RPC, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
          signal: controller.signal,
        })

        clearTimeout(timeoutId)

        if (response.status === 429) {
          // Rate limited - wait longer before retry
          const waitTime = 2000 * (attempt + 1) // 2s, 4s, 6s
          console.log(`RPC rate limited (429), waiting ${waitTime}ms before retry`)
          await new Promise(resolve => setTimeout(resolve, waitTime))
          continue
        }

        if (!response.ok) {
          throw new Error(`RPC request failed: ${response.status}`)
        }

        return await response.json()
      } catch (err: any) {
        lastError = err
        if (err.name === 'AbortError') {
          console.log('RPC timeout, retrying...')
        }
        // Wait before retry
        await new Promise(resolve => setTimeout(resolve, 1000 * (attempt + 1)))
      }
    }

    throw lastError || new Error('RPC request failed after retries')
  }

  // Chain requests to avoid parallel calls
  requestQueue = requestQueue.then(executeRequest, executeRequest)
  return requestQueue
}
