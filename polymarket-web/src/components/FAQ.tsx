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
    question: 'How does paper trading work?',
    answer: 'Paper trading lets you simulate trades without using real money. Your positions are tracked in the database, and you can see how your strategy would have performed. This is perfect for testing strategies before committing real funds. Toggle "Paper Trade" in the trade modal to use this feature.'
  },
  {
    question: 'How do I connect a wallet?',
    answer: 'Click "Connect Wallet" in the top right. You can either generate a new wallet (save your private key securely - it\'s shown only once!), import an existing wallet with your private key, or connect MetaMask. Your wallet needs USDC on Polygon for trading and POL/MATIC for gas fees.'
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
    <div className="mb-6">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-2 text-gray-400 hover:text-white transition mb-4"
      >
        <HelpCircle className="w-5 h-5" />
        <span className="font-medium">Frequently Asked Questions</span>
        <ChevronDown className={`w-4 h-4 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
      </button>

      {isOpen && (
        <div className="bg-poly-card border border-poly-border rounded-xl overflow-hidden">
          {faqs.map((faq, index) => (
            <div key={index} className={index > 0 ? 'border-t border-poly-border' : ''}>
              <button
                onClick={() => toggleItem(index)}
                className="w-full flex items-center justify-between p-4 text-left hover:bg-poly-dark/50 transition"
              >
                <span className="font-medium text-sm">{faq.question}</span>
                <ChevronDown
                  className={`w-4 h-4 text-gray-400 flex-shrink-0 ml-2 transition-transform ${
                    openItems.has(index) ? 'rotate-180' : ''
                  }`}
                />
              </button>
              {openItems.has(index) && (
                <div className="px-4 pb-4 text-sm text-gray-400 leading-relaxed">
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
