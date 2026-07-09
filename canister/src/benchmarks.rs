use crate::order::{
    BasisPoint, FeeRates, LotSize, OrderBookId, OrderStatus, PendingOrder, Price, Quantity, Side,
    TickSize, TimeInForce, TokenId, TokenMetadata, TradingPair,
};

use crate::EXECUTOR;
use crate::order::{OrderHistory, OrderId, TradeHistory};
use crate::state::execution_policy::ExecutionPolicy;
use crate::state::{StableMemoryOptions, State};
use crate::storage;
use canbench_rs::bench;
use candid::Principal;
use oisy_trade_types_internal::{InitArg, Mode};
use serde::Deserialize;
use std::num::{NonZeroU64, NonZeroU128};

/// Minimum price increment for ICP/USDT on Binance: 0.001 USDT with 8 decimal places.
const TICK_SIZE: TickSize = TickSize::new(NonZeroU128::new(100_000).unwrap());
/// Minimum order quantity for ICP/USDT on Binance: 0.01 ICP with 8 decimal places.
const LOT_SIZE: LotSize = LotSize::new(NonZeroU64::new(1_000_000).unwrap());

/// Benchmark a single large sell order that fully fills all 697 bid levels from
/// the Binance depth snapshot, producing one fill per price level. Sized to the
/// exact total bid depth so it consumes every bid and rests nothing — an
/// apples-to-apples GTC counterpart to the FOK full-fill bench, differing only
/// in time-in-force.
#[bench(raw)]
fn bench_gtc_fill_full_bid_side() -> canbench_rs::BenchResult {
    run_full_bid_sweep_bench(
        |total_bid_qty| PendingOrder {
            side: Side::Sell,
            price: Price::new(TICK_SIZE.get()), // 0.001 USDT — crosses all bids
            quantity: total_bid_qty,
            time_in_force: TimeInForce::GoodTilCanceled,
        },
        OrderStatus::Filled,
    )
}

/// Benchmark a fill-or-kill order that fully fills all 697 bid levels from the
/// Binance depth snapshot in a single message. Reuses the GTC bench's exact
/// setup, sized to the total bid depth so it fully fills — exercising the plan
/// pass plus an `apply_plan` replay and one settlement step per level, all
/// atomically. The only difference from the GTC bench is the time-in-force, so
/// the two are directly comparable and isolate the FOK gate cost. Asserts the
/// FOK reaches a terminal state: the bid side is emptied and the ask side gained
/// no resting remainder.
#[bench(raw)]
fn bench_fok_fill_full_bid_side() -> canbench_rs::BenchResult {
    run_full_bid_sweep_bench(
        |total_bid_qty| PendingOrder {
            side: Side::Sell,
            price: Price::new(TICK_SIZE.get()), // 0.001 USDT — crosses all bids
            quantity: total_bid_qty,
            time_in_force: TimeInForce::FillOrKill,
        },
        OrderStatus::Filled,
    )
}

/// Benchmark a fill-or-kill order that is *killed*: identical setup to
/// [`bench_fok_fill_full_bid_side`], but the FOK is sized one lot past
/// the total bid depth, so the plan pass walks every bid level yet cannot fully
/// fill — the plan is discarded before any mutation. This measures the
/// plan-then-discard cost: the read-only walk of all 697 bid levels without the
/// `apply_plan` replay or settlement. Asserts the book is byte-identical (the
/// kill mutated nothing).
#[bench(raw)]
fn bench_fok_killed_full_bid_side() -> canbench_rs::BenchResult {
    run_full_bid_sweep_bench(
        |total_bid_qty| PendingOrder {
            side: Side::Sell,
            price: Price::new(TICK_SIZE.get()), // 0.001 USDT — crosses all bids
            quantity: total_bid_qty
                .checked_add(Quantity::from(LOT_SIZE.get()))
                .unwrap(),
            time_in_force: TimeInForce::FillOrKill,
        },
        OrderStatus::Expired,
    )
}

