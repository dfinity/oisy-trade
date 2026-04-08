use crate::order::{LotSize, PendingOrder, Price, Quantity, Side, TickSize, TokenId, TradingPair};
use crate::state::State;
use canbench_rs::bench;
use candid::{Nat, Principal};
use dex_types_internal::{InitArg, Mode};
use serde::Deserialize;
use std::num::NonZeroU64;

/// Minimum price increment for ICP/USDT on Binance: 0.001 USDT with 8 decimal places.
const TICK_SIZE: TickSize = TickSize::new(NonZeroU64::new(100_000).unwrap());
/// Minimum order quantity for ICP/USDT on Binance: 0.01 ICP with 8 decimal places.
const LOT_SIZE: LotSize = LotSize::new(NonZeroU64::new(1_000_000).unwrap());

const USER: Principal = Principal::anonymous();

fn trading_pair() -> TradingPair {
    TradingPair {
        base: TokenId::new(Principal::from_slice(&[1])),
        quote: TokenId::new(Principal::from_slice(&[2])),
    }
}

fn new_state() -> State {
    let mut state = State::try_from(InitArg {
        mode: Mode::GeneralAvailability,
    })
    .unwrap();
    let pair = trading_pair();
    state
        .add_trading_pair(pair.clone(), TICK_SIZE, LOT_SIZE)
        .unwrap();
    state.deposit(USER, pair.base, Nat::from(u128::MAX));
    state.deposit(USER, pair.quote, Nat::from(u128::MAX));
    state
}

/// Pre-populate an order book with resting orders from the Binance depth snapshot.
/// Best bid (2.304) < best ask (2.305), so no fills occur during population.
fn populate_state(state: &mut State, depth: &DepthSnapshot) {
    let pair = trading_pair();
    for (price_str, qty_str) in &depth.bids {
        state
            .add_limit_order(
                USER,
                pair.clone(),
                PendingOrder {
                    side: Side::Buy,
                    price: Price::new(parse_decimal_8(price_str)),
                    quantity: Quantity::new(parse_decimal_8(qty_str)),
                },
            )
            .expect("valid bid order");
    }
    for (price_str, qty_str) in &depth.asks {
        state
            .add_limit_order(
                USER,
                pair.clone(),
                PendingOrder {
                    side: Side::Sell,
                    price: Price::new(parse_decimal_8(price_str)),
                    quantity: Quantity::new(parse_decimal_8(qty_str)),
                },
            )
            .expect("valid ask order");
    }
    state.process_pending_orders();
}

/// Benchmark processing 1000 incoming orders against a fully populated order book
/// using real Binance ICP/USDT data (697 bid levels + 5000 ask levels).
///
/// Includes both matching and settlement. Use the `matching` and `settling`
/// bench scopes to see the breakdown.
#[bench(raw)]
fn bench_process_1000_orders() -> canbench_rs::BenchResult {
    let depth = load_depth();
    let trades = load_trades();
    let mut state = new_state();

    populate_state(&mut state, &depth);

    // Queue 1000 pending orders from aggregated trades.
    // Binance `m` field: true = buyer is maker, so the taker is a seller.
    let pair = trading_pair();
    for trade in &trades {
        state
            .add_limit_order(
                USER,
                pair.clone(),
                PendingOrder {
                    side: if trade.m { Side::Sell } else { Side::Buy },
                    price: Price::new(parse_decimal_8(&trade.p)),
                    quantity: Quantity::new(parse_decimal_8(&trade.q)),
                },
            )
            .expect("valid trade order");
    }

    canbench_rs::bench_fn(|| {
        state.process_pending_orders();
    })
}

/// Benchmark a single large sell order that sweeps all 697 bid levels from the
/// Binance depth snapshot, producing one fill per price level.
#[bench(raw)]
fn bench_process_single_order_sweeps_697_bid_levels() -> canbench_rs::BenchResult {
    let depth = load_depth();
    let mut state = new_state();

    populate_state(&mut state, &depth);

    // Place a single sell at the minimum price with quantity exceeding total bid depth
    // (~924,901 ICP). This crosses every bid level.
    let pair = trading_pair();
    state
        .add_limit_order(
            USER,
            pair,
            PendingOrder {
                side: Side::Sell,
                price: Price::new(TICK_SIZE.get()), // 0.001 USDT — crosses all bids
                quantity: Quantity::new(100_000_000_000_000), // 1,000,000 ICP
            },
        )
        .expect("valid sell order");

    canbench_rs::bench_fn(|| {
        state.process_pending_orders();
    })
}

/// Benchmark processing 1000 orders that all rest without matching.
/// Wide spread between buys (2.000) and sells (3.000) ensures zero fills.
#[bench(raw)]
fn bench_process_1000_orders_no_fills() -> canbench_rs::BenchResult {
    let mut state = new_state();
    let pair = trading_pair();

    for i in 0..500u64 {
        state
            .add_limit_order(
                USER,
                pair.clone(),
                PendingOrder {
                    side: Side::Buy,
                    price: Price::new(200_000_000), // 2.000 USDT
                    quantity: Quantity::new((i + 1) * LOT_SIZE.get()),
                },
            )
            .expect("valid buy order");
    }
    for i in 0..500u64 {
        state
            .add_limit_order(
                USER,
                pair.clone(),
                PendingOrder {
                    side: Side::Sell,
                    price: Price::new(300_000_000), // 3.000 USDT
                    quantity: Quantity::new((i + 1) * LOT_SIZE.get()),
                },
            )
            .expect("valid sell order");
    }

    canbench_rs::bench_fn(|| {
        state.process_pending_orders();
    })
}

#[derive(Deserialize)]
struct DepthSnapshot {
    bids: Vec<(String, String)>,
    asks: Vec<(String, String)>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct AggTrade {
    p: String,
    q: String,
    m: bool,
}

/// Parse a Binance decimal string (e.g. "2.30400000") into a u64 assuming 8 decimal places.
/// Uses only integer arithmetic to avoid floating-point imprecision.
fn parse_decimal_8(s: &str) -> u64 {
    let (integer_part, fractional_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let integer: u64 = integer_part.parse().expect("invalid integer part");
    // Pad or truncate fractional part to exactly 8 digits.
    let mut frac_digits = [b'0'; 8];
    for (i, byte) in fractional_part.bytes().take(8).enumerate() {
        frac_digits[i] = byte;
    }
    let fractional: u64 = std::str::from_utf8(&frac_digits)
        .expect("ascii digits")
        .parse()
        .expect("invalid fractional part");
    integer * 100_000_000 + fractional
}

fn load_depth() -> DepthSnapshot {
    let json = include_str!("../../docs/trading_data/2026_04_04_binance_depth_ICPUSDT.json");
    serde_json::from_str(json).expect("failed to parse depth snapshot")
}

fn load_trades() -> Vec<AggTrade> {
    let json = include_str!("../../docs/trading_data/2026_04_04_binance_agg_trades_ICPUSDT.json");
    serde_json::from_str(json).expect("failed to parse trades")
}
