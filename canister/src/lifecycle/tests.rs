use super::{init, post_upgrade};
use crate::state::StateSnapshot;
use crate::storage;
use crate::test_fixtures::mocks::MockRuntime;
use crate::test_fixtures::state;
use oisy_trade_types_internal::{InitArg, Mode, OisyTradeArg};

fn init_arg() -> InitArg {
    InitArg {
        mode: Mode::GeneralAvailability,
        max_orders_per_chunk: oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
        instruction_budget: oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
    }
}

#[test]
#[should_panic(expected = "expected Init argument")]
fn init_rejects_upgrade_argument() {
    // `init` never touches the runtime before rejecting the wrong arg.
    let runtime = MockRuntime::new();
    init(OisyTradeArg::Upgrade(None), &runtime);
}

#[test]
#[should_panic(expected = "missing state snapshot")]
fn post_upgrade_traps_when_snapshot_missing() {
    let mut runtime = MockRuntime::new();
    runtime.expect_instruction_counter().return_const(0u64);
    post_upgrade(None, &runtime);
}

#[test]
#[should_panic(expected = "expected Upgrade argument")]
fn post_upgrade_rejects_init_argument() {
    // A snapshot must be present for `post_upgrade` to get past the
    // restore step and reach argument validation.
    let snapshot = StateSnapshot::from_state(&state());
    storage::state_snapshot::save(&snapshot);

    let mut runtime = MockRuntime::new();
    runtime.expect_instruction_counter().return_const(0u64);
    post_upgrade(Some(OisyTradeArg::Init(init_arg())), &runtime);
}
