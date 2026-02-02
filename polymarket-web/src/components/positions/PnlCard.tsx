import { useRef, useState } from 'react'
import { X, Download } from 'lucide-react'
import type { Position } from '../../types'

interface Props {
  position: Position
  isOpen: boolean
  onClose: () => void
}

export function PnlCard({ position, isOpen, onClose }: Props) {
  const cardRef = useRef<HTMLDivElement>(null)
  const [downloading, setDownloading] = useState(false)

  if (!isOpen) return null

  const pnl = position.pnl ? parseFloat(position.pnl) : 0
  const entryPrice = parseFloat(position.entry_price)
  const exitPrice = position.exit_price ? parseFloat(position.exit_price) : 0
  const size = parseFloat(position.size)
  const isWin = pnl > 0

  const returnPct = size > 0 ? (pnl / size) * 100 : 0

  const formatDate = (dateStr: string) => {
    return new Date(dateStr).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
    })
  }

  const green = '#22c55e'
  const red = '#ef4444'
  const accentColor = isWin ? green : red

  const downloadCard = async () => {
    if (!cardRef.current) return
    setDownloading(true)

    try {
      const html2canvas = (await import('html2canvas')).default
      const canvas = await html2canvas(cardRef.current, {
        backgroundColor: '#111111',
        scale: 2,
        useCORS: true,
        logging: false,
      })

      const link = document.createElement('a')
      link.download = `poly-${isWin ? 'win' : 'loss'}-${Math.abs(pnl).toFixed(0)}.png`
      link.href = canvas.toDataURL('image/png')
      link.click()
    } catch (err) {
      console.error('Failed to download:', err)
    } finally {
      setDownloading(false)
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/90 backdrop-blur-sm"
      onClick={onClose}
    >
      <div className="relative max-w-sm w-full" onClick={(e) => e.stopPropagation()}>
        <button
          onClick={onClose}
          className="absolute -top-3 -right-3 z-10 p-2 bg-neutral-800 rounded-full border border-neutral-700 hover:bg-neutral-700 transition"
        >
          <X className="w-4 h-4" />
        </button>

        {/* Card */}
        <div
          ref={cardRef}
          style={{
            backgroundColor: '#111111',
            borderRadius: '16px',
            overflow: 'hidden',
            fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
            border: '1px solid #262626',
          }}
        >
          {/* Header */}
          <div style={{
            padding: '20px 24px',
            borderBottom: '1px solid #262626',
          }}>
            <div style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
            }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: '10px' }}>
                <span style={{ fontSize: '24px' }}>{isWin ? '😺' : '😿'}</span>
                <div>
                  <div style={{ color: '#888', fontSize: '11px', fontWeight: '500', textTransform: 'uppercase', letterSpacing: '0.5px' }}>
                    Trade Result
                  </div>
                  <div style={{ color: '#fff', fontSize: '14px', fontWeight: '600' }}>
                    {position.strategy === 'ResolutionSniper' ? 'Sniper' : position.strategy === 'Dispute' ? 'Dispute' : 'MC'} Strategy
                  </div>
                </div>
              </div>
              <div style={{
                fontSize: '12px',
                fontWeight: '600',
                textTransform: 'uppercase',
                letterSpacing: '0.5px',
                color: accentColor,
              }}>
                {isWin ? 'Win' : 'Loss'}
              </div>
            </div>
          </div>

          {/* PnL Section */}
          <div style={{ padding: '24px' }}>
            <div style={{ marginBottom: '24px' }}>
              <div style={{ color: '#666', fontSize: '12px', fontWeight: '500', marginBottom: '4px' }}>
                Return
              </div>
              <div style={{
                fontSize: '42px',
                fontWeight: '700',
                color: accentColor,
                lineHeight: '1',
                letterSpacing: '-1px',
              }}>
                {isWin ? '+' : ''}{returnPct.toFixed(1)}%
              </div>
              <div style={{
                fontSize: '18px',
                fontWeight: '600',
                color: accentColor,
                opacity: 0.7,
                marginTop: '4px',
              }}>
                {isWin ? '+' : ''}{pnl.toFixed(2)} USDC
              </div>
            </div>

            {/* Market */}
            <div style={{
              padding: '16px',
              backgroundColor: '#1a1a1a',
              borderRadius: '8px',
              marginBottom: '16px',
            }}>
              <div style={{ color: '#666', fontSize: '11px', fontWeight: '500', textTransform: 'uppercase', letterSpacing: '0.5px', marginBottom: '8px' }}>
                Market
              </div>
              <div style={{ color: '#e5e5e5', fontSize: '14px', fontWeight: '500', lineHeight: '1.4' }}>
                {position.question}
              </div>
            </div>

            {/* Stats */}
            <div style={{ display: 'flex', gap: '12px' }}>
              <div style={{ flex: 1, padding: '12px', backgroundColor: '#1a1a1a', borderRadius: '8px' }}>
                <div style={{ color: '#666', fontSize: '10px', fontWeight: '500', textTransform: 'uppercase', letterSpacing: '0.5px', marginBottom: '4px' }}>
                  Side
                </div>
                <div style={{ color: position.side === 'Yes' ? green : red, fontSize: '16px', fontWeight: '700' }}>
                  {position.side}
                </div>
              </div>
              <div style={{ flex: 1, padding: '12px', backgroundColor: '#1a1a1a', borderRadius: '8px' }}>
                <div style={{ color: '#666', fontSize: '10px', fontWeight: '500', textTransform: 'uppercase', letterSpacing: '0.5px', marginBottom: '4px' }}>
                  Entry
                </div>
                <div style={{ color: '#e5e5e5', fontSize: '16px', fontWeight: '700' }}>
                  {(entryPrice * 100).toFixed(0)}¢
                </div>
              </div>
              <div style={{ flex: 1, padding: '12px', backgroundColor: '#1a1a1a', borderRadius: '8px' }}>
                <div style={{ color: '#666', fontSize: '10px', fontWeight: '500', textTransform: 'uppercase', letterSpacing: '0.5px', marginBottom: '4px' }}>
                  Exit
                </div>
                <div style={{ color: '#e5e5e5', fontSize: '16px', fontWeight: '700' }}>
                  {(exitPrice * 100).toFixed(0)}¢
                </div>
              </div>
            </div>
          </div>

          {/* Footer */}
          <div style={{
            padding: '16px 24px',
            backgroundColor: '#0a0a0a',
            borderTop: '1px solid #262626',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
              <span style={{ fontSize: '14px' }}>🐱</span>
              <span style={{ color: '#666', fontSize: '12px', fontWeight: '600' }}>Poly Bot</span>
            </div>
            <div style={{ color: '#444', fontSize: '11px' }}>
              ${size.toFixed(2)} · {formatDate(position.opened_at)}
            </div>
          </div>
        </div>

        {/* Download button */}
        <button
          onClick={downloadCard}
          disabled={downloading}
          className="w-full mt-4 py-3.5 rounded-xl font-semibold transition flex items-center justify-center gap-2 disabled:opacity-50"
          style={{
            backgroundColor: isWin ? green : '#262626',
            color: isWin ? '#000' : '#fff',
          }}
        >
          <Download className="w-5 h-5" />
          {downloading ? 'Generating...' : 'Download Image'}
        </button>
      </div>
    </div>
  )
}
