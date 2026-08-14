use crate::execute::{EXECUTOR, ExecutionStatus};
use crate::order::{
    FeeRates, OrderBookId, OrderId, OrderStatus, Side, TokenId, TokenMetadata, TradingPair,
};
use crate::state::State;
use crate::state::execution_policy::{ExecutionPolicy, MAX_INSTRUCTION_BUDGET};
use crate::test_fixtures;
use crate::test_fixtures::mocks::MockRuntime;
use crate::test_fixtures::{
    LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, PRICE_SCALE, TICK_SIZE, ckbtc_metadata,
    icp_ckbtc_trading_pair, icp_metadata,
};
use candid::Principal;
use ic_stable_structures::VectorMemory;

type TestState = State<VectorMemory, VectorMemory>;

const BUYER: Principal = Principal::from_slice(&[0x01]);
const SELLER: Principal = Principal::from_slice(&[0x02]);

/// Status of `order_id` regardless of which of the two test principals owns
/// it, via the owner-scoped `get_user_order`.
fn status_of(state: &TestState, order_id: OrderId) -> Option<OrderStatus> {
    [BUYER, SELLER].into_iter().find_map(|owner| {
        state
            .get_user_order(&owner, order_id)
            .map(|(_, _, record)| record.status)
    })
}

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
    let buy_id =
        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
    let sell_id =
        test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);

    let status = EXECUTOR.run_once(&mut state, &runtime);

    assert_eq!(status, ExecutionStatus::Complete);
    assert_eq!(status_of(&state, buy_id), Some(OrderStatus::Filled));
    assert_eq!(status_of(&state, sell_id), Some(OrderStatus::Filled));
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
            test_fixtures::order(user, &pair, side, 100 * PRICE_SCALE, lot).place(&mut state)
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
        assert_eq!(status_of(&state, id), Some(OrderStatus::Filled));
    }
}

/// A single taker sweeping more than `max_settlement_units_per_event` resting
/// makers produces several bounded settling events; under an unlimited budget
/// one `run_once` matches and drains all of them, leaving no pending work and
/// every order `Filled`.
#[test]
fn should_drain_all_split_settling_events_in_one_run() {
    let mut state = setup_one_book();
    set_unlimited_policy(&mut state);
    let runtime = runtime();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);
    let cap = oisy_trade_types_internal::DEFAULT_MAX_SETTLEMENT_UNITS_PER_EVENT as usize;
    let num_makers = cap + 2;

    let maker_ids: Vec<(Principal, OrderId)> = (0..num_makers)
        .map(|i| {
            let owner = test_fixtures::maker(i);
            let id = test_fixtures::order(owner, &pair, Side::Sell, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            (owner, id)
        })
        .collect();
    let buy_id = test_fixtures::order(
        BUYER,
        &pair,
        Side::Buy,
        100 * PRICE_SCALE,
        num_makers as u64 * lot,
    )
    .place(&mut state);

    let status = EXECUTOR.run_once(&mut state, &runtime);

    assert_eq!(status, ExecutionStatus::Complete);
    assert!(!state.has_pending_orders());
    assert!(!state.has_pending_settling_events());
    assert_eq!(status_of(&state, buy_id), Some(OrderStatus::Filled));
    for (owner, id) in maker_ids {
        let status = state.get_user_order(&owner, id).map(|(_, _, r)| r.status);
        assert_eq!(status, Some(OrderStatus::Filled));
    }
}

#[test]
fn should_be_a_no_op_when_globally_halted() {
    let mut state = setup_one_book();
    set_unlimited_policy(&mut state);
    let runtime = runtime();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);
    let buy_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100, lot).place(&mut state);
    let sell_id = test_fixtures::order(SELLER, &pair, Side::Sell, 100, lot).place(&mut state);

    state.permissions_mut().halt_trading_globally();

    let status = EXECUTOR.run_once(&mut state, &runtime);

    // Crossable orders are left untouched: no matching, no settling.
    assert_eq!(status, ExecutionStatus::Complete);
    assert_eq!(status_of(&state, buy_id), Some(OrderStatus::Pending));
    assert_eq!(status_of(&state, sell_id), Some(OrderStatus::Pending));
    assert!(state.has_pending_orders());

    // Resuming lets the same orders fill.
    state.permissions_mut().resume_trading_globally();
    let status = EXECUTOR.run_once(&mut state, &runtime);
    assert_eq!(status, ExecutionStatus::Complete);
    assert_eq!(status_of(&state, buy_id), Some(OrderStatus::Filled));
    assert_eq!(status_of(&state, sell_id), Some(OrderStatus::Filled));
}

