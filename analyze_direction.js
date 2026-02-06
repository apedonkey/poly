const fs = require('fs');

// Load all batches
let allTrades = [];
for (let i = 1; i <= 7; i++) {
  try {
    const batch = JSON.parse(fs.readFileSync(`trades_batch${i}.json`, 'utf8'));
    if (Array.isArray(batch)) {
      allTrades = allTrades.concat(batch);
    }
  } catch (e) {}
}

// Group by conditionId
const byCondition = {};
for (const t of allTrades) {
  if (!byCondition[t.conditionId]) {
    byCondition[t.conditionId] = {
      title: t.title,
      trades: [],
      timestamp: t.timestamp
    };
  }
  byCondition[t.conditionId].trades.push(t);
}

console.log('=== DIRECTIONAL ANALYSIS ===');
console.log('');

// For each market, figure out which side he's betting on (larger position)
const bets = [];

for (const [cid, data] of Object.entries(byCondition)) {
  const upBuys = data.trades.filter(t => t.outcome === 'Up' && t.side === 'BUY');
  const downBuys = data.trades.filter(t => t.outcome === 'Down' && t.side === 'BUY');

  const upSize = upBuys.reduce((s, t) => s + t.size, 0);
  const downSize = downBuys.reduce((s, t) => s + t.size, 0);
  const upCost = upBuys.reduce((s, t) => s + t.size * t.price, 0);
  const downCost = downBuys.reduce((s, t) => s + t.size * t.price, 0);

  const totalCost = upCost + downCost;
  const pairableShares = Math.min(upSize, downSize);

  // Which side is he heavier on?
  const bettingSide = upSize > downSize ? 'Up' : 'Down';
  const orphanSize = Math.abs(upSize - downSize);

  // His orphan cost basis
  let orphanCost = 0;
  let orphanAvgPrice = 0;
  if (bettingSide === 'Up' && upSize > 0) {
    orphanAvgPrice = upCost / upSize;
    orphanCost = orphanSize * orphanAvgPrice;
  } else if (downSize > 0) {
    orphanAvgPrice = downCost / downSize;
    orphanCost = orphanSize * orphanAvgPrice;
  }

  // Merge P&L (cost of paired shares vs $1 return)
  const pairedUpCost = pairableShares * (upCost / upSize || 0);
  const pairedDownCost = pairableShares * (downCost / downSize || 0);
  const mergePnL = pairableShares - pairedUpCost - pairedDownCost;

  bets.push({
    title: data.title,
    timestamp: data.timestamp,
    bettingSide,
    orphanSize,
    orphanCost,
    orphanAvgPrice,
    pairableShares,
    mergePnL,
    totalCost,
    upSize,
    downSize,
    upAvg: upSize > 0 ? upCost / upSize : 0,
    downAvg: downSize > 0 ? downCost / downSize : 0
  });
}

// Sort by timestamp
bets.sort((a, b) => a.timestamp - b.timestamp);

console.log('His position sizing strategy:');
console.log('');

// Analyze the pattern
let totalOrphanUp = 0;
let totalOrphanDown = 0;
let upBets = 0;
let downBets = 0;

for (const b of bets) {
  if (b.bettingSide === 'Up') {
    totalOrphanUp += b.orphanSize;
    upBets++;
  } else {
    totalOrphanDown += b.orphanSize;
    downBets++;
  }
}

console.log(`Bets on UP: ${upBets} markets, ${totalOrphanUp.toFixed(0)} orphan shares`);
console.log(`Bets on DOWN: ${downBets} markets, ${totalOrphanDown.toFixed(0)} orphan shares`);
console.log('');

// Look at the prices he's paying
console.log('=== PRICE ANALYSIS ===');
console.log('');

let cheapSideTotal = 0;
let cheapSideCount = 0;
let expSideTotal = 0;
let expSideCount = 0;

for (const b of bets) {
  const cheapPrice = Math.min(b.upAvg, b.downAvg);
  const expPrice = Math.max(b.upAvg, b.downAvg);

  if (cheapPrice > 0) {
    cheapSideTotal += cheapPrice;
    cheapSideCount++;
  }
  if (expPrice > 0) {
    expSideTotal += expPrice;
    expSideCount++;
  }
}

console.log(`Average cheap side price: $${(cheapSideTotal / cheapSideCount).toFixed(3)}`);
console.log(`Average expensive side price: $${(expSideTotal / expSideCount).toFixed(3)}`);
console.log(`Average pair cost: $${((cheapSideTotal + expSideTotal) / cheapSideCount).toFixed(3)}`);
console.log('');

// His edge: buying cheap side VERY cheap
console.log('=== THE STRATEGY ===');
console.log('');

// Look at the actual fill prices
let subTwenty = 0;
let twentyToThirty = 0;
let thirtyToForty = 0;
let fortyPlus = 0;

for (const b of bets) {
  const cheapPrice = Math.min(b.upAvg, b.downAvg);
  if (cheapPrice < 0.20) subTwenty++;
  else if (cheapPrice < 0.30) twentyToThirty++;
  else if (cheapPrice < 0.40) thirtyToForty++;
  else fortyPlus++;
}

console.log('Cheap side price distribution:');
console.log(`  < 20¢: ${subTwenty} markets`);
console.log(`  20-30¢: ${twentyToThirty} markets`);
console.log(`  30-40¢: ${thirtyToForty} markets`);
console.log(`  > 40¢: ${fortyPlus} markets`);
console.log('');

// The key insight
console.log('=== KEY INSIGHT ===');
console.log('');
console.log('He buys BOTH sides but:');
console.log('1. Buys the cheap side at ~25¢ average');
console.log('2. Buys the expensive side at ~65¢ average');
console.log('3. Total pair cost ~$0.90, but he buys MORE of one side');
console.log('');
console.log('If he buys 1000 Up @ $0.65 and 800 Down @ $0.25:');
console.log('  - Merges 800 pairs → 800 × ($1 - $0.90) = $80 profit');
console.log('  - 200 orphan Up shares @ $0.65 = $130 at risk');
console.log('  - If Up wins: 200 × $1 = $200 return → $70 profit');
console.log('  - If Down wins: 200 × $0 = $0 → $130 loss');
console.log('');
console.log('Net: +$80 (merge) + $70 (if win) = $150 profit');
console.log('     +$80 (merge) - $130 (if lose) = -$50 loss');
console.log('');
console.log('He needs to win ~40% of directional bets to break even!');
console.log('If he has ANY edge predicting direction, he prints money.');

// Show some specific examples
console.log('');
console.log('=== RECENT EXAMPLES ===');
console.log('');

for (const b of bets.slice(-10)) {
  const cheapPrice = Math.min(b.upAvg, b.downAvg).toFixed(2);
  const expPrice = Math.max(b.upAvg, b.downAvg).toFixed(2);
  const pairCost = (parseFloat(cheapPrice) + parseFloat(expPrice)).toFixed(2);

  console.log(`${b.title.slice(0, 45)}`);
  console.log(`  Pair cost: $${pairCost} | Betting: ${b.bettingSide} | Orphans: ${b.orphanSize.toFixed(0)}`);
  console.log(`  Merge P&L: $${b.mergePnL.toFixed(2)} | Orphan risk: $${b.orphanCost.toFixed(2)}`);
  console.log('');
}
