use super::{MATCHING_INTERVAL, Scheduler, TaskType, run_task_if_ready, schedule_now};
use crate::Timestamp;
use crate::order::FeeRates;
use crate::state;
use crate::state::execution_policy::MAX_INSTRUCTION_BUDGET;
use crate::test_fixtures::mocks::MockRuntime;
use crate::test_fixtures::{
    LOT_SIZE, PRICE_SCALE, fund_user, init_state_with_order_book,
    init_state_with_order_book_and_fees,
};
use candid::Principal;
use mockall::Sequence;
use oisy_trade_types::{LimitOrderRequest, Side};

fn now() -> Timestamp {
    Timestamp::new(1_000_000_000)
}

#[test]
fn should_keep_earliest_deadline_when_scheduled_twice_earlier_first() {
    let mut s = Scheduler::default();
    let early = Timestamp::new(100);
    let late = Timestamp::new(200);

    s.schedule_at(TaskType::ProcessPendingOrders, early);
    s.schedule_at(TaskType::ProcessPendingOrders, late);

    assert_eq!(s.next_deadline(), Some(early));
    assert_eq!(s.deadlines.len(), 1);
}

#[test]
fn should_keep_earliest_deadline_when_scheduled_twice_later_first() {
    let mut s = Scheduler::default();
    let early = Timestamp::new(100);
    let late = Timestamp::new(200);

    s.schedule_at(TaskType::ProcessPendingOrders, late);
    s.schedule_at(TaskType::ProcessPendingOrders, early);

    assert_eq!(s.next_deadline(), Some(early));
    assert_eq!(s.deadlines.len(), 1);
}

#[test]
fn should_pop_task_when_now_equals_deadline() {
    let mut s = Scheduler::default();
    let deadline = Timestamp::new(500);
    s.schedule_at(TaskType::ProcessPendingOrders, deadline);

    let popped = s.pop_if_ready(deadline);

    assert_eq!(popped, Some(TaskType::ProcessPendingOrders));
}

#[test]
fn should_not_pop_task_when_now_less_than_deadline() {
    let mut s = Scheduler::default();
    let deadline = Timestamp::new(500);
    s.schedule_at(TaskType::ProcessPendingOrders, deadline);

    let popped = s.pop_if_ready(Timestamp::new(499));

    assert_eq!(popped, None);
}

#[test]
fn should_remove_popped_entry_from_queue() {
    let mut s = Scheduler::default();
    s.schedule_at(TaskType::ProcessPendingOrders, Timestamp::new(100));

    s.pop_if_ready(Timestamp::new(100));

    assert_eq!(s.next_deadline(), None);
    assert!(s.deadlines.is_empty());
}

#[test]
fn should_return_none_when_queue_empty() {
    let mut s = Scheduler::default();

    assert_eq!(s.pop_if_ready(Timestamp::MAX), None);
    assert_eq!(s.next_deadline(), None);
}

#[test]
fn should_saturate_timestamp_max_on_addition() {
    let near_max = Timestamp::new(u64::MAX - 1);
    let result = near_max.saturating_add(std::time::Duration::from_secs(u64::MAX));
    assert_eq!(result, Timestamp::MAX);
}

#[test]
fn should_arm_global_timer_with_same_deadline_for_burst_of_schedule_now() {
    let mut runtime = MockRuntime::new();
    runtime.expect_time().return_const(now());

    let expected_deadline = now();
    runtime
        .expect_global_timer_set()
        .times(3)
        .withf(move |&ts| ts == expected_deadline)
        .return_const(());

    schedule_now(TaskType::ProcessPendingOrders, &runtime);
    schedule_now(TaskType::ProcessPendingOrders, &runtime);
    schedule_now(TaskType::ProcessPendingOrders, &runtime);
}

#[test]
fn should_leave_one_queue_entry_after_burst_of_schedule_now() {
    let mut runtime = MockRuntime::new();
    runtime.expect_time().return_const(now());
    runtime.expect_global_timer_set().return_const(());

    schedule_now(TaskType::ProcessPendingOrders, &runtime);
    schedule_now(TaskType::ProcessPendingOrders, &runtime);
    schedule_now(TaskType::ProcessPendingOrders, &runtime);

    let count = super::SCHEDULER.with(|s| s.borrow().deadlines.len());
    assert_eq!(count, 1);
}

fn place_order(user: Principal, side: Side) {
    crate::add_limit_order(
        LimitOrderRequest {
            pair: crate::test_fixtures::icp_ckbtc_trading_pair().into(),
            side,
            price: candid::Nat::from(100u128 * PRICE_SCALE),
            quantity: candid::Nat::from(u64::from(LOT_SIZE)),
            time_in_force: None,
        },
        &crate::test_fixtures::mocks::mock_runtime_for(user),
    )
    .unwrap();
}

fn timer_runtime() -> MockRuntime {
    let mut runtime = MockRuntime::new();
    runtime.expect_time().return_const(now());
    runtime.expect_instruction_counter().return_const(0u64);
    runtime
}

#[test]
fn should_set_heartbeat_then_zero_delay_timer_when_more_work_pending() {
    init_state_with_order_book_and_fees(FeeRates::default());

    let buyer = Principal::from_slice(&[0x01]);
    let seller = Principal::from_slice(&[0x02]);
    fund_user(buyer);
    fund_user(seller);

    state::with_state_mut(|s| {
        s.set_execution_policy(
            state::ExecutionPolicy::try_new(1, MAX_INSTRUCTION_BUDGET, 1).expect("valid policy"),
        );
    });

    place_order(buyer, Side::Buy);
    place_order(seller, Side::Sell);
    place_order(buyer, Side::Buy);
    place_order(seller, Side::Sell);

    let heartbeat_ts = now().saturating_add(MATCHING_INTERVAL);
    let zero_ts = now();

    let mut seq = Sequence::new();
    let mut runtime = timer_runtime();
    runtime
        .expect_global_timer_set()
        .times(1)
        .in_sequence(&mut seq)
        .withf(move |&ts| ts == heartbeat_ts)
        .return_const(());
    runtime
        .expect_global_timer_set()
        .times(1)
        .in_sequence(&mut seq)
        .withf(move |&ts| ts == zero_ts)
        .return_const(());

    super::SCHEDULER.with(|s| {
        s.borrow_mut()
            .schedule_at(TaskType::ProcessPendingOrders, now())
    });

    run_task_if_ready(&runtime);
}

#[test]
fn should_set_heartbeat_only_when_complete() {
    init_state_with_order_book();

    let heartbeat_ts = now().saturating_add(MATCHING_INTERVAL);

    let mut seq = Sequence::new();
    let mut runtime = timer_runtime();
    runtime
        .expect_global_timer_set()
        .times(1)
        .in_sequence(&mut seq)
        .withf(move |&ts| ts == heartbeat_ts)
        .return_const(());

    super::SCHEDULER.with(|s| {
        s.borrow_mut()
            .schedule_at(TaskType::ProcessPendingOrders, now())
    });

    run_task_if_ready(&runtime);
}
