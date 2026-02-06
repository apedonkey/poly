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
      firstTs: t.timestamp,
      lastTs: t.timestamp
    };
  }
  byCondition[t.conditionId].trades.push(t);
  byCondition[t.conditionId].firstTs = Math.min(byCondition[t.conditionId].firstTs, t.timestamp);
  byCondition[t.conditionId].lastTs = Math.max(byCondition[t.conditionId].lastTs, t.timestamp);
}

console.log('=== FILL PATTERN ANALYSIS ===');
console.log('');
console.log('Looking at HOW his orders fill over time...');
console.log('');

// For each market, look at the sequence of fills
let totalPaired = 0;
let totalOrphaned = 0;
let perfectPairs = 0;
let imbalancedPairs = 0;

const markets = Object.values(byCondition);
markets.sort((a, b) => a.firstTs - b.firstTs);

for (const m of markets.slice(-15)) {
  console.log(`${m.title}`);

  // Sort trades by timestamp
  m.trades.sort((a, b) => a.timestamp - b.timestamp);

  // Track running totals
  let upTotal = 0;
  let downTotal = 0;
  let upCost = 0;
  let downCost = 0;

  // Show fill sequence
  const fills = [];
  for (const t of m.trades) {
    const time = new Date(t.timestamp * 1000).toISOString().slice(11, 19);
    if (t.outcome === 'Up') {
      upTotal += t.size;
      upCost += t.size * t.price;
    } else {
      downTotal += t.size;
      downCost += t.size * t.price;
    }
    fills.push({
      time,
      side: t.outcome,
      size: t.size,
      price: t.price,
      upTotal,
      downTotal
    });
  }

  // Show first and last few fills
  const showFills = fills.length <= 6 ? fills : [...fills.slice(0, 3), '...', ...fills.slice(-3)];
  for (const f of showFills) {
    if (f === '...') {
      console.log(`  ...${fills.length - 6} more fills...`);
    } else {
      const bal = f.upTotal - f.downTotal;
      const balStr = bal > 0 ? `+${bal.toFixed(0)} Up` : bal < 0 ? `+${Math.abs(bal).toFixed(0)} Down` : 'balanced';
      console.log(`  ${f.time} ${f.side.padEnd(4)} ${f.size.toFixed(1).padStart(6)} @ $${f.price.toFixed(2)} → Up:${f.upTotal.toFixed(0)} Down:${f.downTotal.toFixed(0)} (${balStr})`);
    }
  }

  // Final stats
  const paired = Math.min(upTotal, downTotal);
  const orphaned = Math.abs(upTotal - downTotal);
  const orphanSide = upTotal > downTotal ? 'Up' : 'Down';
  const pairCost = (upCost / upTotal) + (downCost / downTotal);

  totalPaired += paired;
  totalOrphaned += orphaned;

  if (orphaned < paired * 0.05) {
    perfectPairs++;
  } else {
    imbalancedPairs++;
  }

  console.log(`  RESULT: ${paired.toFixed(0)} paired @ $${pairCost.toFixed(2)}, ${orphaned.toFixed(0)} orphan ${orphanSide}`);
  console.log('');
}

console.log('=== SUMMARY ===');
console.log(`Total paired: ${totalPaired.toFixed(0)} shares`);
console.log(`Total orphaned: ${totalOrphaned.toFixed(0)} shares`);
console.log(`Orphan rate: ${(totalOrphaned / (totalPaired + totalOrphaned) * 100).toFixed(1)}%`);
console.log(`Perfect pairs: ${perfectPairs}, Imbalanced: ${imbalancedPairs}`);
