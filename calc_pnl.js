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

console.log(`Total trades: ${allTrades.length}`);
console.log('');

// Group by conditionId
const byCondition = {};
for (const t of allTrades) {
  if (!byCondition[t.conditionId]) {
    byCondition[t.conditionId] = { title: t.title, trades: [] };
  }
  byCondition[t.conditionId].trades.push(t);
}

console.log(`Markets: ${Object.keys(byCondition).length}`);
console.log('');

// Calculate P&L for each market
let totalSpent = 0;
let totalMergeReturn = 0;
let totalOrphanShares = 0;
let totalOrphanCost = 0;
let marketsWithOrphans = 0;

const results = [];

for (const [cid, data] of Object.entries(byCondition)) {
  const upBuys = data.trades.filter(t => t.outcome === 'Up' && t.side === 'BUY');
  const downBuys = data.trades.filter(t => t.outcome === 'Down' && t.side === 'BUY');

  const upSize = upBuys.reduce((s, t) => s + t.size, 0);
  const downSize = downBuys.reduce((s, t) => s + t.size, 0);
  const upCost = upBuys.reduce((s, t) => s + t.size * t.price, 0);
  const downCost = downBuys.reduce((s, t) => s + t.size * t.price, 0);

  const totalCost = upCost + downCost;
  const pairableShares = Math.min(upSize, downSize);
  const mergeReturn = pairableShares * 1.0;
  const mergeProfit = mergeReturn - totalCost;

  // Orphan calculation
  const orphanSize = Math.abs(upSize - downSize);
  const orphanSide = upSize > downSize ? 'Up' : 'Down';

  // Cost basis of orphaned shares (proportional)
  let orphanCost = 0;
  if (orphanSize > 0) {
    if (orphanSide === 'Up') {
      const avgUpPrice = upCost / upSize;
      orphanCost = orphanSize * avgUpPrice;
    } else {
      const avgDownPrice = downCost / downSize;
      orphanCost = orphanSize * avgDownPrice;
    }
    marketsWithOrphans++;
    totalOrphanShares += orphanSize;
    totalOrphanCost += orphanCost;
  }

  // Paired cost (cost of the shares that can be merged)
  const pairedCost = totalCost - orphanCost;
  const pureArbitrageProfit = mergeReturn - pairedCost;

  totalSpent += totalCost;
  totalMergeReturn += mergeReturn;

  results.push({
    title: data.title,
    upSize,
    downSize,
    upCost,
    downCost,
    totalCost,
    pairableShares,
    mergeReturn,
    pureArbitrageProfit,
    orphanSize,
    orphanSide,
    orphanCost
  });
}

// Sort by total cost (biggest positions first)
results.sort((a, b) => b.totalCost - a.totalCost);

console.log('=== TOP 20 MARKETS BY SIZE ===');
console.log('');

for (const r of results.slice(0, 20)) {
  const upAvg = r.upSize > 0 ? (r.upCost / r.upSize).toFixed(2) : '0';
  const downAvg = r.downSize > 0 ? (r.downCost / r.downSize).toFixed(2) : '0';
  const pairCost = (r.upSize > 0 && r.downSize > 0) ?
    ((r.upCost / r.upSize) + (r.downCost / r.downSize)).toFixed(2) : 'N/A';

  console.log(r.title.slice(0, 55));
  console.log(`  Up: ${r.upSize.toFixed(1)} @ $${upAvg} = $${r.upCost.toFixed(2)}`);
  console.log(`  Down: ${r.downSize.toFixed(1)} @ $${downAvg} = $${r.downCost.toFixed(2)}`);
  console.log(`  Pair cost: $${pairCost}/share | Pairs: ${r.pairableShares.toFixed(1)} | Arbitrage profit: $${r.pureArbitrageProfit.toFixed(2)}`);
  if (r.orphanSize > 0) {
    console.log(`  ORPHAN: ${r.orphanSize.toFixed(1)} ${r.orphanSide} ($${r.orphanCost.toFixed(2)} at risk)`);
  }
  console.log('');
}

console.log('=== SUMMARY ===');
console.log('');
console.log(`Total spent: $${totalSpent.toFixed(2)}`);
console.log(`Total merge return (if all paired): $${totalMergeReturn.toFixed(2)}`);
console.log(`Pure arbitrage profit (paired only): $${(totalMergeReturn - (totalSpent - totalOrphanCost)).toFixed(2)}`);
console.log('');
console.log(`Markets with orphans: ${marketsWithOrphans}`);
console.log(`Total orphan shares: ${totalOrphanShares.toFixed(2)}`);
console.log(`Total orphan cost (at risk): $${totalOrphanCost.toFixed(2)}`);
console.log('');

// If orphans all lose
const worstCase = totalMergeReturn - totalSpent;
// If orphans all win (worth $1 each)
const bestCase = totalMergeReturn - totalSpent + totalOrphanShares;

console.log(`WORST CASE (orphans = $0): $${worstCase.toFixed(2)}`);
console.log(`BEST CASE (orphans = $1): $${bestCase.toFixed(2)}`);
console.log('');

// Average pair cost
const avgPairCost = (totalSpent - totalOrphanCost) / totalMergeReturn;
console.log(`Average pair cost: $${avgPairCost.toFixed(4)} per $1 merged`);
console.log(`Average spread profit: $${(1 - avgPairCost).toFixed(4)} per share`);