/// Benchmark processing 1000 incoming orders against a fully populated order book
/// using real Binance ICP/USDT data (697 bid levels + 5000 ask levels).
/// Each order is placed by a different user (worst case for balance lookups).
#[bench(raw)]
fn bench_process_pending_orders_1000() -> canbench_rs::BenchResult {
    bench_process_pending_orders_1000_with(FeeRates::default())
}

/// Same workload as [`bench_process_pending_orders_1000`] with non-zero
/// maker/taker rates (10 bps / 20 bps) — exercises the fee accrual path
/// during matching/settling.
#[bench(raw)]
fn bench_process_pending_orders_1000_with_fees() -> canbench_rs::BenchResult {
    bench_process_pending_orders_1000_with(FeeRates {
        maker: BasisPoint::new(10).unwrap(),
        taker: BasisPoint::new(20).unwrap(),
    })
}

fn bench_process_pending_orders_1000_with(fee_rates: FeeRates) -> canbench_rs::BenchResult {
    let depth = load_depth();
    let trades = load_trades();
    let mut state = new_state_with_fees(fee_rates);

    populate_state(&mut state, &depth);

    // Queue 1000 pending orders from aggregated trades.
    // Binance `m` field: true = buyer is maker, so the taker is a seller.
    let pair = trading_pair();
    let taker_id_offset = depth.bids.len() + depth.asks.len();
    for (i, trade) in trades.iter().enumerate() {
        let principal = user((taker_id_offset + i) as u64);
        fund_user(&mut state, principal);
        place_order(
            &mut state,
            principal,
            PendingOrder {
                side: if trade.m { Side::Sell } else { Side::Buy },
                price: Price::new(parse_decimal_8(&trade.p)),
                quantity: Quantity::from_u128(parse_decimal_8(&trade.q)),
                time_in_force: TimeInForce::GoodTilCanceled,
            },
        );
    }

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), trades.len());

    state.set_execution_policy(ExecutionPolicy::MAX);
    let res = canbench_rs::bench_fn(|| {
        EXECUTOR.run_once(&mut state, &crate::IC_RUNTIME);
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

    state.set_execution_policy(ExecutionPolicy::MAX);
    let res = canbench_rs::bench_fn(|| {
        EXECUTOR.run_once(&mut state, &crate::IC_RUNTIME);
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
    state.set_execution_policy(ExecutionPolicy::MAX);
    EXECUTOR.run_once(&mut state, &crate::IC_RUNTIME);
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
/// [`oisy_trade_types::PriceLevel::quantity`] aggregates across every resting order at that
/// price — so cost scales with the number of orders at the best bid and best
/// ask, not with total depth. In this fixture each level holds a single order,
/// so the benchmark measures the minimal constant-overhead path.
#[bench(raw)]
fn bench_get_order_book_ticker() -> canbench_rs::BenchResult {
    install_populated_state();
    let pair = oisy_trade_types::TradingPair::from(trading_pair());
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
    let request = oisy_trade_types::GetOrderBookDepthRequest {
        trading_pair: oisy_trade_types::TradingPair::from(trading_pair()),
        limit: None,
    };
    canbench_rs::bench_fn(|| {
        let _depth = crate::get_order_book_depth(request.clone());
    })
}

/// Benchmark `get_order_book_depth` at the hard cap (1000 levels per side)
/// against a fully populated Binance ICP/USDT snapshot. Upper bound on the
/// instructions a depth query consumes for this fixture; per-level cost
/// scales with resting orders at each price, so denser books can exceed it.
#[bench(raw)]
fn bench_get_order_book_depth_max() -> canbench_rs::BenchResult {
    install_populated_state();
    let request = oisy_trade_types::GetOrderBookDepthRequest {
        trading_pair: oisy_trade_types::TradingPair::from(trading_pair()),
        limit: Some(crate::MAX_DEPTH_LIMIT),
    };
    canbench_rs::bench_fn(|| {
        let _depth = crate::get_order_book_depth(request.clone());
    })
}

/// Benchmark paginating through *all* of a user's orders via `get_my_orders`,
/// for the user holding the most orders. Reuses the
/// `bench_process_pending_orders_1000` setup (fully populated Binance book) but
/// places all 1000 trade orders under a single user, then walks every page
/// (capped at `MAX_ORDERS_PER_RESPONSE`) until the history is exhausted.
#[bench(raw)]
fn bench_get_my_orders() -> canbench_rs::BenchResult {
    let depth = load_depth();
    let trades = load_trades();
    let mut state = new_state();
    populate_state(&mut state, &depth);

    let trader = user((depth.bids.len() + depth.asks.len()) as u64);
    fund_user(&mut state, trader);
    for trade in &trades {
        place_order(
            &mut state,
            trader,
            PendingOrder {
                side: if trade.m { Side::Sell } else { Side::Buy },
                price: Price::new(parse_decimal_8(&trade.p)),
                quantity: Quantity::from_u128(parse_decimal_8(&trade.q)),
                time_in_force: TimeInForce::GoodTilCanceled,
            },
        );
    }
    assert_eq!(
        state
            .get_user_orders(&trader, None, trades.len() * 2)
            .unwrap()
            .len(),
        trades.len()
    );

    crate::state::reset_state();
    crate::state::init_state(state);

    let total = trades.len();
    let page = oisy_trade_types::MAX_ORDERS_PER_RESPONSE;
    canbench_rs::bench_fn(|| {
        let mut after: Option<oisy_trade_types::OrderId> = None;
        let mut retrieved = 0usize;
        loop {
            let orders = crate::get_my_orders(
                Some(oisy_trade_types::GetMyOrdersArgs::by_page(
                    after.clone(),
                    page,
                )),
                trader,
            )
            .expect("benchmark cursor is always a valid order id");
            retrieved += orders.len();
            // Stop once the known total is reached; checking the count rather
            // than waiting for a short page avoids one extra empty call when
            // the total is an exact multiple of the page size.
            if retrieved >= total {
                break;
            }
            after = orders.last().map(|o| o.id.clone());
        }
        assert_eq!(retrieved, total);
    })
}

/// Benchmark paginating through *all* of one account's trades via
/// `get_my_trades { ByAccount }`, for a trader that is a party to 1000 fills.
/// Seeds the fills by having a single trader sweep 1000 resting 1-lot
/// counterparty sells as the taker (one fill per resting order), then walks
/// every page (capped at `MAX_TRADES_PER_RESPONSE`) until the account's trade
/// history is exhausted.
#[bench(raw)]
fn bench_get_my_trades() -> canbench_rs::BenchResult {
    const FILLS: u64 = 1_000;
    let price = Price::new(200_000_000); // 2.000 USDT
    let mut state = new_state();

    for i in 0..FILLS {
        let counterparty = user(i);
        fund_user(&mut state, counterparty);
        place_order(
            &mut state,
            counterparty,
            PendingOrder {
                side: Side::Sell,
                price,
                quantity: Quantity::from(LOT_SIZE.get()),
                time_in_force: TimeInForce::GoodTilCanceled,
            },
        );
    }

    let trader = user(FILLS);
    fund_user(&mut state, trader);
    let buy_order = place_order(
        &mut state,
        trader,
        PendingOrder {
            side: Side::Buy,
            price,
            quantity: Quantity::from(FILLS * LOT_SIZE.get()),
            time_in_force: TimeInForce::GoodTilCanceled,
        },
    );

    state.set_execution_policy(ExecutionPolicy::MAX);
    EXECUTOR.run_once(&mut state, &crate::IC_RUNTIME);

    let total = FILLS as usize;
    let (_, _, buy_order_record) = state.get_user_order(&trader, buy_order).unwrap();
    assert_eq!(buy_order_record.status, OrderStatus::Filled);

    crate::state::reset_state();
    crate::state::init_state(state);

    let page = oisy_trade_types::MAX_TRADES_PER_RESPONSE;
    canbench_rs::bench_fn(|| {
        let mut after: Option<oisy_trade_types::TradeId> = None;
        let mut retrieved = 0usize;
        loop {
            let mut trades = crate::get_my_trades(
                oisy_trade_types::GetMyTradesArgs {
                    filter: oisy_trade_types::TradesFilter::ByAccount(
                        oisy_trade_types::TradesByAccount {
                            after: after.take(),
                            length: page,
                        },
                    ),
                },
                trader,
            )
            .expect("benchmark issues only well-formed, owned cursors");
            retrieved += trades.len();
            if trades.is_empty() {
                break;
            }
            // Stop once the known total is reached; checking the count rather
            // than waiting for a short page avoids one extra empty call when
            // the total is an exact multiple of the page size.
            if retrieved >= total {
                break;
            }
            after = trades.pop().map(|t| t.id);
        }
        assert_eq!(retrieved, total);
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

/// Parse a Binance decimal string (e.g. "2.30400000") into a u128 assuming 8 decimal places.
/// Uses only integer arithmetic to avoid floating-point imprecision.
fn parse_decimal_8(s: &str) -> u128 {
    let (integer_part, fractional_part) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let integer: u128 = integer_part.parse().expect("invalid integer part");
    // Pad or truncate fractional part to exactly 8 digits.
    let mut frac_digits = [b'0'; 8];
    for (i, byte) in fractional_part.bytes().take(8).enumerate() {
        frac_digits[i] = byte;
    }
    let fractional: u128 = std::str::from_utf8(&frac_digits)
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
    new_state_with_fees(FeeRates::default())
}

fn new_state_with_fees(fee_rates: FeeRates) -> State<storage::VMem, storage::VMem> {
    let mut state = State::new(
        InitArg {
            mode: Mode::GeneralAvailability,
            max_orders_per_chunk: oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
            instruction_budget: oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
        },
        OrderHistory::new(
            storage::order_history_memory(),
            storage::user_orders_memory(),
        ),
        TradeHistory::new(storage::trades_memory(), storage::trades_by_user_memory()),
        crate::user::UserRegistry::new(
            storage::user_registry_memory(),
            storage::trading_accounts_memory(),
            storage::trading_accounts_by_funding_memory(),
        ),
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
        Quantity::from_u128(1),
        None,
        fee_rates,
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
                quantity: Quantity::from_u128(parse_decimal_8(qty_str)),
                time_in_force: TimeInForce::GoodTilCanceled,
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
                quantity: Quantity::from_u128(parse_decimal_8(qty_str)),
                time_in_force: TimeInForce::GoodTilCanceled,
            },
        );
    }
    assert_eq!(
        state.get_order_book(&pair).unwrap().pending_orders_len(),
        depth.bids.len() + depth.asks.len()
    );

    state.set_execution_policy(ExecutionPolicy::MAX);
    EXECUTOR.run_once(state, &crate::IC_RUNTIME);

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
                time_in_force: TimeInForce::GoodTilCanceled,
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
                time_in_force: TimeInForce::GoodTilCanceled,
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
) -> OrderId {
    let pair = trading_pair();
    let (order_id, order) = state.validate_limit_order(user, pair, pending).unwrap();
    state.record_limit_order(
        user,
        order_id.book_id(),
        order,
        crate::Timestamp::EPOCH,
        StableMemoryOptions::Write,
    );
    order_id
}

/// Run a full bid-sweep bench: build the shared `setup_bid_sweep`, run one
/// `EXECUTOR.run_once` under `bench_fn`, and assert the appropriate terminal
/// book state. The three `#[bench]` entry points stay distinct (canbench
/// reports one result per function) and differ only by these two parameters:
///   - `time_in_force` selects GTC vs FOK;
///   - `killed` sizes the taker one lot past the total bid depth so the FOK is
///     killed (the plan is discarded and the book stays byte-identical),
///     versus sized to the exact depth for a full fill (the bids empty and the
///     ask side gains no resting remainder).
fn run_full_bid_sweep_bench<F>(
    pending_order: F,
    expected_status: OrderStatus,
) -> canbench_rs::BenchResult
where
    F: FnOnce(Quantity) -> PendingOrder,
{
    let depth = load_depth();
    let mut state = new_state();
    populate_state(&mut state, &depth);

    let total_bid_qty = Quantity::from_u128(
        depth
            .bids
            .iter()
            .fold(0u128, |total_qty, (_price_str, qty_str)| {
                total_qty + parse_decimal_8(qty_str)
            }),
    );

    let pair = trading_pair();
    let taker = user((depth.bids.len() + depth.asks.len()) as u64);
    fund_user(&mut state, taker);
    let sell_order = place_order(&mut state, taker, pending_order(total_bid_qty));

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), 1);
    assert_eq!(book.bids_len(), depth.bids.len());
    let snapshot_before = crate::order::OrderBookSnapshot::from(book);

    state.set_execution_policy(ExecutionPolicy::MAX);
    let res = canbench_rs::bench_fn(|| {
        EXECUTOR.run_once(&mut state, &crate::IC_RUNTIME);
    });

    let book = state.get_order_book(&pair).unwrap();
    assert_eq!(book.pending_orders_len(), 0);
    let snapshot_after = crate::order::OrderBookSnapshot::from(book);

    let (_, _, sell_order_record) = state.get_user_order(&taker, sell_order).unwrap();
    assert_eq!(sell_order_record.status, expected_status);

    match expected_status {
        OrderStatus::Expired => {
            assert_eq!(
                snapshot_after.bids, snapshot_before.bids,
                "killed FOK must not touch the bid side"
            );
            assert_eq!(
                snapshot_after.asks, snapshot_before.asks,
                "killed FOK must not touch the ask side"
            );
        }
        OrderStatus::Filled => {
            assert!(
                snapshot_after.bids.is_empty(),
                "the taker fully filled the bids, so the bid side must be empty"
            );
            assert_eq!(
                snapshot_after.asks, snapshot_before.asks,
                "the taker fully filled the bids, so it must not have rested any remainder on the ask side"
            );
        }
        _ => {
            panic!("Unexpected status {expected_status:?}")
        }
    }

    res
}

/// Bound settling-event application cost during matching.
///
/// One taker crossing many resting makers used to produce a single oversized
/// settling event applied in one message, whose cost scaled with the number of
/// fills until it trapped the message. The round now partitions its fills into
/// bounded settling events (`crate::settlement::MAX_FILLS_PER_SETTLING_EVENT`
/// fills each), and the executor checks the instruction budget between events,
/// so the same sweep drains as many small events. This bench keeps the worst
/// case under the mainnet ICP/ckUSDT listing parameters: a book of 22_900
/// resting min-notional sell orders (each from a distinct principal, one fill
/// each) swept by a single fill-or-kill buy that empties the book. Under an
/// unlimited per-message budget
/// (`crate::state::execution_policy::MAX_INSTRUCTION_BUDGET`, 40B instructions)
/// all the resulting bounded events drain in one `run_once`.
///
/// 22_900 is the largest sweep that measured under the 40B cap before the fix
/// (~38.46B). Cost near the cap was step-wise, not smooth — the ~1.68M
/// instructions/maker average alone would extrapolate the crossing to ~23_800,
/// but the stable memory grew another chunk just above 22_900, pushing the
/// ~23_000 sweep to ~40.30B (both canbench measurements). The fix removes the
/// trap regardless of the exact crossing point.
mod settling_event_sweep {
    use crate::order::{
        FeeRates, LotSize, OrderBookId, OrderStatus, PendingOrder, Price, Quantity, Side, TickSize,
        TimeInForce, TokenMetadata,
    };
    use crate::order::{OrderHistory, TradeHistory};
    use crate::state::State;
    use crate::state::execution_policy::ExecutionPolicy;
    use crate::storage;
    use crate::{EXECUTOR, IC_RUNTIME, Runtime, Timestamp};
    use async_trait::async_trait;
    use canbench_rs::bench;
    use candid::Principal;
    use candid::utils::ArgumentEncoder;
    use ic_cdk::call::{CallFailed, Response};
    use oisy_trade_types_internal::{InitArg, Mode};
    use std::num::{NonZeroU64, NonZeroU128};

    /// A [`Runtime`] that reports the instruction counter *relative to its own
    /// construction*. It delegates every other call to [`IC_RUNTIME`] but
    /// subtracts the counter value captured at `new()` from every reading, so
    /// the executor's per-message budget check sees a fresh (near-zero) counter
    /// at the start of the benched sweep — exactly as on the IC, where the
    /// sweep runs in its own message with its own budget.
    ///
    /// Without this, canbench's single shared instruction counter (setup and
    /// `bench_fn` run in one message) makes `run_once` bail before the sweep
    /// once the unmeasured setup work alone crosses the 40B budget. canbench
    /// measures the real instructions independently of this reading, so the
    /// reported sweep cost is unaffected — this only keeps the executor from
    /// charging the sweep for the benchmark's setup.
    struct SweepRuntime {
        baseline: u64,
    }

    impl SweepRuntime {
        fn new() -> Self {
            Self {
                baseline: IC_RUNTIME.instruction_counter(),
            }
        }
    }

    #[async_trait]
    impl Runtime for SweepRuntime {
        async fn call_unbounded_wait<A>(
            &self,
            canister_id: Principal,
            method: &str,
            args: A,
        ) -> Result<Response, CallFailed>
        where
            A: ArgumentEncoder + Send,
        {
            IC_RUNTIME
                .call_unbounded_wait(canister_id, method, args)
                .await
        }

        fn msg_caller(&self) -> Principal {
            IC_RUNTIME.msg_caller()
        }

        fn canister_self(&self) -> Principal {
            IC_RUNTIME.canister_self()
        }

        fn is_controller(&self, principal: &Principal) -> bool {
            IC_RUNTIME.is_controller(principal)
        }

        fn instruction_counter(&self) -> u64 {
            IC_RUNTIME
                .instruction_counter()
                .saturating_sub(self.baseline)
        }

        fn time(&self) -> Timestamp {
            IC_RUNTIME.time()
        }
    }

    // Mainnet ICP/ckUSDT listing parameters (ICP: 8 decimals, ckUSDT: 6).
    /// 0.001 ckUSDT price increment: `0.001 × 10^6`.
    const TICK_SIZE: TickSize = TickSize::new(NonZeroU128::new(1_000).unwrap());
    /// 0.01 ICP quantity increment: `0.01 × 10^8`.
    const LOT_SIZE: LotSize = LotSize::new(NonZeroU64::new(1_000_000).unwrap());
    /// 5 ckUSDT minimum order value: `5 × 10^6`.
    const MIN_NOTIONAL: u128 = 5_000_000;
    /// 5.000 ckUSDT per ICP (a multiple of the tick).
    const MAKER_PRICE: u128 = 5_000_000;
    /// 1 ICP (= 100 lots). At `MAKER_PRICE` this is exactly the min notional:
    /// `5_000_000 × 100_000_000 / 10^8 = 5_000_000` = 5 ckUSDT.
    const MAKER_QUANTITY: u128 = 100_000_000;

    /// The largest maker count whose sweep still fits under the 40B cap.
    const NUM_MAKERS: u64 = 22_900;

    #[bench(raw)]
    fn bench_fok_sweep_22_900_makers() -> canbench_rs::BenchResult {
        let num_makers = NUM_MAKERS;
        let mut state = new_state();
        let pair = super::trading_pair();

        // Resting sell side: `num_makers` orders, each exactly at the min
        // notional, each from a distinct principal so every fill touches a
        // fresh maker balance (worst case for balance lookups). All rest at the
        // same price, so they never cross one another.
        for i in 0..num_makers {
            let principal = super::user(i);
            super::fund_user(&mut state, principal);
            super::place_order(
                &mut state,
                principal,
                PendingOrder {
                    side: Side::Sell,
                    price: Price::new(MAKER_PRICE),
                    quantity: Quantity::from_u128(MAKER_QUANTITY),
                    time_in_force: TimeInForce::GoodTilCanceled,
                },
            );
        }
        state.set_execution_policy(ExecutionPolicy::MAX);
        EXECUTOR.run_once(&mut state, &SweepRuntime::new());
        let book = state.get_order_book(&pair).unwrap();
        assert_eq!(book.pending_orders_len(), 0);
        assert_eq!(book.resting_orders_len(), num_makers as usize);

        // One fill-or-kill buy sized to the whole book: it crosses every maker
        // at `MAKER_PRICE` and fully fills, emptying the book across the bounded
        // settling events its fills partition into (each at most
        // `MAX_FILLS_PER_SETTLING_EVENT` fills), whose combined application cost
        // scales with `num_makers`.
        let taker = super::user(num_makers);
        super::fund_user(&mut state, taker);
        let buy = super::place_order(
            &mut state,
            taker,
            PendingOrder {
                side: Side::Buy,
                price: Price::new(MAKER_PRICE),
                quantity: Quantity::from_u128(num_makers as u128 * MAKER_QUANTITY),
                time_in_force: TimeInForce::FillOrKill,
            },
        );

        let res = canbench_rs::bench_fn(|| {
            EXECUTOR.run_once(&mut state, &SweepRuntime::new());
        });

        let book = state.get_order_book(&pair).unwrap();
        assert_eq!(book.pending_orders_len(), 0);
        assert_eq!(
            book.resting_orders_len(),
            0,
            "the FOK must sweep every resting maker"
        );
        let (_, _, buy_record) = state.get_user_order(&taker, buy).unwrap();
        assert_eq!(buy_record.status, OrderStatus::Filled);

        res
    }

    /// Mainnet ICP/ckUSDT book — differs from the parent module's Binance
    /// fixture (6-decimal quote, wider tick/lot, 5 ckUSDT min notional), so it
    /// is kept local; the pair, principal, funding and order-placement helpers
    /// are shared from the parent module.
    fn new_state() -> State<storage::VMem, storage::VMem> {
        let mut state = State::new(
            InitArg {
                mode: Mode::GeneralAvailability,
                max_orders_per_chunk: oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
                instruction_budget: oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
            },
            OrderHistory::new(
                storage::order_history_memory(),
                storage::user_orders_memory(),
            ),
            TradeHistory::new(storage::trades_memory(), storage::trades_by_user_memory()),
            crate::user::UserRegistry::new(
                storage::user_registry_memory(),
                storage::trading_accounts_memory(),
                storage::trading_accounts_by_funding_memory(),
            ),
            crate::balance::TokenBalance::new(storage::balances_memory()),
        )
        .unwrap();
        state.record_trading_pair(
            OrderBookId::ZERO,
            super::trading_pair(),
            TokenMetadata {
                symbol: "ICP".to_string(),
                decimals: 8,
            },
            TokenMetadata {
                symbol: "ckUSDT".to_string(),
                decimals: 6,
            },
            TICK_SIZE,
            LOT_SIZE,
            Quantity::from_u128(MIN_NOTIONAL),
            None,
            FeeRates::default(),
        );
        state
    }
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
