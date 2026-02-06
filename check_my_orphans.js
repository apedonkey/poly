const https = require('https');

// Your one-sided pairs
const orphans = [
  { id: 471, slug: 'eth-updown-15m-1770276600', side: 'YES', price: 0.48, size: 8 },
  { id: 470, slug: 'btc-updown-15m-1770276600', side: 'YES', price: 0.48, size: 8 },
  { id: 469, slug: 'btc-updown-15m-1770275700', side: 'NO', price: 0.49, size: 7 },
  { id: 466, slug: 'eth-updown-15m-1770274800', side: 'NO', price: 0.50, size: 7 },
  { id: 464, slug: 'btc-updown-15m-1770274800', side: 'NO', price: 0.48, size: 8 },
  { id: 463, slug: 'eth-updown-15m-1770273900', side: 'YES', price: 0.50, size: 7 },
  { id: 454, slug: 'eth-updown-15m-1770271200', side: 'YES', price: 0.54, size: 7 },
  { id: 445, slug: 'eth-updown-15m-1770270300', side: 'YES', price: 0.48, size: 10 },
  { id: 441, slug: 'eth-updown-15m-1770267600', side: 'NO', price: 0.49, size: 10 },
  { id: 435, slug: 'btc-updown-15m-1770266700', side: 'YES', price: 0.49, size: 11 },
  { id: 433, slug: 'eth-updown-15m-1770266700', side: 'YES', price: 0.48, size: 11 },
  { id: 430, slug: 'btc-updown-15m-1770264900', side: 'NO', price: 0.50, size: 9 },
  { id: 405, slug: 'eth-updown-15m-1770259500', side: 'YES', price: 0.48, size: 12 },
  { id: 399, slug: 'eth-updown-15m-1770256800', side: 'YES', price: 0.52, size: 10 },
  { id: 398, slug: 'btc-updown-15m-1770256800', side: 'YES', price: 0.49, size: 10 },
  { id: 384, slug: 'btc-updown-15m-1770254100', side: 'YES', price: 0.48, size: 9 },
  { id: 356, slug: 'eth-updown-15m-1770246900', side: 'NO', price: 0.49, size: 7 },
  { id: 295, slug: 'btc-updown-15m-1770234300', side: 'NO', price: 0.68, size: 10 },
  { id: 294, slug: 'btc-updown-15m-1770234300', side: 'NO', price: 0.68, size: 10 },
  { id: 293, slug: 'btc-updown-15m-1770234300', side: 'NO', price: 0.68, size: 10 },
  { id: 288, slug: 'eth-updown-15m-1770231600', side: 'YES', price: 0.82, size: 6 },
  { id: 287, slug: 'eth-updown-15m-1770231600', side: 'YES', price: 0.82, size: 6 },
  { id: 285, slug: 'eth-updown-15m-1770231600', side: 'YES', price: 0.82, size: 6 },
  { id: 261, slug: 'eth-updown-15m-1770217200', side: 'NO', price: 0.52, size: 5 },
  { id: 257, slug: 'btc-updown-15m-1770217200', side: 'NO', price: 0.54, size: 5 },
  { id: 256, slug: 'btc-updown-15m-1770217200', side: 'NO', price: 0.54, size: 5 },
  { id: 206, slug: 'btc-updown-15m-1770200100', side: 'YES', price: 0.50, size: 6 },
  { id: 198, slug: 'eth-updown-15m-1770197400', side: 'YES', price: 0.56, size: 5 },
  { id: 197, slug: 'btc-updown-15m-1770196500', side: 'YES', price: 0.31, size: 5 },
  { id: 195, slug: 'btc-updown-15m-1770195600', side: 'YES', price: 0.51, size: 8 },
];

function fetchEvent(slug) {
  return new Promise((resolve) => {
    const url = `https://gamma-api.polymarket.com/events?slug=${slug}`;
    https.get(url, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        try { resolve(JSON.parse(data)); }
        catch (e) { resolve(null); }
      });
    }).on('error', () => resolve(null));
  });
}

async function main() {
  let wins = 0, losses = 0, pending = 0;
  let winPnL = 0, lossPnL = 0;

  const uniqueSlugs = [...new Set(orphans.map(o => o.slug))];
  const resolutions = {};

  console.log(`Checking ${uniqueSlugs.length} unique markets...`);

  for (const slug of uniqueSlugs) {
    const event = await fetchEvent(slug);
    if (event && event[0] && event[0].markets && event[0].markets[0]) {
      const m = event[0].markets[0];
      let prices = m.outcomePrices;
      if (typeof prices === 'string') {
        try { prices = JSON.parse(prices); } catch(e) {}
      }
      // Up = prices[0], Down = prices[1]
      if (prices && parseFloat(prices[0]) === 1) {
        resolutions[slug] = 'Up';
      } else if (prices && parseFloat(prices[1]) === 1) {
        resolutions[slug] = 'Down';
      } else if (prices && parseFloat(prices[0]) === 0) {
        resolutions[slug] = 'Down';
      } else if (prices && parseFloat(prices[1]) === 0) {
        resolutions[slug] = 'Up';
      }
    }
    await new Promise(r => setTimeout(r, 50));
  }

  console.log('');
  console.log('=== YOUR ORPHAN RESULTS ===');
  console.log('');

  for (const o of orphans) {
    const winner = resolutions[o.slug];
    // YES = Up, NO = Down
    const yourBet = o.side === 'YES' ? 'Up' : 'Down';
    const cost = o.price * o.size;

    if (!winner) {
      pending++;
      console.log(`PENDING: #${o.id} ${o.slug.slice(0,25)} - ${o.size} ${o.side} @ $${o.price}`);
    } else if (yourBet === winner) {
      wins++;
      const pnl = o.size * 1 - cost;
      winPnL += pnl;
      console.log(`WIN:  #${o.id} ${o.slug.slice(0,25)} - ${o.size} ${o.side} @ $${o.price} → +$${pnl.toFixed(2)}`);
    } else {
      losses++;
      lossPnL -= cost;
      console.log(`LOSS: #${o.id} ${o.slug.slice(0,25)} - ${o.size} ${o.side} @ $${o.price} → -$${cost.toFixed(2)}`);
    }
  }

  console.log('');
  console.log('=== SUMMARY ===');
  console.log(`Wins: ${wins}, Losses: ${losses}, Pending: ${pending}`);
  if (wins + losses > 0) {
    console.log(`Win rate: ${(wins / (wins + losses) * 100).toFixed(1)}%`);
  }
  console.log(`Win P&L: +$${winPnL.toFixed(2)}`);
  console.log(`Loss P&L: -$${Math.abs(lossPnL).toFixed(2)}`);
  console.log(`Net orphan P&L: $${(winPnL + lossPnL).toFixed(2)}`);
}

main().catch(console.error);
