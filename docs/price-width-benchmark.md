# Benchmark: order-book `Price` as `u64` vs `u128`

> **Experiment branch — not for merge.** This captures a throwaway measurement of
> the matching-engine cost of widening the order-book `Price` key from `u64` to
> `u128`. The diff makes only the minimal type changes needed to compile and run
> the existing `canbench` matching benchmarks.

## Why

To represent realistic prices for asymmetric-decimal pairs, the order-book price
is being redefined as *quote base units per 1 whole base unit*. An empirical
check of all live Binance spot pairs showed 22 pairs (every one quoted in an
18-decimal token: FDUSD/USD1/USDS/ETH…) exceed `u64::MAX` under that encoding —
e.g. BTC/FDUSD ≈ 6×10²². So the key likely needs to be `u128`. This measures
what that costs the matching engine.

## What changed (throwaway diff)

- `Price` stores a `u128` instead of a `u64` (`canister/src/order/mod.rs`); the
  BTreeMap key, `Ord`, `is_multiple_of`, and `checked_mul` now operate on `u128`.
- `Price::new`/`get` keep their `u64` signatures (benchmark prices are small), so
  call sites are untouched — only the *stored width* and the hot-path arithmetic
  change, which is exactly what we want to measure.
- Added `Quantity::checked_mul_u128` (correct 128×256→256 checked multiply) and a
  `cbor::u128_codec` minicbor helper mirroring `Quantity`'s bignum encoding.

## Method

`cd canister && canbench`, comparing against the committed `canbench_results.yml`
(the `u64` baseline). Exact integers captured via `canbench --persist`.

## Results (IC instructions)

| Benchmark | u64 | u128 | Δ abs | Δ % |
| --- | ---: | ---: | ---: | ---: |
| `bench_process_pending_orders_1_large` | 859,551,528 | 857,696,378 | −1,855,150 | −0.22% |
| `bench_process_pending_orders_1000` | 1,077,636,906 | 1,079,573,931 | +1,937,025 | +0.18% |
| `bench_process_pending_orders_1000_with_fees` | 1,097,725,580 | 1,095,975,218 | −1,750,362 | −0.16% |
| `bench_process_pending_orders_1000_no_fills` | 81,239,226 | 82,003,098 | +763,872 | +0.94% |

All four are within canbench's noise threshold; **two are negative** (u128 cannot
be causally faster — that's run-to-run noise), so the real cost is below the
measurement floor.

## Interpretation

Widening `Price` to `u128` has **no measurable matching-engine cost** (≤0.22% on
the fill-heavy workloads, and net-negative on two of them). The largest relative
move is the `no_fills` bench (+0.94%) — the smallest workload (82M instr,
dominated by tick-size validation), where the `u128 % u64` modulo is
proportionally most visible because there is no settlement/fee work to dwarf it.

This matches the static analysis: the engine already operates on `u256`
`Quantity` values (comparisons, long division, multiplication) and performs
stable-memory `order_history` lookups and CBOR (de)serialization on the fill
path. A `u128` price comparison is strictly cheaper than the `u256` quantity
comparison already on every loop iteration, so widening the cheapest operand is
lost in the noise.
