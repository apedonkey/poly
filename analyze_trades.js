const fs = require('fs');

// Load all batches
let allTrades = [];
for (let i = 1; i <= 7; i++) {
  try {
    const batch = JSON.parse(fs.readFileSync(`trades_batch${i}.json`, 'utf8'));
    if (Array.isArray(batch)) {
      allTrades = allTrades.concat(batch);
    }
  } catch (e) {
    console.log(`Batch ${i} error:`, e.message);
  }
}

console.log(`Total trades loaded: ${allTrades.length}`);
console.log('');

// Sort by timestamp (oldest first for analysis)
allTrades.sort((a, b) => a.timestamp - b.timestamp);

// Date range
const oldest = new Date(allTrades[0].timestamp * 1000);
const newest = new Date(allTrades[allTrades.length - 1].timestamp * 1000);
console.log(`Date range: ${oldest.toISOString()} to ${newest.toISOString()}`);
console.log('');

// Count by side
const buys = allTrades.filter(t => t.side === 'BUY');
const sells = allTrades.filter(t => t.side === 'SELL');
console.log(`Buys: ${buys.length}, Sells: ${sells.length}`);

// Total volume
const totalBuySize = buys.reduce((sum, t) => sum + t.size, 0);
const totalSellSize = sells.reduce((sum, t) => sum + t.size, 0);
const totalBuyValue = buys.reduce((sum, t) => sum + (t.size * t.price), 0);
const totalSellValue = sells.reduce((sum, t) => sum + (t.size * t.price), 0);
console.log(`Buy volume: ${totalBuySize.toFixed(2)} shares ($${totalBuyValue.toFixed(2)})`);
console.log(`Sell volume: ${totalSellSize.toFixed(2)} shares ($${totalSellValue.toFixed(2)})`);
console.log('');

// Group by conditionId to see paired trades
const byCondition = {};
for (const t of allTrades) {
  if (!byCondition[t.conditionId]) {
    byCondition[t.conditionId] = [];
  }
  byCondition[t.conditionId].push(t);
}

console.log(`Unique markets traded: ${Object.keys(byCondition).length}`);
console.log('');

// Analyze trade patterns
console.log('=== TRADE PATTERN ANALYSIS ===');
console.log('');

// Look for pairs (YES + NO buys on same market)
let pairedMarkets = 0;
let yesOnlyMarkets = 0;
let noOnlyMarkets = 0;
let mixedMarkets = 0;

for (const [conditionId, trades] of Object.entries(byCondition)) {
  const outcomes = new Set(trades.map(t => t.outcome));
  const sides = new Set(trades.map(t => t.side));

  if (outcomes.has('Up') && outcomes.has('Down')) {
    pairedMarkets++;
  } else if (outcomes.has('Up')) {
    yesOnlyMarkets++;
  } else if (outcomes.has('Down')) {
    noOnlyMarkets++;
  } else {
    mixedMarkets++;
  }
}

console.log(`Paired (YES+NO): ${pairedMarkets} markets`);
console.log(`YES only: ${yesOnlyMarkets} markets`);
console.log(`NO only: ${noOnlyMarkets} markets`);
console.log(`Other: ${mixedMarkets} markets`);
console.log('');

// Look at recent trades in detail
console.log('=== RECENT TRADES (last 50) ===');
console.log('');

const recent = allTrades.slice(-50);
for (const t of recent) {
  const time = new Date(t.timestamp * 1000).toISOString().slice(11, 19);
  const market = t.title.replace('Bitcoin Up or Down - ', 'BTC ').replace('Ethereum Up or Down - ', 'ETH ');
  console.log(`${time} ${t.side.padEnd(4)} ${t.outcome.padEnd(4)} ${t.size.toFixed(2).padStart(8)} @ $${t.price.toFixed(2)} = $${(t.size * t.price).toFixed(2).padStart(7)}  ${market.slice(0, 40)}`);
}

// Group recent by market to see pairing
console.log('');
console.log('=== RECENT MARKET BREAKDOWN ===');
console.log('');

const recentByMarket = {};
for (const t of recent) {
  const key = t.conditionId;
  if (!recentByMarket[key]) {
    recentByMarket[key] = { title: t.title, trades: [] };
  }
  recentByMarket[key].trades.push(t);
}

for (const [cid, data] of Object.entries(recentByMarket)) {
  const upBuys = data.trades.filter(t => t.outcome === 'Up' && t.side === 'BUY');
  const downBuys = data.trades.filter(t => t.outcome === 'Down' && t.side === 'BUY');
  const upSells = data.trades.filter(t => t.outcome === 'Up' && t.side === 'SELL');
  const downSells = data.trades.filter(t => t.outcome === 'Down' && t.side === 'SELL');

  const upBuySize = upBuys.reduce((s, t) => s + t.size, 0);
  const downBuySize = downBuys.reduce((s, t) => s + t.size, 0);
  const upBuyCost = upBuys.reduce((s, t) => s + t.size * t.price, 0);
  const downBuyCost = downBuys.reduce((s, t) => s + t.size * t.price, 0);

  const totalCost = upBuyCost + downBuyCost;
  const minShares = Math.min(upBuySize, downBuySize);
  const mergeReturn = minShares * 1.0;
  const profit = mergeReturn - totalCost;

  console.log(`${data.title.slice(0, 50)}`);
  console.log(`  Up buys: ${upBuySize.toFixed(2)} shares @ avg $${upBuySize > 0 ? (upBuyCost/upBuySize).toFixed(2) : '0'} = $${upBuyCost.toFixed(2)}`);
  console.log(`  Down buys: ${downBuySize.toFixed(2)} shares @ avg $${downBuySize > 0 ? (downBuyCost/downBuySize).toFixed(2) : '0'} = $${downBuyCost.toFixed(2)}`);
  if (upBuySize > 0 && downBuySize > 0) {
    console.log(`  Merge: ${minShares.toFixed(2)} pairs @ $1 = $${mergeReturn.toFixed(2)}, cost=$${totalCost.toFixed(2)}, profit=$${profit.toFixed(2)}`);
    if (upBuySize !== downBuySize) {
      const orphan = Math.abs(upBuySize - downBuySize);
      const orphanSide = upBuySize > downBuySize ? 'Up' : 'Down';
      console.log(`  ORPHAN: ${orphan.toFixed(2)} ${orphanSide} shares (directional risk)`);
    }
  }
  console.log('');
}

// Save to file
fs.writeFileSync('trades_analysis.txt', `
TRADE ANALYSIS FOR 0x6031b6eed1c97e853c6e0f03ad3ce3529351f96d
=============================================================

Total trades: ${allTrades.length}
Date range: ${oldest.toISOString()} to ${newest.toISOString()}

Buys: ${buys.length} (${totalBuySize.toFixed(2)} shares, $${totalBuyValue.toFixed(2)})
Sells: ${sells.length} (${totalSellSize.toFixed(2)} shares, $${totalSellValue.toFixed(2)})

Unique markets: ${Object.keys(byCondition).length}
Paired (YES+NO): ${pairedMarkets}
YES only: ${yesOnlyMarkets}
NO only: ${noOnlyMarkets}
`);

console.log('Analysis saved to trades_analysis.txt');
