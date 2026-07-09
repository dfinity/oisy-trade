use super::{ExecutionPolicy, MAX_INSTRUCTION_BUDGET};

#[test]
fn should_construct_with_valid_args() {
    let p = ExecutionPolicy::try_new(1, 1, 1).unwrap();
    assert_eq!(p.max_orders_per_chunk(), 1);
    assert_eq!(p.instruction_budget(), 1);
    assert_eq!(p.max_fills_per_settling_event(), 1);

    let p = ExecutionPolicy::try_new(5_000, MAX_INSTRUCTION_BUDGET, 256).unwrap();
    assert_eq!(p.max_orders_per_chunk(), 5_000);
    assert_eq!(p.instruction_budget(), MAX_INSTRUCTION_BUDGET);
    assert_eq!(p.max_fills_per_settling_event(), 256);
}

#[test]
fn should_default_to_production_policy() {
    let p = ExecutionPolicy::default();
    assert_eq!(
        p.max_orders_per_chunk(),
        oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
    );
    assert_eq!(
        p.instruction_budget(),
        oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
    );
    assert_eq!(
        p.max_fills_per_settling_event(),
        oisy_trade_types_internal::DEFAULT_MAX_FILLS_PER_SETTLING_EVENT,
    );
}

#[test]
fn should_reject_zero_max_orders_per_chunk() {
    assert_eq!(
        ExecutionPolicy::try_new(0, 1_000, 128),
        Err("max_orders_per_chunk must be non-zero".to_string()),
    );
}

#[test]
fn should_reject_zero_instruction_budget() {
    assert_eq!(
        ExecutionPolicy::try_new(1, 0, 128),
        Err("instruction_budget must be non-zero".to_string()),
    );
}

#[test]
fn should_reject_zero_max_fills_per_settling_event() {
    assert_eq!(
        ExecutionPolicy::try_new(1, 1_000, 0),
        Err("max_fills_per_settling_event must be non-zero".to_string()),
    );
}

#[test]
fn should_reject_instruction_budget_above_ic_cap() {
    let err = ExecutionPolicy::try_new(1, MAX_INSTRUCTION_BUDGET + 1, 128).unwrap_err();
    assert!(
        err.contains("exceeds IC per-message cap"),
        "unexpected error: {err}",
    );
}
