# Trading Data

The aim is to get a rough idea of the load that can be envisioned for a successful order book in the ICP ecosystem. 
We focus on Binance as the primary benchmark because it handles 25-40x more ICP volume than the next largest exchange (Kraken):

| Metric             | Binance ICP/USDT | Kraken ICP/USD | Ratio |
|--------------------|-----------------|----------------|-------|
| Trades/24h         | 15,726          | 644            | 24x   |
| Volume/24h (ICP)   | 1,190,221       | 28,933         | 41x   |
| Book depth (levels) | 5,697          | 761            | 7.5x  |
| Peak trades/hour   | 143,932         | 3,532          | 41x   |

Both exchanges peaked at the same hour (2026-03-11 06:00 UTC) with similar peak-to-average ratios (~30-54x), confirming that burst patterns are market-wide, not exchange-specific.

The trading data below was obtained from Binance public API on 2026-04-04. Interesting pairs related to ICP:
* ICP/BTC: pure crypto trading pair
* ICP/USDT: most active trading pair involving ICP


## Overview of ICP Trading Pairs on Binance (24h snapshot, 2026-04-04)

| Pair      | Trades/24h | Volume (ICP)  | Quote Volume        |
|-----------|-----------|---------------|---------------------|
| ICP/USDT  | 15,754    | 1,190,221     | $2,730,276          |
| ICP/BNB   | 4,000     | 36,058        | 567 BNB             |
| ICP/BUSD  | 4,000     | 112,270       | $442,288            |
| ICP/ETH   | 4,000     | 70,381        | 84 ETH              |
| ICP/USDC  | 3,680     | 241,641       | $554,496            |
| ICP/TRY   | 475       | 32,241        | 3,289,432 TRY       |
| ICP/FDUSD | 184       | 2,879         | $6,597              |
| ICP/BTC   | 132       | 6,993         | 0.24 BTC            |
| ICP/EUR   | 131       | 5,017         | 10,003 EUR          |
| ICP/RUB   | 0         | 0             | 0 RUB               |

ICP/USDT is the busiest pair by far (~4x the runner-up). BNB, BUSD, and ETH pairs all show exactly 4,000 trades, likely reflecting Binance's internal market-making bots rather than organic volume.

Fetched by querying all ICP pairs from the exchange info endpoint, then their 24h tickers:

```bash
# List all ICP pairs
curl -s 'https://api.binance.com/api/v3/exchangeInfo' | python3 -c "
import json,sys; info=json.load(sys.stdin)
for s in info['symbols']:
    if s['baseAsset']=='ICP' or s['quoteAsset']=='ICP': print(s['symbol'])"

# Get 24h stats for each pair (example for one)
curl -s 'https://api.binance.com/api/v3/ticker/24hr?symbol=ICPUSDT'
```

## Data Sources

All data was fetched from Binance public API v3 on 2026-04-04. Commands to recreate:

```bash
API="https://api.binance.com/api/v3"
DATE=$(date +%Y_%m_%d)
SYMBOLS="ICPBTC ICPUSDT"

for SYM in $SYMBOLS; do
  # 24h ticker statistics
  curl -s "$API/ticker/24hr?symbol=$SYM"                    -o "${DATE}_binance_ticker_24hr_${SYM}.json"
  # Hourly candlestick data (last 1000 hours ~ 41 days)
  curl -s "$API/klines?symbol=$SYM&interval=1h&limit=1000"  -o "${DATE}_binance_klines_1h_${SYM}.json"
  # Order book depth (up to 5000 levels)
  curl -s "$API/depth?symbol=$SYM&limit=5000"               -o "${DATE}_binance_depth_${SYM}.json"
  # Aggregated trades (last 1000)
  curl -s "$API/aggTrades?symbol=$SYM&limit=1000"           -o "${DATE}_binance_agg_trades_${SYM}.json"
  # Individual historical trades (last 1000)
  curl -s "$API/historicalTrades?symbol=$SYM&limit=1000"    -o "${DATE}_binance_historical_trades_${SYM}.json"
done
```

## Analysis Results

### 1. Volume Comparison (24h snapshot)

| Pair     | Trades/24h | Volume (ICP)   | Quote Volume       | Trades/hour |
|----------|-----------|----------------|---------------------|-------------|
| ICP/BTC  | 132       | 6,993          | 0.24 BTC            | 5.5         |
| ICP/USDT | 15,726    | 1,189,216      | $2,727,915          | 655.2       |

