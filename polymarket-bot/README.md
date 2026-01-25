# Polymarket Trading Bot

A Rust-based trading bot for Polymarket prediction markets implementing two complementary strategies.

## Strategies

### 1. Resolution Sniping (Primary)

Bet on heavy favorites in markets about to close. Historical data shows:
- **4 hours before close**: 95.3% accuracy
- **12 hours before close**: 90.6% accuracy

The profit is in the **gap** between the favorite's price and the expected win rate.

**Sweet spot**: Favorites priced at 70-90% (NOT 95-99% where accuracy is already priced in)

### 2. NO Bias (Secondary)

Exploit the structural bias that **78.4% of all markets resolve NO**.

People create markets hoping YES happens, which inflates YES prices. Buy NO when it's undervalued.

## Installation

```bash
# Clone and build
cd polymarket-bot
cargo build --release

# Copy environment template
cp .env.example .env
```

## Configuration

Edit `.env`:

```bash
# Only needed for live trading (leave empty for paper trading)
POLYMARKET_PRIVATE_KEY=

# Database path
DATABASE_PATH=polymarket.db

# Paper trading mode - KEEP TRUE until strategy is validated
PAPER_TRADING=true

# Risk management
MAX_POSITION_SIZE=50
MAX_TOTAL_EXPOSURE=500

# Logging level
RUST_LOG=info
```

## Usage

### Scan all opportunities

```bash
cargo run --release -- scan
```

Output:
```
SNIPER OPPORTUNITIES (closing in 1-12 hours)
----------------------------------------------------------------------
1. "Will PSV win on 2026-01-24?"
   YES at 72c (favorite) | Liquidity: $145K
   Return: 38.9% | EV: 23.3% | Time: 3.2h

NO BIAS OPPORTUNITIES (longer dated)
----------------------------------------------------------------------
1. "Will Trump visit Germany in 2026?"
   NO at 20c | Edge: 58.4% vs 78.4% base rate
   Liquidity: $6K | Time: 48.6w
```

### Sniper mode only

```bash
# Markets closing within 12 hours (default)
cargo run --release -- snipe

# Custom time window
cargo run --release -- snipe --max-hours 6
```

### NO bias mode only

```bash
# Default 10% minimum edge
cargo run --release -- bias

# Custom minimum edge
cargo run --release -- bias --min-edge 20
```

### Continuous scanning

```bash
# Scan every 60 seconds (paper trading)
cargo run --release -- run

# With auto-execution above 15% EV threshold
cargo run --release -- run --auto-execute 15

# Custom interval
cargo run --release -- run --interval 30
```

### View statistics

```bash
cargo run --release -- stats
```

## Strategy Logic

### Resolution Sniper

```
Market closes in 4 hours
NO price: 80¢ (favorite)

Expected accuracy at 4h: 95.3%
Potential return: (100-80)/80 = 25%

EV = (0.953 × 0.20) - (0.047 × 0.80)
   = 0.1906 - 0.0376 = 15.3%

→ Strong opportunity
```

### NO Bias

```
Historical NO resolution rate: 78.4%
Current NO price: 35¢

Edge = 0.784 - 0.35 = 43.4%

→ NO is significantly underpriced
```

## Filters

### Sniper Strategy
- Time to close: 1-12 hours (4-6h is peak)
- Favorite price: 70-90% (gap for profit)
- Minimum liquidity: $1,000
- Minimum EV: 5%

### NO Bias Strategy
- Minimum edge: 10% vs historical rate
- YES price range: 20-80% (not already decided)
- Excludes: Sports, Crypto (fairly priced)
- Minimum liquidity: $1,000

## Important Notes

1. **Paper trading first** - Validate strategy before real money
2. **US users** - Run on non-US VPS (Hetzner, OVH)
3. **Resolution lag** - Capital locked until market resolves
4. **Fast sources** - Prefer sports/crypto (quick resolution)

## Project Structure

```
polymarket-bot/
├── Cargo.toml
├── .env.example
├── README.md
└── src/
    ├── main.rs          # CLI interface
    ├── lib.rs           # Library exports
    ├── config.rs        # Configuration
    ├── types.rs         # Core types
    ├── scanner.rs       # Gamma API client
    ├── db.rs            # SQLite persistence
    ├── executor.rs      # Order execution
    └── strategies/
        ├── mod.rs       # Strategy traits
        ├── sniper.rs    # Resolution sniping
        └── no_bias.rs   # NO bias strategy
```

## License

MIT
