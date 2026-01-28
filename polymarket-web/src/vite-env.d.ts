/// <reference types="vite/client" />

import { Buffer } from 'buffer'
import { ethers } from 'ethers'

declare global {
  interface Window {
    Buffer: typeof Buffer
    ethereum?: ethers.providers.ExternalProvider
  }
}
