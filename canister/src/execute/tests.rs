use crate::execute::{EXECUTOR, ExecutionStatus};
use crate::order::{OrderBookId, OrderId, OrderStatus, Side, TokenId, TokenMetadata, TradingPair};
use crate::state::State;
use crate::state::execution_policy::{ExecutionPolicy, MAX_INSTRUCTION_BUDGET};
use crate::test_fixtures;
use crate::test_fixtures::mocks::MockRuntime;
use crate::test_fixtures::{
    LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata,
};
use candid::Principal;
use ic_stable_structures::VectorMemory;

type TestState = State<VectorMemory, VectorMemory>;

const BUYER: Principal = Principal::from_slice(&[0x01]);
const SELLER: Principal = Principal::from_slice(&[0x02]);

#[test]
fn should_return_complete_on_idle_state() {
    let mut state = setup_one_book();
    set_unlimited_policy(&mut state);
    let runtime = runtime();

    let status = EXECUTOR.run_once(&mut state, &runtime);

    assert_eq!(status, ExecutionStatus::Complete);
    assert!(!state.has_pending_orders());
    assert!(!state.has_pending_settling_events());
}

#[test]
fn should_complete_in_one_run_when_budget_covers_all() {
    let mut state = setup_one_book();
    set_unlimited_policy(&mut state);
    let runtime = runtime();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);
    let buy_id = test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
    let sell_id = test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, lot);

    let status = EXECUTOR.run_once(&mut state, &runtime);

    assert_eq!(status, ExecutionStatus::Complete);
    assert_eq!(state.get_order_status(buy_id), Some(OrderStatus::Filled));
    assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Filled));
}

#[test]
fn should_signal_more_work_until_all_orders_are_drained() {
    let mut state = setup_one_book();
    let runtime = runtime();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);
    let ids: Vec<OrderId> = (0..4)
        .map(|i| {
            let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
            let user = if i % 2 == 0 { BUYER } else { SELLER };
            test_fixtures::place_order(&mut state, user, &pair, side, 100, lot)
        })
        .collect();

    set_chunk_policy(&mut state, 1);

    let mut statuses = Vec::new();
    while state.has_pending_orders() || state.has_pending_settling_events() {
        statuses.push(EXECUTOR.run_once(&mut state, &runtime));
        // Bound the loop in case of a bug.
        assert!(statuses.len() <= 10, "executor failed to make progress");
    }

    assert!(matches!(
        statuses.as_slice(),
        [
            ExecutionStatus::MoreWork,
            ExecutionStatus::MoreWork,
            ExecutionStatus::MoreWork,
            ExecutionStatus::Complete
        ]
    ));
    for id in ids {
        assert_eq!(state.get_order_status(id), Some(OrderStatus::Filled));
    }
}

/// Driving the same workload through a single unlimited [`Executor`] run and
/// through many chunk-size-1 runs must end in the same canister state.
#[test]
fn should_produce_state_equivalent_to_single_shot_run() {
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);

    let single_shot = {
        let mut state = setup_one_book();
        set_unlimited_policy(&mut state);
        place_workload(&mut state, &pair, lot);
        EXECUTOR.run_once(&mut state, &runtime());
        state
    };

    let chunked = {
        let mut state = setup_one_book();
        set_chunk_policy(&mut state, 1);
        place_workload(&mut state, &pair, lot);
        let runtime = runtime();
        while state.has_pending_orders() || state.has_pending_settling_events() {
            EXECUTOR.run_once(&mut state, &runtime);
        }
        // Align with `single_shot`'s policy so state equality compares the
        // matching outcome (books, balances, history) and not the policy
        // each run executed under.
        set_unlimited_policy(&mut state);
        state
    };

    assert_eq!(single_shot, chunked);
}

