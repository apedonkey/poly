import { useState } from 'react'
import { ChevronDown, HelpCircle } from 'lucide-react'

interface FAQItem {
  question: string
  answer: string
}

const faqs: FAQItem[] = [
  {
    question: 'What is the Resolution Sniper strategy?',
    answer: 'Resolution Sniper targets markets closing within 12 hours where the favorite has a high probability of winning. Historical data shows that at 4 hours before close, the favorite wins 95.3% of the time. The strategy profits from the gap between the favorite\'s price and this high win rate. For example, if a favorite is priced at 80¢ but wins 95% of the time, there\'s a profitable edge.'
  },
  {
    question: 'What is the NO Bias strategy?',
    answer: 'NO Bias exploits a structural bias in prediction markets: 78.4% of markets resolve to NO. People create markets hoping YES happens, which inflates YES prices. This strategy buys NO when it\'s undervalued relative to the historical resolution rate. If NO is priced at 35¢ but historically wins 78% of the time, that\'s a 43% edge.'
  },
  {
    question: 'How do I connect a wallet?',
    answer: 'Click "Connect Wallet" in the top right and connect with MetaMask, Coinbase Wallet, or WalletConnect. Your trading wallet needs USDC.e (bridged USDC) on Polygon for trading. Polymarket is gasless - you don\'t need POL in your trading wallet! You only need a small amount of POL in your main wallet to transfer USDC.e into your Polymarket trading wallet.'
  },
  {
    question: 'How often do opportunities update?',
    answer: 'The scanner checks for new opportunities every 60 seconds. The opportunities list will automatically refresh with the latest markets matching the Sniper and NO Bias strategies. Prices and market data are fetched in real-time from Polymarket.'
  },
  {
    question: 'What does Edge mean?',
    answer: 'Edge is the difference between the estimated true probability and the market price. For example, if a market prices NO at 35¢ but historical data suggests NO wins 78% of the time, the edge is 43% (0.78 - 0.35). Higher edge means a potentially more profitable opportunity.'
  },
  {
    question: 'What does Return mean?',
    answer: 'Return is the potential profit percentage if your position wins. It\'s calculated as (1 - entry_price) / entry_price. For example, buying at 25¢ gives a potential return of 300% if the market resolves in your favor (you get $1 back for every 25¢ invested).'
  },
  {
    question: 'What does Confidence mean?',
    answer: 'Confidence represents the estimated probability that the position will win, based on historical data and strategy-specific analysis. For Sniper, it\'s based on how often favorites win at that time before close. For NO Bias, it\'s the historical NO resolution rate (78.4%).'
  },
  {
    question: 'What do the filters (Sniper, NO Bias, Crypto, Sports) do?',
    answer: 'Filters let you focus on specific opportunity types. Sniper shows high-probability favorites closing soon (≤12h). NO Bias shows undervalued NO positions. Crypto and Sports filter markets by category. Use the sort dropdown (funnel icon) to order by time, edge, return, or liquidity.'
  },
  {
    question: 'Why does the "Close" time sometimes seem wrong?',
    answer: 'The close time shown is based on the market\'s end_date from the API, which can be misleading. Markets don\'t always resolve when the end_date says - they resolve based on the rules in the market description. For example, a market might show "40m" left but actually resolves when an event happens tomorrow. Always click "Resolution Rules" on the opportunity card to see the actual resolution criteria before trading.'
  },
  {
    question: 'Is this financial advice?',
    answer: 'No. This tool is for informational and educational purposes only. Prediction markets involve risk, and you can lose your entire investment. Always do your own research and never invest more than you can afford to lose. Past performance does not guarantee future results.'
  }
]

export function FAQ() {
  const [isOpen, setIsOpen] = useState(false)
  const [openItems, setOpenItems] = useState<Set<number>>(new Set())

  const toggleItem = (index: number) => {
    const newOpenItems = new Set(openItems)
    if (newOpenItems.has(index)) {
      newOpenItems.delete(index)
    } else {
      newOpenItems.add(index)
    }
    setOpenItems(newOpenItems)
  }

  return (
    <div className="mb-4 sm:mb-6">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-2 text-gray-400 hover:text-white active:text-white transition mb-3 sm:mb-4 touch-target py-1"
      >
        <HelpCircle className="w-4 h-4 sm:w-5 sm:h-5" />
        <span className="font-medium text-sm sm:text-base">FAQ</span>
        <ChevronDown className={`w-4 h-4 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
      </button>

      {isOpen && (
        <div className="bg-poly-card border border-poly-border rounded-xl overflow-hidden">
          {faqs.map((faq, index) => (
            <div key={index} className={index > 0 ? 'border-t border-poly-border' : ''}>
              <button
                onClick={() => toggleItem(index)}
                className="w-full flex items-center justify-between p-3 sm:p-4 text-left hover:bg-poly-dark/50 active:bg-poly-dark/50 transition touch-target"
              >
                <span className="font-medium text-sm pr-2">{faq.question}</span>
                <ChevronDown
                  className={`w-4 h-4 text-gray-400 flex-shrink-0 transition-transform ${
                    openItems.has(index) ? 'rotate-180' : ''
                  }`}
                />
              </button>
              {openItems.has(index) && (
                <div className="px-3 sm:px-4 pb-3 sm:pb-4 text-sm text-gray-400 leading-relaxed max-w-prose">
                  {faq.answer}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
