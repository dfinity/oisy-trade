use crate::order::{
    LotSize, OrderBookId, PendingOrder, Price, Quantity, Side, TickSize, TokenId, TokenMetadata,
    TradingPair,
};

use crate::order::OrderHistory;
use crate::state::{StableMemoryOptions, State};
use crate::storage;
use canbench_rs::bench;
use candid::Principal;
use dex_types_internal::{InitArg, Mode};
use serde::Deserialize;
use std::num::NonZeroU64;

/// Minimum price increment for ICP/USDT on Binance: 0.001 USDT with 8 decimal places.
const TICK_SIZE: TickSize = TickSize::new(NonZeroU64::new(100_000).unwrap());
/// Minimum order quantity for ICP/USDT on Binance: 0.01 ICP with 8 decimal places.
const LOT_SIZE: LotSize = LotSize::new(NonZeroU64::new(1_000_000).unwrap());

/// Benchmark a single large sell order that sweeps all 697 bid levels from the
/// Binance depth snapshot, producing one fill per price level.
#[bench(raw)]
fn bench_process_pending_orders_1_large() -> canbench_rs::BenchResult {
    let depth = load_depth();
    let mut state = new_state();

    populate_state(&mut state, &depth);

    // Place a single sell at the minimum price with quantity exceeding total bid depth
    // (~924,901 ICP). This crosses every bid level.
    let pair = trading_pair();
    let taker = user((depth.bids.len() + depth.asks.len()) as u64);
    fund_user(&mut state, taker);
    place_order(
        &mut state,
        taker,
        PendingOrder {
            side: Side::Sell,
            price: Price::new(TICK_SIZE.get()), // 0.001 USDT — crosses all bids
            quantity: Quantity::from(100_000_000_000_000), // 1,000,000 ICP
        },
    );

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), 1);
    assert_eq!(book.bids_len(), depth.bids.len());

    let res = canbench_rs::bench_fn(|| {
        crate::test_fixtures::process_pending_orders(&mut state, &crate::IC_RUNTIME);
    });

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), 0);
    assert_eq!(book.bids_len(), 0);

    res
}

/// Benchmark processing 1000 incoming orders against a fully populated order book
/// using real Binance ICP/USDT data (697 bid levels + 5000 ask levels).
/// Each order is placed by a different user (worst case for balance lookups).
#[bench(raw)]
fn bench_process_pending_orders_1000() -> canbench_rs::BenchResult {
    let depth = crate::benchmarks::load_depth();
    let trades = crate::benchmarks::load_trades();
    let mut state = crate::benchmarks::new_state();

    crate::benchmarks::populate_state(&mut state, &depth);

    // Queue 1000 pending orders from aggregated trades.
    // Binance `m` field: true = buyer is maker, so the taker is a seller.
    let pair = crate::benchmarks::trading_pair();
    let taker_id_offset = depth.bids.len() + depth.asks.len();
    for (i, trade) in trades.iter().enumerate() {
        let principal = crate::benchmarks::user((taker_id_offset + i) as u64);
        crate::benchmarks::fund_user(&mut state, principal);
        place_order(
            &mut state,
            principal,
            PendingOrder {
                side: if trade.m { Side::Sell } else { Side::Buy },
                price: Price::new(crate::benchmarks::parse_decimal_8(&trade.p)),
                quantity: Quantity::from(crate::benchmarks::parse_decimal_8(&trade.q)),
            },
        );
    }

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), trades.len());

    let res = canbench_rs::bench_fn(|| {
        crate::test_fixtures::process_pending_orders(&mut state, &crate::IC_RUNTIME);
    });

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), 0);

    res
}

/// Benchmark processing 1000 orders that all rest without matching.
/// Wide spread between buys (2.000) and sells (3.000) ensures zero fills.
/// Each order is placed by a different user (worst case for balance lookups).
#[bench(raw)]
fn bench_process_pending_orders_1000_no_fills() -> canbench_rs::BenchResult {
    let mut state = new_state();
    let pair = trading_pair();
    place_1000_non_crossing_orders(&mut state);

    let book = state.get_order_book(&pair).unwrap();
    let num_resting_orders_before = book.resting_orders_len();
    assert_eq!(book.pending_orders_len(), 1_000);

    let res = canbench_rs::bench_fn(|| {
        crate::test_fixtures::process_pending_orders(&mut state, &crate::IC_RUNTIME);
    });

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), 0);
    assert_eq!(book.resting_orders_len(), num_resting_orders_before + 1_000);

    res
}

/// Benchmark pre_upgrade + post_upgrade against a fully populated order book
/// (697 bid + 5000 ask levels from the Binance snapshot).
#[bench(raw)]
fn bench_upgrade_full_depth() -> canbench_rs::BenchResult {
    let depth = load_depth();
    let mut state = new_state();
    populate_state(&mut state, &depth);
    bench_upgrade_roundtrip(state)
}

/// Benchmark pre_upgrade + post_upgrade against 1000 resting orders with no
/// fills.
#[bench(raw)]
fn bench_upgrade_1000_no_fills() -> canbench_rs::BenchResult {
    let mut state = new_state();
    place_1000_non_crossing_orders(&mut state);
    crate::test_fixtures::process_pending_orders(&mut state, &crate::IC_RUNTIME);
    bench_upgrade_roundtrip(state)
}

