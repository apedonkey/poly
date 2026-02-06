const https = require('https');
const fs = require('fs');

// Load trades
let allTrades = [];
for (let i = 1; i <= 7; i++) {
  try {
    const batch = JSON.parse(fs.readFileSync(`trades_batch${i}.json`, 'utf8'));
    if (Array.isArray(batch)) allTrades = allTrades.concat(batch);
  } catch (e) {}
}

// Group by market
const byCondition = {};
for (const t of allTrades) {
  if (!byCondition[t.conditionId]) {
    byCondition[t.conditionId] = {
      slug: t.eventSlug,
      title: t.title,
      trades: []
    };
  }
  byCondition[t.conditionId].trades.push(t);
}

// Get unique slugs
const slugs = [...new Set(Object.values(byCondition).map(m => m.slug))];
console.log(`Checking ${slugs.length} markets for resolution...`);

function fetchEvent(slug) {
  return new Promise((resolve, reject) => {
    const url = `https://gamma-api.polymarket.com/events?slug=${slug}`;
    https.get(url, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        try {
          resolve(JSON.parse(data));
        } catch (e) {
          resolve(null);
        }
      });
    }).on('error', () => resolve(null));
  });
}

async function main() {
  const resolutions = {};

  for (const slug of slugs) {
    const event = await fetchEvent(slug);
    if (event && event[0] && event[0].markets && event[0].markets[0]) {
      const m = event[0].markets[0];
      let prices = m.outcomePrices;
      // Parse if string
      if (typeof prices === 'string') {
        try { prices = JSON.parse(prices); } catch(e) {}
      }
      if (prices && prices[0] === "1") {
        resolutions[m.conditionId] = 'Up';
      } else if (prices && prices[1] === "1") {
        resolutions[m.conditionId] = 'Down';
      } else if (prices && parseFloat(prices[0]) === 0) {
        resolutions[m.conditionId] = 'Down';  // Up = 0 means Down won
      } else if (prices && parseFloat(prices[1]) === 0) {
        resolutions[m.conditionId] = 'Up';    // Down = 0 means Up won
      }
    }
    await new Promise(r => setTimeout(r, 100)); // rate limit
  }

  console.log(`Got resolution for ${Object.keys(resolutions).length} markets`);
  console.log('');

  // Calculate orphan P&L
  let orphanWins = 0;
  let orphanLosses = 0;
  let orphanWinValue = 0;
  let orphanLossValue = 0;
  let mergeProfit = 0;

  for (const [cid, data] of Object.entries(byCondition)) {
    const winner = resolutions[cid];
    if (!winner) continue;

    const up = data.trades.filter(t => t.outcome === 'Up');
    const down = data.trades.filter(t => t.outcome === 'Down');
    const upSize = up.reduce((s, t) => s + t.size, 0);
    const downSize = down.reduce((s, t) => s + t.size, 0);
    const upCost = up.reduce((s, t) => s + t.size * t.price, 0);
    const downCost = down.reduce((s, t) => s + t.size * t.price, 0);

    if (upSize === 0 || downSize === 0) continue;

    const paired = Math.min(upSize, downSize);
    const pairCost = (upCost / upSize + downCost / downSize);
    mergeProfit += paired * (1 - pairCost);

    // Orphan calculation
    const orphanSize = Math.abs(upSize - downSize);
    const orphanSide = upSize > downSize ? 'Up' : 'Down';

    if (orphanSize > 0) {
      const orphanAvgPrice = orphanSide === 'Up' ? upCost / upSize : downCost / downSize;
      const orphanCost = orphanSize * orphanAvgPrice;

      if (orphanSide === winner) {
        // Orphan won! Worth $1 each
        orphanWins++;
        const pnl = orphanSize * 1 - orphanCost;
        orphanWinValue += pnl;
        console.log(`WIN: ${data.title.slice(0, 40)} - ${orphanSize.toFixed(0)} ${orphanSide} @ $${orphanAvgPrice.toFixed(2)} → +$${pnl.toFixed(2)}`);
      } else {
        // Orphan lost, worth $0
        orphanLosses++;
        orphanLossValue -= orphanCost;
        console.log(`LOSS: ${data.title.slice(0, 40)} - ${orphanSize.toFixed(0)} ${orphanSide} @ $${orphanAvgPrice.toFixed(2)} → -$${orphanCost.toFixed(2)}`);
      }
    }
  }

  console.log('');
  console.log('=== RESULTS ===');
  console.log(`Orphan wins: ${orphanWins}, Losses: ${orphanLosses}`);
  console.log(`Win rate: ${(orphanWins / (orphanWins + orphanLosses) * 100).toFixed(1)}%`);
  console.log('');
  console.log(`Merge P&L: $${mergeProfit.toFixed(2)}`);
  console.log(`Orphan wins P&L: +$${orphanWinValue.toFixed(2)}`);
  console.log(`Orphan losses P&L: -$${Math.abs(orphanLossValue).toFixed(2)}`);
  console.log('');
  console.log(`TOTAL P&L: $${(mergeProfit + orphanWinValue + orphanLossValue).toFixed(2)}`);
}

main().catch(console.error);
