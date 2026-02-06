const https = require('https');
const fs = require('fs');

const USER_ADDRESS = '0x6031b6eed1c97e853c6e0f03ad3ce3529351f96d';
const LIMIT = 500;
const OUTPUT_FILE = 'all_trades.json';

function fetchTrades(offset) {
  return new Promise((resolve, reject) => {
    const url = `https://data-api.polymarket.com/trades?user=${USER_ADDRESS}&limit=${LIMIT}&offset=${offset}`;
    https.get(url, (res) => {
      let data = '';
      res.on('data', chunk => data += chunk);
      res.on('end', () => {
        try {
          resolve(JSON.parse(data));
        } catch (e) {
          reject(e);
        }
      });
    }).on('error', reject);
  });
}

async function main() {
  let allTrades = [];
  let offset = 0;
  let batch = 0;

  console.log(`Fetching trades for ${USER_ADDRESS}...`);

  while (true) {
    batch++;
    process.stdout.write(`Batch ${batch} (offset ${offset})... `);

    const trades = await fetchTrades(offset);
    console.log(`got ${trades.length} trades`);

    if (trades.length === 0) {
      break;
    }

    allTrades = allTrades.concat(trades);

    if (trades.length < LIMIT) {
      // Last batch
      break;
    }

    offset += LIMIT;

    // Small delay to be nice to the API
    await new Promise(r => setTimeout(r, 200));
  }

  console.log(`\nTotal trades fetched: ${allTrades.length}`);

  // Sort by timestamp (oldest first)
  allTrades.sort((a, b) => a.timestamp - b.timestamp);

  // Save to file
  fs.writeFileSync(OUTPUT_FILE, JSON.stringify(allTrades, null, 2));
  console.log(`Saved to ${OUTPUT_FILE}`);

  // Print summary
  if (allTrades.length > 0) {
    const oldest = new Date(allTrades[0].timestamp * 1000);
    const newest = new Date(allTrades[allTrades.length - 1].timestamp * 1000);
    console.log(`\nDate range: ${oldest.toISOString()} to ${newest.toISOString()}`);

    // Count by side
    const buys = allTrades.filter(t => t.side === 'BUY').length;
    const sells = allTrades.filter(t => t.side === 'SELL').length;
    console.log(`Buys: ${buys}, Sells: ${sells}`);

    // Total volume
    const totalSize = allTrades.reduce((sum, t) => sum + t.size, 0);
    const totalValue = allTrades.reduce((sum, t) => sum + (t.size * t.price), 0);
    console.log(`Total shares: ${totalSize.toFixed(2)}`);
    console.log(`Total value: $${totalValue.toFixed(2)}`);
  }
}

main().catch(console.error);