ICP/USDT dominates with ~80% of all trades and ~80% of dollar volume. ICP/BTC is negligible by comparison.

### 2. Trade Rate and Burstiness

From the last 1000 historical trades of each pair:

| Pair     | Period covered | Avg trades/hour | Avg gap between trades | Min gap (burst) | Max gap (quiet) |
|----------|---------------|-----------------|------------------------|-----------------|-----------------|
| ICP/BTC  | 5.0 days      | 8.4             | 7 min 8s               | < 1ms           | 1h 53min        |
| ICP/USDT | 1.5 hours     | 656.8           | 5.5s                   | < 1ms           | 2.8 min         |

All pairs exhibit bursty behavior: the minimum gap is sub-millisecond (multiple trades from a single market order hitting several resting orders), while quiet periods can stretch to nearly 2 hours on ICP/BTC.

### 3. Aggregation Ratio (aggTrades vs individual trades)

| Pair     | Individual trades per aggTrade |
|----------|-------------------------------|
| ICP/BTC  | 1.25                          |
| ICP/USDT | 2.09                          |

A ratio above 1 means a single incoming order frequently matches against multiple resting orders at different price levels. ICP/USDT's ratio of 2.09 means the average market order eats through ~2 price levels. This is relevant for matching engine design: each incoming order triggers on average 1-2 fills, not just one.

### 4. Order Book Depth (resting orders)

| Pair     | Bid levels | Ask levels | Total levels | Bid depth (ICP) | Ask depth (ICP) | Spread    |
|----------|-----------|-----------|-------------|-----------------|-----------------|-----------|
| ICP/BTC  | 92        | 1,180     | 1,272       | 61,857          | 78,557          | 0.291%    |
| ICP/USDT | 697       | 5,000+    | 5,697       | 924,901         | 1,289,920       | 0.043%    |

ICP/USDT has the deepest book with 5,697+ price levels of resting orders. Both pairs are heavily ask-skewed, meaning more liquidity is offered on the sell side. ICP/USDT has a tight spread (0.043%), while BTC has a wider spread (0.291%), reflecting lower liquidity.

### 5. Peak Load Analysis (from 1000 hours of kline data)

| Pair     | Avg trades/hour | Median | p95   | p99    | Peak hour         | Peak trades/hour | Peak trades/sec |
|----------|----------------|--------|-------|--------|-------------------|-----------------|-----------------|
| ICP/BTC  | 38.5           | 17     | 134   | 297    | 2026-03-11 06:00  | 3,955           | 1.10            |
| ICP/USDT | 2,648          | 1,779  | 6,229 | 13,483 | 2026-03-11 06:00  | 143,932         | 39.98           |

Both pairs saw their peak at the exact same hour (2026-03-11 06:00 UTC), suggesting a market-wide event. The peak-to-average ratio is 103x (BTC) and 54x (USDT), showing extreme spikiness.

## Interpretation for ICP DEX Design

**Steady-state load is very manageable.** During normal conditions, ICP/USDT -- the busiest pair -- sees about 2,600 trades/hour (~0.7/sec). Even aggregating all three pairs, the matching engine would process fewer than 1 trade per second on average. This is well within the capacity of a single canister on ICP.

**Peak load is the real design constraint.** The busiest hour recorded saw ~144,000 trades on ICP/USDT alone -- that is 40 trades/sec sustained over an hour, with likely much higher sub-minute bursts. At p99 the rate is ~3.7 trades/sec. Designing for the p99 case (~15,000 trades/hour, ~4 trades/sec) is sensible; handling the extreme peak (40/sec) would require either batching or accepting some queuing delay.

**Order book size is moderate.** The deepest book (ICP/USDT) has ~5,700 price levels. An ICP canister can easily hold this in-memory. Even 10x this depth (57,000 levels) would be manageable.

**Fan-out is cheap in our design.** Each incoming order generates ~2 fills on average (ICP/USDT). Since our DEX settles fills via internal balance updates (no ledger calls), the fan-out only adds in-canister bookkeeping cost, which is negligible. Ledger transfers are only needed at deposit/withdrawal time, not per fill.

**ICP/USDT should be the primary benchmark.** It represents ~80% of volume and depth. ICP/BTC is essentially a rounding error in comparison and could be deprioritized in capacity planning.