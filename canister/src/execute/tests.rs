use crate::execute::{Executor, Outcome};
use crate::order::{OrderBookId, OrderId, OrderStatus, Side, TokenId, TokenMetadata, TradingPair};
use crate::state::State;
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

fn unlimited_executor() -> Executor {
    Executor {
        max_orders_per_chunk: usize::MAX,
        instruction_budget: u64::MAX,
    }
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

#[test]
fn should_return_complete_on_idle_state() {
    let mut state = setup_one_book();
    let runtime = runtime();

    let outcome = unlimited_executor().run_once(&mut state, &runtime);

    assert_eq!(outcome, Outcome::Complete);
    assert!(!state.has_pending_orders());
    assert!(!state.has_pending_settling_events());
}

#[test]
fn should_complete_in_one_run_when_budget_covers_all() {
    let mut state = setup_one_book();
    let runtime = runtime();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);
    let buy_id = test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
    let sell_id = test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, lot);

    let outcome = unlimited_executor().run_once(&mut state, &runtime);

    assert_eq!(outcome, Outcome::Complete);
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

    let executor = Executor {
        max_orders_per_chunk: 1,
        instruction_budget: u64::MAX,
    };

    let mut outcomes = Vec::new();
    while state.has_pending_orders() || state.has_pending_settling_events() {
        outcomes.push(executor.run_once(&mut state, &runtime));
        // Bound the loop in case of a bug.
        assert!(outcomes.len() <= 10, "executor failed to make progress");
    }

    assert!(matches!(
        outcomes.as_slice(),
        [
            Outcome::MoreWork,
            Outcome::MoreWork,
            Outcome::MoreWork,
            Outcome::Complete
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
        place_workload(&mut state, &pair, lot);
        unlimited_executor().run_once(&mut state, &runtime());
        state
    };

    let chunked = {
        let mut state = setup_one_book();
        place_workload(&mut state, &pair, lot);
        let executor = Executor {
            max_orders_per_chunk: 1,
            instruction_budget: u64::MAX,
        };
        let runtime = runtime();
        while state.has_pending_orders() || state.has_pending_settling_events() {
            executor.run_once(&mut state, &runtime);
        }
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

    let executor = Executor {
        max_orders_per_chunk: 2,
        instruction_budget: u64::MAX,
    };

    let outcome = executor.run_once(&mut state, &runtime());

    assert_eq!(outcome, Outcome::MoreWork);
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

    let executor = Executor {
        max_orders_per_chunk: 5,
        instruction_budget: u64::MAX,
    };

    let outcome = executor.run_once(&mut state, &runtime());

    assert_eq!(outcome, Outcome::MoreWork);
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

#[test]
fn should_exit_early_when_instruction_budget_already_exceeded() {
    let mut state = setup_one_book();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);
    test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);

    let executor = Executor {
        max_orders_per_chunk: usize::MAX,
        instruction_budget: 0,
    };

    let outcome = executor.run_once(&mut state, &runtime());

    assert_eq!(outcome, Outcome::MoreWork);
    assert!(state.has_pending_orders());
    // No matching event was emitted, so no settling was queued either.
    assert!(!state.has_pending_settling_events());
}

fn place_workload(state: &mut TestState, pair: &TradingPair, lot: u64) {
    // A mix that exercises partial fills, full fills, and resting orders.
    test_fixtures::place_order(state, BUYER, pair, Side::Buy, 100, lot);
    test_fixtures::place_order(state, BUYER, pair, Side::Buy, 110, lot);
    test_fixtures::place_order(state, SELLER, pair, Side::Sell, 100, lot);
    test_fixtures::place_order(state, SELLER, pair, Side::Sell, 110, lot * 2);
    test_fixtures::place_order(state, BUYER, pair, Side::Buy, 110, lot);
}
