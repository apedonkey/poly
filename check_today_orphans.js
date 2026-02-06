const https = require('https');

// Today's orphans only (since fix)
const orphans = [
  { id: 471, slug: 'eth-updown-15m-1770276600', side: 'YES', price: 0.48, size: 8, exp: 'NO' },
  { id: 470, slug: 'btc-updown-15m-1770276600', side: 'YES', price: 0.48, size: 8, exp: 'NO' },
  { id: 469, slug: 'btc-updown-15m-1770275700', side: 'NO', price: 0.49, size: 7, exp: 'NO' },
  { id: 466, slug: 'eth-updown-15m-1770274800', side: 'NO', price: 0.50, size: 7, exp: 'NO' },
  { id: 464, slug: 'btc-updown-15m-1770274800', side: 'NO', price: 0.48, size: 8, exp: 'NO' },
  { id: 463, slug: 'eth-updown-15m-1770273900', side: 'YES', price: 0.50, size: 7, exp: 'YES' },
  { id: 454, slug: 'eth-updown-15m-1770271200', side: 'YES', price: 0.54, size: 7, exp: 'YES' },
  { id: 445, slug: 'eth-updown-15m-1770270300', side: 'YES', price: 0.48, size: 10, exp: 'NO' },
  { id: 441, slug: 'eth-updown-15m-1770267600', side: 'NO', price: 0.49, size: 10, exp: 'NO' },
  { id: 435, slug: 'btc-updown-15m-1770266700', side: 'YES', price: 0.49, size: 11, exp: 'YES' },
  { id: 433, slug: 'eth-updown-15m-1770266700', side: 'YES', price: 0.48, size: 11, exp: 'NO' },
  { id: 430, slug: 'btc-updown-15m-1770264900', side: 'NO', price: 0.50, size: 9, exp: 'NO' },
  { id: 405, slug: 'eth-updown-15m-1770259500', side: 'YES', price: 0.48, size: 12, exp: 'NO' },
  { id: 399, slug: 'eth-updown-15m-1770256800', side: 'YES', price: 0.52, size: 10, exp: 'YES' },
  { id: 398, slug: 'btc-updown-15m-1770256800', side: 'YES', price: 0.49, size: 10, exp: 'YES' },
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
  const uniqueSlugs = [...new Set(orphans.map(o => o.slug))];
  const resolutions = {};

  for (const slug of uniqueSlugs) {
    const event = await fetchEvent(slug);
    if (event && event[0] && event[0].markets && event[0].markets[0]) {
      const m = event[0].markets[0];
      let prices = m.outcomePrices;
      if (typeof prices === 'string') {
        try { prices = JSON.parse(prices); } catch(e) {}
      }
      if (prices && parseFloat(prices[0]) === 1) resolutions[slug] = 'Up';
      else if (prices && parseFloat(prices[1]) === 1) resolutions[slug] = 'Down';
      else if (prices && parseFloat(prices[0]) === 0) resolutions[slug] = 'Down';
      else if (prices && parseFloat(prices[1]) === 0) resolutions[slug] = 'Up';
    }
    await new Promise(r => setTimeout(r, 50));
  }

  console.log('=== TODAY\'S ORPHANS (since fix) ===\n');

  let expFilledWin = 0, expFilledLoss = 0;
  let cheapFilledWin = 0, cheapFilledLoss = 0;
  let winPnL = 0, lossPnL = 0;

  for (const o of orphans) {
    const winner = resolutions[o.slug];
    const yourBet = o.side === 'YES' ? 'Up' : 'Down';
    const cost = o.price * o.size;
    const filledExpensive = o.side === o.exp;

    if (!winner) {
      console.log(`PENDING: #${o.id}`);
      continue;
    }

    const won = yourBet === winner;
    const pnl = won ? (o.size - cost) : -cost;

    if (filledExpensive) {
      if (won) expFilledWin++; else expFilledLoss++;
    } else {
      if (won) cheapFilledWin++; else cheapFilledLoss++;
    }

    if (won) winPnL += pnl; else lossPnL += pnl;

    const tag = filledExpensive ? '[EXP]' : '[CHEAP]';
    console.log(`${won ? 'WIN ' : 'LOSS'} ${tag}: #${o.id} ${o.side} @ $${o.price} → ${won ? '+' : ''}$${pnl.toFixed(2)}`);
  }

  console.log('\n=== BREAKDOWN ===');
  console.log(`Expensive side filled: ${expFilledWin} wins, ${expFilledLoss} losses (${expFilledWin + expFilledLoss > 0 ? ((expFilledWin/(expFilledWin+expFilledLoss))*100).toFixed(0) : 0}% win rate)`);
  console.log(`Cheap side filled: ${cheapFilledWin} wins, ${cheapFilledLoss} losses (${cheapFilledWin + cheapFilledLoss > 0 ? ((cheapFilledWin/(cheapFilledWin+cheapFilledLoss))*100).toFixed(0) : 0}% win rate)`);
  console.log(`\nNet P&L: $${(winPnL + lossPnL).toFixed(2)}`);
}

main().catch(console.error);