fn bench_upgrade_roundtrip(state: State<storage::VMem, storage::VMem>) -> canbench_rs::BenchResult {
    // canbench installs the canister via `init`, which already populated
    // the thread-local state. Swap in the benchmark's populated state.
    crate::state::reset_state();
    crate::state::init_state(state);
    canbench_rs::bench_fn(|| {
        crate::lifecycle::pre_upgrade(&crate::IC_RUNTIME);
        crate::state::reset_state();
        crate::lifecycle::post_upgrade(None, &crate::IC_RUNTIME);
    })
}

/// Benchmark the top-of-book query against a fully populated Binance ICP/USDT
/// snapshot. Only the first entry of each side is read, but the returned
/// [`dex_types::PriceLevel::quantity`] aggregates across every resting order at that
/// price — so cost scales with the number of orders at the best bid and best
/// ask, not with total depth. In this fixture each level holds a single order,
/// so the benchmark measures the minimal constant-overhead path.
#[bench(raw)]
fn bench_get_order_book_ticker() -> canbench_rs::BenchResult {
    install_populated_state();
    let pair = dex_types::TradingPair::from(trading_pair());
    canbench_rs::bench_fn(|| {
        let _ticker = crate::get_order_book_ticker(pair);
    })
}

/// Benchmark `get_order_book_depth` with the default limit (100 levels per side)
/// against a fully populated Binance ICP/USDT snapshot. Represents the common
/// case where a caller wants a reasonable L2 snapshot.
#[bench(raw)]
fn bench_get_order_book_depth_default() -> canbench_rs::BenchResult {
    install_populated_state();
    let request = dex_types::GetOrderBookDepthRequest {
        trading_pair: dex_types::TradingPair::from(trading_pair()),
        limit: None,
    };
    canbench_rs::bench_fn(|| {
        let _depth = crate::get_order_book_depth(request.clone());
    })
}

/// Benchmark `get_order_book_depth` at the hard cap (1000 levels per side)
/// against a fully populated Binance ICP/USDT snapshot. Upper bound on the
/// instructions a depth query consumes for this fixture; per-level cost
/// scales with resting orders at each price (see DEFI-2795), so denser
/// books can exceed it.
#[bench(raw)]
fn bench_get_order_book_depth_max() -> canbench_rs::BenchResult {
    install_populated_state();
    let request = dex_types::GetOrderBookDepthRequest {
        trading_pair: dex_types::TradingPair::from(trading_pair()),
        limit: Some(crate::MAX_DEPTH_LIMIT),
    };
    canbench_rs::bench_fn(|| {
        let _depth = crate::get_order_book_depth(request.clone());
    })
}

/// Build a freshly populated state from the Binance snapshot and install it
/// as the canister's thread-local state, so library dispatchers that read via
/// `state::with_state` observe it. canbench's `init` populated the
/// thread-local with an empty state, so we reset first.
fn install_populated_state() {
    let depth = load_depth();
    let mut state = new_state();
    populate_state(&mut state, &depth);
    crate::state::reset_state();
    crate::state::init_state(state);
}

#[derive(Deserialize)]
struct DepthSnapshot {
    /// Bid levels as `(price, quantity)` decimal strings, sorted by price descending.
    bids: Vec<(String, String)>,
    /// Ask levels as `(price, quantity)` decimal strings, sorted by price ascending.
    asks: Vec<(String, String)>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct AggTrade {
    /// Price as a decimal string (e.g. "2.30400000").
    p: String,
    /// Quantity as a decimal string (e.g. "56.45000000").
    q: String,
    /// `true` if the buyer is the maker (i.e. the taker is a seller).
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
    let snapshot: DepthSnapshot =
        serde_json::from_str(json).expect("failed to parse depth snapshot");
    assert_eq!(snapshot.bids.len(), 697);
    assert_eq!(snapshot.asks.len(), 5_000);
    snapshot
}

fn load_trades() -> Vec<AggTrade> {
    let json = include_str!("../../docs/trading_data/2026_04_04_binance_agg_trades_ICPUSDT.json");
    let trades: Vec<AggTrade> = serde_json::from_str(json).expect("failed to parse trades");
    assert_eq!(trades.len(), 1_000);
    trades
}

fn new_state() -> State<storage::VMem, storage::VMem> {
    let mut state = State::new(
        InitArg {
            mode: Mode::GeneralAvailability,
            max_orders_per_chunk: 1_000,
            instruction_budget: 1_000_000_000,
        },
        OrderHistory::new(storage::order_history_memory()),
        crate::balance::TokenBalance::new(storage::balances_memory()),
    )
    .unwrap();
    state.record_trading_pair(
        OrderBookId::ZERO,
        trading_pair(),
        TokenMetadata {
            symbol: "ICP".to_string(),
            decimals: 8,
        },
        TokenMetadata {
            symbol: "USDT".to_string(),
            decimals: 8,
        },
        TICK_SIZE,
        LOT_SIZE,
    );
    state
}

fn trading_pair() -> TradingPair {
    TradingPair {
        base: TokenId::new(Principal::from_slice(&[1])),
        quote: TokenId::new(Principal::from_slice(&[2])),
    }
}

/// Pre-populate an order book with resting orders from the Binance depth snapshot.
/// Each depth level is placed by a different user (IDs 0..bids+asks).
/// Best bid (2.304) < best ask (2.305), so no fills occur during population.
fn populate_state(state: &mut State<storage::VMem, storage::VMem>, depth: &DepthSnapshot) {
    let pair = trading_pair();
    for (i, (price_str, qty_str)) in depth.bids.iter().enumerate() {
        let principal = user(i as u64);
        fund_user(state, principal);
        place_order(
            state,
            principal,
            PendingOrder {
                side: Side::Buy,
                price: Price::new(parse_decimal_8(price_str)),
                quantity: Quantity::from(parse_decimal_8(qty_str)),
            },
        );
    }
    for (i, (price_str, qty_str)) in depth.asks.iter().enumerate() {
        let principal = user((depth.bids.len() + i) as u64);
        fund_user(state, principal);
        place_order(
            state,
            principal,
            PendingOrder {
                side: Side::Sell,
                price: Price::new(parse_decimal_8(price_str)),
                quantity: Quantity::from(parse_decimal_8(qty_str)),
            },
        );
    }
    assert_eq!(
        state.get_order_book(&pair).unwrap().pending_orders_len(),
        depth.bids.len() + depth.asks.len()
    );

    crate::test_fixtures::process_pending_orders(&mut state, &crate::IC_RUNTIME);

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), 0);
    assert_eq!(book.bids_len(), depth.bids.len());
    assert_eq!(book.asks_len(), depth.asks.len());
}

