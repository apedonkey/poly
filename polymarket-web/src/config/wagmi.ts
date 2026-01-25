import { getDefaultConfig } from '@rainbow-me/rainbowkit'
import { polygon } from 'wagmi/chains'

export const config = getDefaultConfig({
  appName: 'Polymarket Bot',
  projectId: 'polymarket-bot', // For WalletConnect - can use any string for dev
  chains: [polygon],
  ssr: false,
})