#[test]
fn should_process_book_with_more_pending_first_under_tight_chunk_budget() {
    let mut state = setup_two_books();
    let pair_a = icp_ckbtc_trading_pair();
    let pair_b = pair_b();
    let lot = u64::from(LOT_SIZE);

    // Book A has 3 pending orders; book B has 1.
    test_fixtures::place_order(&mut state, BUYER, &pair_a, Side::Buy, 100, lot);
    test_fixtures::place_order(&mut state, BUYER, &pair_a, Side::Buy, 110, lot);
    test_fixtures::place_order(&mut state, BUYER, &pair_a, Side::Buy, 120, lot);
    test_fixtures::place_order(&mut state, BUYER, &pair_b, Side::Buy, 100, lot);

    set_chunk_policy(&mut state, 2);

    let status = EXECUTOR.run_once(&mut state, &runtime());

    assert_eq!(status, ExecutionStatus::MoreWork);
    // Book A (most pending) was processed first and now has 1 left; book B is untouched.
    assert_eq!(
        state
            .order_book(&OrderBookId::ZERO)
            .unwrap()
            .pending_orders_len(),
        1,
    );
    assert_eq!(
        state
            .order_book(&OrderBookId::ONE)
            .unwrap()
            .pending_orders_len(),
        1,
    );
}

/// Ranking by pending count must beat ascending book-ID order: with book 0
/// holding 2 pending and book 1 holding 5, a `max_orders_per_chunk = 5` run
/// must fully drain book 1 (the larger one) and leave book 0 untouched.
#[test]
fn should_rank_higher_id_book_with_more_pending_ahead_of_lower_id_book() {
    let mut state = setup_two_books();
    let pair_a = icp_ckbtc_trading_pair();
    let pair_b = pair_b();
    let lot = u64::from(LOT_SIZE);

    // Book 0: 2 pending.
    test_fixtures::place_order(&mut state, BUYER, &pair_a, Side::Buy, 100, lot);
    test_fixtures::place_order(&mut state, BUYER, &pair_a, Side::Buy, 110, lot);
    // Book 1: 5 pending.
    for price in [100, 110, 120, 130, 140] {
        test_fixtures::place_order(&mut state, BUYER, &pair_b, Side::Buy, price, lot);
    }

    set_chunk_policy(&mut state, 5);

    let status = EXECUTOR.run_once(&mut state, &runtime());

    assert_eq!(status, ExecutionStatus::MoreWork);
    assert_eq!(
        state
            .order_book(&OrderBookId::ZERO)
            .unwrap()
            .pending_orders_len(),
        2,
        "lower-id book with fewer pending must be skipped this chunk",
    );
    assert_eq!(
        state
            .order_book(&OrderBookId::ONE)
            .unwrap()
            .pending_orders_len(),
        0,
        "higher-id book with more pending must be fully drained first",
    );
}

/// `run_once` must drain settling events that another path (e.g. a prior
/// chunk whose inline drain was budget-interrupted, or a cancel) left on
/// the queue, even when there is no new matching work to do.
#[test]
fn should_drain_leftover_settling_events_before_running_matching() {
    let mut state = setup_one_book();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);

    // Stage a settling event without draining it by calling
    // record_matching_event directly — that's the producer side of the
    // pending_settling_events queue.
    test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
    test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, lot);
    let pending: Vec<_> = state
        .order_book(&OrderBookId::ZERO)
        .unwrap()
        .pending_order_seqs()
        .collect();
    state.record_matching_event(
        &crate::state::event::MatchingEvent {
            book_id: OrderBookId::ZERO,
            orders: pending,
        },
        crate::state::StableMemoryOptions::Write,
    );
    assert!(state.has_pending_settling_events());
    assert!(!state.has_pending_orders());

    set_unlimited_policy(&mut state);
    let status = EXECUTOR.run_once(&mut state, &runtime());

    assert_eq!(status, ExecutionStatus::Complete);
    assert!(!state.has_pending_settling_events());
}

