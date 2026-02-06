#!/bin/bash
# Mint Maker performance stats — run from the polymarket-bot directory
# Usage: bash stats.sh [hours_back]
#   hours_back: how many hours to look back (default: 24)

DB="polymarket.db"
HOURS="${1:-24}"

if [ ! -f "$DB" ]; then
    echo "Error: $DB not found. Run from the polymarket-bot directory."
    exit 1
fi

echo "=== Mint Maker Stats (last ${HOURS}h) ==="
echo ""

echo "--- Status Breakdown ---"
sqlite3 "$DB" "
SELECT status, COUNT(*) as count
FROM mint_maker_pairs
WHERE created_at > datetime('now', '-${HOURS} hours')
GROUP BY status
ORDER BY count DESC;
"

echo ""
echo "--- ExpPlaced Flow ---"
sqlite3 "$DB" "
SELECT
  (SELECT COUNT(*) FROM mint_maker_pairs WHERE status = 'ExpPlaced' AND created_at > datetime('now', '-${HOURS} hours')) as 'Waiting',
  (SELECT COUNT(*) FROM mint_maker_pairs WHERE status = 'Cancelled' AND created_at > datetime('now', '-${HOURS} hours')) as 'Cancelled (\$0 loss)',
  (SELECT COUNT(*) FROM mint_maker_pairs WHERE status IN ('HalfFilled','Matched','Merged','Merging') AND created_at > datetime('now', '-${HOURS} hours')) as 'Progressed';
"

echo ""
echo "--- Completed Pairs (Matched/Merged) ---"
sqlite3 "$DB" "
SELECT
  COUNT(*) as pairs,
  ROUND(SUM(CAST(pair_cost AS REAL) * CAST(size AS REAL)), 2) as total_cost,
  ROUND(SUM(CAST(profit AS REAL) * CAST(size AS REAL)), 2) as total_profit,
  ROUND(AVG(CAST(pair_cost AS REAL)), 4) as avg_pair_cost,
  ROUND(AVG(CAST(profit AS REAL)), 4) as avg_profit_per_share
FROM mint_maker_pairs
WHERE status IN ('Matched', 'Merged')
  AND created_at > datetime('now', '-${HOURS} hours')
  AND pair_cost IS NOT NULL;
"

echo ""
echo "--- Orphaned (half-fill losses) ---"
sqlite3 "$DB" "
SELECT
  COUNT(*) as orphaned_pairs,
  ROUND(SUM(CAST(size AS REAL) * COALESCE(CAST(yes_fill_price AS REAL), CAST(no_fill_price AS REAL), 0)), 2) as capital_at_risk
FROM mint_maker_pairs
WHERE status = 'Orphaned'
  AND created_at > datetime('now', '-${HOURS} hours');
"

echo ""
echo "--- By Asset ---"
sqlite3 "$DB" "
SELECT
  asset,
  SUM(CASE WHEN status IN ('Matched','Merged') THEN 1 ELSE 0 END) as matched,
  SUM(CASE WHEN status = 'Cancelled' THEN 1 ELSE 0 END) as cancelled,
  SUM(CASE WHEN status = 'Orphaned' THEN 1 ELSE 0 END) as orphaned,
  SUM(CASE WHEN status = 'ExpPlaced' THEN 1 ELSE 0 END) as waiting,
  SUM(CASE WHEN status = 'HalfFilled' THEN 1 ELSE 0 END) as half_filled,
  ROUND(SUM(CASE WHEN status IN ('Matched','Merged') AND profit IS NOT NULL THEN CAST(profit AS REAL) * CAST(size AS REAL) ELSE 0 END), 2) as profit
FROM mint_maker_pairs
WHERE created_at > datetime('now', '-${HOURS} hours')
GROUP BY asset
ORDER BY matched DESC;
"

echo ""
echo "--- Recent Activity (last 10 actions) ---"
sqlite3 "$DB" "
SELECT
  substr(created_at, 1, 19) as time,
  action,
  asset,
  details
FROM mint_maker_log
ORDER BY created_at DESC
LIMIT 10;
"

echo ""
echo "--- Current Open Positions ---"
sqlite3 "$DB" "
SELECT status, COUNT(*) as count
FROM mint_maker_pairs
WHERE status IN ('ExpPlaced', 'Pending', 'HalfFilled', 'Matched', 'Merging', 'Orphaned')
GROUP BY status
ORDER BY count DESC;
"
