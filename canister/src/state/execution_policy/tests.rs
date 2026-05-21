use super::{ExecutionPolicy, MAX_INSTRUCTION_BUDGET};

#[test]
fn should_construct_with_valid_args() {
    let p = ExecutionPolicy::new(1, 1);
    assert_eq!(p.max_orders_per_chunk(), 1);
    assert_eq!(p.instruction_budget(), 1);

    let p = ExecutionPolicy::new(5_000, MAX_INSTRUCTION_BUDGET);
    assert_eq!(p.max_orders_per_chunk(), 5_000);
    assert_eq!(p.instruction_budget(), MAX_INSTRUCTION_BUDGET);
}

#[test]
fn should_default_to_production_policy() {
    let p = ExecutionPolicy::default();
    assert_eq!(p.max_orders_per_chunk(), 1_000);
    assert_eq!(p.instruction_budget(), 1_000_000_000);
}

#[test]
#[should_panic(expected = "max_orders_per_chunk must be non-zero")]
fn should_panic_on_zero_max_orders_per_chunk() {
    let _ = ExecutionPolicy::new(0, 1_000);
}

#[test]
#[should_panic(expected = "instruction_budget must be non-zero")]
fn should_panic_on_zero_instruction_budget() {
    let _ = ExecutionPolicy::new(1, 0);
}

#[test]
#[should_panic(expected = "exceeds IC per-message cap")]
fn should_panic_when_instruction_budget_exceeds_ic_cap() {
    let _ = ExecutionPolicy::new(1, MAX_INSTRUCTION_BUDGET + 1);
}