/// Each book's settling must complete before the next book is matched, so
/// the settling queue is empty between books and the post-`run_once`
/// invariant `!has_pending_settling_events()` holds whenever the budget
/// covered every book.
#[test]
fn should_settle_each_book_before_advancing_to_the_next() {
    let mut state = setup_two_books();
    let pair_a = icp_ckbtc_trading_pair();
    let pair_b = pair_b();
    let lot = u64::from(LOT_SIZE);

    // Both books have a crossing pair that will produce a SettlingEvent.
    test_fixtures::place_order(&mut state, BUYER, &pair_a, Side::Buy, 100, lot);
    test_fixtures::place_order(&mut state, SELLER, &pair_a, Side::Sell, 100, lot);
    test_fixtures::place_order(&mut state, BUYER, &pair_b, Side::Buy, 100, lot);
    test_fixtures::place_order(&mut state, SELLER, &pair_b, Side::Sell, 100, lot);

    set_unlimited_policy(&mut state);
    let status = EXECUTOR.run_once(&mut state, &runtime());

    assert_eq!(status, ExecutionStatus::Complete);
    assert!(!state.has_pending_orders());
    assert!(!state.has_pending_settling_events());
    // Balances on both books reflect the fills — proves both settlements ran.
    assert_eq!(state.get_balance(&BUYER, &pair_a.base).free(), &lot.into(),);
    assert_eq!(state.get_balance(&BUYER, &pair_b.base).free(), &lot.into(),);
}

#[test]
fn should_exit_early_when_instruction_budget_already_exceeded() {
    let mut state = setup_one_book();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);
    test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);

    // Minimum budget; mock returns a counter already past it.
    state.set_execution_policy(ExecutionPolicy::try_new(u32::MAX, 1).unwrap());
    let mut mock = MockRuntime::new();
    mock.expect_time().return_const(0u64);
    mock.expect_instruction_counter().return_const(1u64);

    let status = EXECUTOR.run_once(&mut state, &mock);

    assert_eq!(status, ExecutionStatus::MoreWork);
    assert!(state.has_pending_orders());
    // No matching event was emitted, so no settling was queued either.
    assert!(!state.has_pending_settling_events());
}

fn set_chunk_policy(state: &mut TestState, max_orders_per_chunk: u32) {
    state.set_execution_policy(
        ExecutionPolicy::try_new(max_orders_per_chunk, MAX_INSTRUCTION_BUDGET).unwrap(),
    );
}

fn set_unlimited_policy(state: &mut TestState) {
    set_chunk_policy(state, u32::MAX);
}

fn runtime() -> MockRuntime {
    let mut mock = MockRuntime::new();
    mock.expect_time().return_const(0u64);
    mock.expect_instruction_counter().return_const(0u64);
    mock
}

fn setup_one_book() -> TestState {
    let mut state = test_fixtures::state();
    state.record_trading_pair(
        OrderBookId::ZERO,
        icp_ckbtc_trading_pair(),
        icp_metadata(),
        ckbtc_metadata(),
        TICK_SIZE,
        LOT_SIZE,
    );
    state
}

fn pair_b() -> TradingPair {
    TradingPair {
        base: TokenId::new(Principal::from_slice(&[0xB1])),
        quote: TokenId::new(Principal::from_slice(&[0xB2])),
    }
}

fn setup_two_books() -> TestState {
    let mut state = setup_one_book();
    state.record_trading_pair(
        OrderBookId::ONE,
        pair_b(),
        TokenMetadata {
            symbol: "B".to_string(),
            decimals: 8,
        },
        TokenMetadata {
            symbol: "Q".to_string(),
            decimals: 8,
        },
        TICK_SIZE,
        LOT_SIZE,
    );
    state
}

fn place_workload(state: &mut TestState, pair: &TradingPair, lot: u64) {
    // A mix that exercises partial fills, full fills, and resting orders.
    test_fixtures::place_order(state, BUYER, pair, Side::Buy, 100, lot);
    test_fixtures::place_order(state, BUYER, pair, Side::Buy, 110, lot);
    test_fixtures::place_order(state, SELLER, pair, Side::Sell, 100, lot);
    test_fixtures::place_order(state, SELLER, pair, Side::Sell, 110, lot * 2);
    test_fixtures::place_order(state, BUYER, pair, Side::Buy, 110, lot);
}