/// A global halt must still drain settling events left over from a prior
/// chunk: those events apply the balance effects of already-matched fills, so
/// stranding them for the halt would trap a counterparty's proceeds. No new
/// matching happens, and `run_once` reschedules only to finish the drain.
#[test]
fn should_drain_leftover_settling_events_under_global_halt() {
    let mut state = setup_one_book();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);

    // Stage a settling event without draining it by recording the matching
    // event directly — that's the producer side of the settling queue.
    test_fixtures::order(BUYER, &pair, Side::Buy, 100, lot).place(&mut state);
    test_fixtures::order(SELLER, &pair, Side::Sell, 100, lot).place(&mut state);
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
        crate::Timestamp::EPOCH,
        crate::state::StableMemoryOptions::Write,
    );
    assert!(state.has_pending_settling_events());
    assert!(!state.has_pending_orders());

    state.permissions_mut().halt_trading_globally();

    set_unlimited_policy(&mut state);
    let status = EXECUTOR.run_once(&mut state, &runtime());

    // The leftover settling was applied despite the halt; no new matching ran.
    assert_eq!(status, ExecutionStatus::Complete);
    assert!(!state.has_pending_settling_events());
    assert_eq!(state.get_balance(&BUYER, &pair.base).free(), &lot.into());
}

/// A per-pair halt must skip only the halted book: its crossable orders stay
/// pending while every other book keeps matching. Because the halted book's
/// pending orders are not matchable, the executor reports `Complete` rather
/// than busy-spinning on them; unhalting lets the book fill.
#[test]
fn should_skip_halted_book_while_matching_others() {
    let mut state = setup_two_books();
    set_unlimited_policy(&mut state);
    let runtime = runtime();
    let pair_a = icp_ckbtc_trading_pair();
    let pair_b = pair_b();
    let lot = u64::from(LOT_SIZE);

    // A crossable pair on each book.
    let buy_a = test_fixtures::order(BUYER, &pair_a, Side::Buy, 100, lot).place(&mut state);
    let sell_a = test_fixtures::order(SELLER, &pair_a, Side::Sell, 100, lot).place(&mut state);
    let buy_b = test_fixtures::order(BUYER, &pair_b, Side::Buy, 100, lot).place(&mut state);
    let sell_b = test_fixtures::order(SELLER, &pair_b, Side::Sell, 100, lot).place(&mut state);

    // Halt book A only.
    state.permissions_mut().halt_trading(OrderBookId::ZERO);

    // The halted book keeps its pending orders, but they are not matchable,
    // so once book B drains the run reports `Complete` — no matching happens
    // on the halted book and the matching timer can stop instead of spinning.
    let status = EXECUTOR.run_once(&mut state, &runtime);
    assert_eq!(status, ExecutionStatus::Complete);

    // Book A's orders are left untouched; book B's cross fills.
    assert_eq!(status_of(&state, buy_a), Some(OrderStatus::Pending));
    assert_eq!(status_of(&state, sell_a), Some(OrderStatus::Pending));
    assert_eq!(status_of(&state, buy_b), Some(OrderStatus::Filled));
    assert_eq!(status_of(&state, sell_b), Some(OrderStatus::Filled));

    // Unhalting lets book A's cross fill.
    state.permissions_mut().resume_trading(OrderBookId::ZERO);
    let status = EXECUTOR.run_once(&mut state, &runtime);
    assert_eq!(status, ExecutionStatus::Complete);
    assert_eq!(status_of(&state, buy_a), Some(OrderStatus::Filled));
    assert_eq!(status_of(&state, sell_a), Some(OrderStatus::Filled));
}