/// Places 1000 non-crossing limit orders (500 Buy at 2.000, 500 Sell at 3.000)
/// across 1000 distinct users. The wide spread guarantees zero fills when
/// matching runs, so every order ends up as a resting order.
fn place_1000_non_crossing_orders(state: &mut State<storage::VMem, storage::VMem>) {
    let half = 500u64;
    for i in 0..half {
        let principal = user(i);
        fund_user(state, principal);
        place_order(
            state,
            principal,
            PendingOrder {
                side: Side::Buy,
                price: Price::new(200_000_000), // 2.000 USDT
                quantity: Quantity::from((i + 1) * LOT_SIZE.get()),
            },
        );
    }
    for i in 0..half {
        let principal = user(half + i);
        fund_user(state, principal);
        place_order(
            state,
            principal,
            PendingOrder {
                side: Side::Sell,
                price: Price::new(300_000_000), // 3.000 USDT
                quantity: Quantity::from((i + 1) * LOT_SIZE.get()),
            },
        );
    }
}

/// Generate a unique principal from a sequential counter.
fn user(id: u64) -> Principal {
    // Principal::from_slice accepts up to 29 bytes; 8 bytes is plenty for unique IDs.
    Principal::from_slice(&id.to_be_bytes())
}

/// Fund a user with a large balance for both tokens of the trading pair.
fn fund_user(state: &mut State<storage::VMem, storage::VMem>, principal: Principal) {
    let pair = trading_pair();
    state.deposit(
        principal,
        pair.base,
        Quantity::from_u128(u128::MAX),
        StableMemoryOptions::Write,
    );
    state.deposit(
        principal,
        pair.quote,
        Quantity::from_u128(u128::MAX),
        StableMemoryOptions::Write,
    );
}

fn place_order(
    state: &mut State<storage::VMem, storage::VMem>,
    user: Principal,
    pending: PendingOrder,
) {
    let pair = trading_pair();
    let (order_id, order) = state.validate_limit_order(user, pair, pending).unwrap();
    state.record_limit_order(user, order_id.book_id(), order, StableMemoryOptions::Write);
}

mod event_storage {
    use crate::storage;
    use crate::test_fixtures::event::WorstCaseEvent;
    use canbench_rs::bench;
    use strum::IntoEnumIterator;

    #[bench(raw)]
    fn bench_write_events() -> canbench_rs::BenchResult {
        canbench_rs::bench_fn(|| {
            for variant in WorstCaseEvent::iter() {
                let name: &'static str = (&variant).into();
                {
                    let _scope = canbench_rs::bench_scope(name);
                    let event = variant.worst_case_instructions_event();
                    storage::record_event(event.timestamp, event.payload);
                }
            }
        })
    }

    #[bench(raw)]
    fn bench_read_events() -> canbench_rs::BenchResult {
        let mut indices = Vec::new();
        for variant in WorstCaseEvent::iter() {
            let event = variant.worst_case_instructions_event();
            storage::record_event(event.timestamp, event.payload);
            indices.push((storage::total_event_count() - 1, variant));
        }
        canbench_rs::bench_fn(|| {
            for (idx, variant) in &indices {
                let name: &'static str = variant.into();
                {
                    let _scope = canbench_rs::bench_scope(name);
                    storage::get_event(*idx);
                }
            }
        })
    }
}
