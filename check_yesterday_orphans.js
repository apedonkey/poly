const https = require('https');

// Yesterday's orphans (Feb 4)
const orphans = [
  { id: 384, slug: 'btc-updown-15m-1770254100', side: 'YES', price: 0.48, yesBid: 0.485, noBid: 0.485 },
  { id: 356, slug: 'eth-updown-15m-1770246900', side: 'NO', price: 0.49, yesBid: 0.475, noBid: 0.495 },
  { id: 295, slug: 'btc-updown-15m-1770234300', side: 'NO', price: 0.68, yesBid: 0.30, noBid: 0.68 },
  { id: 294, slug: 'btc-updown-15m-1770234300', side: 'NO', price: 0.68, yesBid: 0.30, noBid: 0.68 },
  { id: 293, slug: 'btc-updown-15m-1770234300', side: 'NO', price: 0.68, yesBid: 0.30, noBid: 0.68 },
  { id: 288, slug: 'eth-updown-15m-1770231600', side: 'YES', price: 0.82, yesBid: 0.825, noBid: 0.155 },
  { id: 287, slug: 'eth-updown-15m-1770231600', side: 'YES', price: 0.82, yesBid: 0.825, noBid: 0.155 },
  { id: 285, slug: 'eth-updown-15m-1770231600', side: 'YES', price: 0.82, yesBid: 0.825, noBid: 0.155 },
  { id: 261, slug: 'eth-updown-15m-1770217200', side: 'NO', price: 0.52, yesBid: 0.42, noBid: 0.525 },
  { id: 257, slug: 'btc-updown-15m-1770217200', side: 'NO', price: 0.54, yesBid: 0.41, noBid: 0.545 },
  { id: 256, slug: 'btc-updown-15m-1770217200', side: 'NO', price: 0.54, yesBid: 0.41, noBid: 0.545 },
  { id: 206, slug: 'btc-updown-15m-1770200100', side: 'YES', price: 0.50, yesBid: 0.505, noBid: 0.475 },
  { id: 198, slug: 'eth-updown-15m-1770197400', side: 'YES', price: 0.56, yesBid: 0.565, noBid: 0.415 },
  { id: 197, slug: 'btc-updown-15m-1770196500', side: 'YES', price: 0.31, yesBid: 0.315, noBid: 0.655 },
  { id: 195, slug: 'btc-updown-15m-1770195600', side: 'YES', price: 0.51, yesBid: 0.515, noBid: 0.465 },
  { id: 193, slug: 'btc-updown-15m-1770194700', side: 'NO', price: 0.39, yesBid: 0.575, noBid: 0.39 },
  { id: 192, slug: 'btc-updown-15m-1770193800', side: 'NO', price: 0.43, yesBid: 0.515, noBid: 0.43 },
  { id: 191, slug: 'btc-updown-15m-1770193800', side: 'NO', price: 0.45, yesBid: 0.515, noBid: 0.45 },
  { id: 190, slug: 'btc-updown-15m-1770193800', side: 'NO', price: 0.43, yesBid: 0.515, noBid: 0.43 },
  { id: 187, slug: 'btc-updown-15m-1770192900', side: 'YES', price: 0.50, yesBid: 0.505, noBid: 0.475 },
  { id: 186, slug: 'btc-updown-15m-1770192900', side: 'YES', price: 0.50, yesBid: 0.505, noBid: 0.475 },
  { id: 176, slug: 'btc-updown-15m-1770189300', side: 'YES', price: 0.43, yesBid: 0.43, noBid: 0.53 },
  { id: 174, slug: 'btc-updown-15m-1770188400', side: 'YES', price: 0.44, yesBid: 0.445, noBid: 0.535 },
  { id: 167, slug: 'btc-updown-15m-1770186600', side: 'YES', price: 0.37, yesBid: 0.37, noBid: 0.525 },
  { id: 164, slug: 'btc-updown-15m-1770184800', side: 'NO', price: 0.50, yesBid: 0.39, noBid: 0.505 },
  { id: 155, slug: 'btc-updown-15m-1770183000', side: 'YES', price: 0.31, yesBid: 0.31, noBid: 0.665 },
  { id: 150, slug: 'btc-updown-15m-1770182100', side: 'NO', price: 0.44, yesBid: 0.515, noBid: 0.44 },
  { id: 139, slug: 'btc-updown-15m-1770180300', side: 'YES', price: 0.49, yesBid: 0.495, noBid: 0.485 },
  { id: 138, slug: 'btc-updown-15m-1770180300', side: 'YES', price: 0.49, yesBid: 0.495, noBid: 0.485 },
  { id: 134, slug: 'btc-updown-15m-1770178500', side: 'YES', price: 0.12, yesBid: 0.12, noBid: 0.825 },
  { id: 124, slug: 'btc-updown-15m-1770175800', side: 'NO', price: 0.04, yesBid: 0.95, noBid: 0.04 },
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

  console.log(`Checking ${uniqueSlugs.length} unique markets...`);

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

  console.log('\n=== YESTERDAY\'S ORPHANS (Feb 4) ===\n');

  let expWin = 0, expLoss = 0, cheapWin = 0, cheapLoss = 0;
  let winPnL = 0, lossPnL = 0;

  for (const o of orphans) {
    const winner = resolutions[o.slug];
    const yourBet = o.side === 'YES' ? 'Up' : 'Down';

    // Determine which side was expensive
    const yesExpensive = o.yesBid > o.noBid;
    const filledExpensive = (o.side === 'YES' && yesExpensive) || (o.side === 'NO' && !yesExpensive);

    if (!winner) {
      console.log(`PENDING: #${o.id} ${o.slug.slice(-15)}`);
      continue;
    }

    const won = yourBet === winner;
    const cost = o.price * 10; // assume ~10 shares avg
    const pnl = won ? (10 - cost) : -cost;

    if (filledExpensive) {
      if (won) expWin++; else expLoss++;
    } else {
      if (won) cheapWin++; else cheapLoss++;
    }

    if (won) winPnL += pnl; else lossPnL += pnl;

    const tag = filledExpensive ? '[EXP]' : '[CHEAP]';
    console.log(`${won ? 'WIN ' : 'LOSS'} ${tag}: #${o.id} ${o.side} @ $${o.price.toFixed(2)} (yes=${o.yesBid} no=${o.noBid})`);
  }

  console.log('\n=== BREAKDOWN ===');
  console.log(`Expensive side filled: ${expWin} wins, ${expLoss} losses → ${expWin+expLoss > 0 ? ((expWin/(expWin+expLoss))*100).toFixed(0) : 0}% win rate`);
  console.log(`Cheap side filled: ${cheapWin} wins, ${cheapLoss} losses → ${cheapWin+cheapLoss > 0 ? ((cheapWin/(cheapWin+cheapLoss))*100).toFixed(0) : 0}% win rate`);
  console.log(`\nTotal: ${expWin+cheapWin} wins, ${expLoss+cheapLoss} losses → ${((expWin+cheapWin)/(expWin+cheapWin+expLoss+cheapLoss)*100).toFixed(0)}% win rate`);
}

main().catch(console.error);