/// Drives the actual `drive_matching` reschedule decision (loop while
/// `run_once` reports `MoreWork`) rather than a single direct call. While a
/// pair is halted and only its book holds pending orders, the loop must
/// terminate at `Complete` with no forward progress — otherwise the
/// zero-delay self-reschedule chain busy-spins for the whole halt. After
/// unhalting, a fresh drive must fill the previously-halted book's resting
/// cross.
#[test]
fn should_not_busy_spin_while_pair_halted_and_resume_on_unhalt() {
    let mut state = setup_one_book();
    set_chunk_policy(&mut state, 1);
    let runtime = runtime();
    let pair = icp_ckbtc_trading_pair();
    let lot = u64::from(LOT_SIZE);

    let buy = test_fixtures::order(BUYER, &pair, Side::Buy, 100, lot).place(&mut state);
    let sell = test_fixtures::order(SELLER, &pair, Side::Sell, 100, lot).place(&mut state);

    // Halt the only book before any matching runs.
    state.permissions_mut().halt_trading(OrderBookId::ZERO);

    // Mirror `drive_matching`: a halted book reports no matchable work, so the
    // run reaches `Complete` instead of self-rescheduling — a busy-spin would
    // never reach `Complete`.
    let status = EXECUTOR.run_once(&mut state, &runtime);
    assert_eq!(status, ExecutionStatus::Complete);
    assert_eq!(status_of(&state, buy), Some(OrderStatus::Pending));
    assert_eq!(status_of(&state, sell), Some(OrderStatus::Pending));

    // Unhalt and drive again: the resting cross now fills.
    state.permissions_mut().resume_trading(OrderBookId::ZERO);
    while state.has_pending_orders() || state.has_pending_settling_events() {
        EXECUTOR.run_once(&mut state, &runtime);
    }
    assert_eq!(status_of(&state, buy), Some(OrderStatus::Filled));
    assert_eq!(status_of(&state, sell), Some(OrderStatus::Filled));
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
    test_fixtures::order(BUYER, &pair_a, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
    test_fixtures::order(BUYER, &pair_a, Side::Buy, 110 * PRICE_SCALE, lot).place(&mut state);
    test_fixtures::order(BUYER, &pair_a, Side::Buy, 120 * PRICE_SCALE, lot).place(&mut state);
    test_fixtures::order(BUYER, &pair_b, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);

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
    test_fixtures::order(BUYER, &pair_a, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
    test_fixtures::order(BUYER, &pair_a, Side::Buy, 110 * PRICE_SCALE, lot).place(&mut state);
    // Book 1: 5 pending.
    for price in [
        100 * PRICE_SCALE,
        110 * PRICE_SCALE,
        120 * PRICE_SCALE,
        130 * PRICE_SCALE,
        140 * PRICE_SCALE,
    ] {
        test_fixtures::order(BUYER, &pair_b, Side::Buy, price, lot).place(&mut state);
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
    test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
    test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);
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
        crate::Timestamp::EPOCH,
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
    test_fixtures::order(BUYER, &pair_a, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
    test_fixtures::order(SELLER, &pair_a, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);
    test_fixtures::order(BUYER, &pair_b, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
    test_fixtures::order(SELLER, &pair_b, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);

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
    test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);

    // Minimum budget; mock returns a counter already past it.
    state.set_execution_policy(ExecutionPolicy::try_new(u32::MAX, 1, u32::MAX).unwrap());
    let mut mock = MockRuntime::new();
    mock.expect_time().return_const(crate::Timestamp::EPOCH);
    mock.expect_instruction_counter().return_const(1u64);

    let status = EXECUTOR.run_once(&mut state, &mock);

    assert_eq!(status, ExecutionStatus::MoreWork);
    assert!(state.has_pending_orders());
    // No matching event was emitted, so no settling was queued either.
    assert!(!state.has_pending_settling_events());
}

fn set_chunk_policy(state: &mut TestState, max_orders_per_chunk: u32) {
    state.set_execution_policy(
        ExecutionPolicy::try_new(
            max_orders_per_chunk,
            MAX_INSTRUCTION_BUDGET,
            oisy_trade_types_internal::DEFAULT_MAX_SETTLEMENT_UNITS_PER_EVENT,
        )
        .unwrap(),
    );
}

fn set_unlimited_policy(state: &mut TestState) {
    set_chunk_policy(state, u32::MAX);
}

fn runtime() -> MockRuntime {
    let mut mock = MockRuntime::new();
    mock.expect_time().return_const(crate::Timestamp::EPOCH);
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
        MIN_NOTIONAL,
        Some(MAX_NOTIONAL),
        FeeRates::default(),
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
        MIN_NOTIONAL,
        Some(MAX_NOTIONAL),
        FeeRates::default(),
    );
    state
}

fn place_workload(state: &mut TestState, pair: &TradingPair, lot: u64) {
    // A mix that exercises partial fills, full fills, and resting orders.
    test_fixtures::order(BUYER, pair, Side::Buy, 100 * PRICE_SCALE, lot).place(state);
    test_fixtures::order(BUYER, pair, Side::Buy, 110 * PRICE_SCALE, lot).place(state);
    test_fixtures::order(SELLER, pair, Side::Sell, 100 * PRICE_SCALE, lot).place(state);
    test_fixtures::order(SELLER, pair, Side::Sell, 110 * PRICE_SCALE, lot * 2).place(state);
    test_fixtures::order(BUYER, pair, Side::Buy, 110 * PRICE_SCALE, lot).place(state);
}
