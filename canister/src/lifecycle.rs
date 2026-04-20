use crate::balance::TokenBalance;
use crate::order::OrderHistory;
use crate::state::audit;
use crate::state::event::EventType;
use crate::state::{State, StateSnapshot};
use crate::{MATCHING_INTERVAL, Runtime, state, storage};
use dex_types_internal::DexArg;
use dex_types_internal::log::Priority;

pub fn init(arg: DexArg, runtime: &impl Runtime) {
    let init_arg = match arg {
        DexArg::Init(init_arg) => init_arg,
        DexArg::Upgrade(_) => {
            panic!("ERROR: expected Init argument");
        }
    };
    let order_history = OrderHistory::new(storage::order_history_memory());
    let balances = TokenBalance::new(storage::balances_memory());
    state::init_state(
        State::new(init_arg.clone(), order_history, balances).expect("ERROR: invalid init args"),
    );
    storage::record_event(runtime.time(), EventType::Init(init_arg));
    setup_timers();
    canlog::log!(Priority::Info, "[init]: DEX canister initialized");
}

pub fn pre_upgrade(runtime: &impl Runtime) {
    let start = runtime.instruction_counter();
    let snapshot = state::with_state(StateSnapshot::from_state);
    storage::state_snapshot::save(&snapshot);
    let instructions_used = runtime.instruction_counter() - start;
    canlog::log!(
        Priority::Info,
        "[pre_upgrade]: state snapshot written, total instructions used: {instructions_used}"
    );
}

pub fn post_upgrade(arg: Option<DexArg>, runtime: &impl Runtime) {
    let start = runtime.instruction_counter();

    let order_history = OrderHistory::new(storage::order_history_memory());
    let balances = TokenBalance::new(storage::balances_memory());
    let snapshot = storage::state_snapshot::load().expect(
        "missing state snapshot at post_upgrade — pre_upgrade trapped or was skipped; \
         manual recovery required",
    );
    state::init_state(snapshot.into_state(order_history, balances));

    match arg {
        Some(DexArg::Init(_)) => {
            panic!("ERROR: expected Upgrade argument");
        }
        Some(DexArg::Upgrade(Some(upgrade_arg))) => {
            state::with_state_mut(|s| {
                audit::process_event(s, EventType::Upgrade(upgrade_arg), runtime)
            });
        }
        Some(DexArg::Upgrade(None)) | None => {}
    }

    let instructions_used = runtime.instruction_counter() - start;
    canlog::log!(
        Priority::Info,
        "[post_upgrade]: state restored from snapshot, total instructions used: {instructions_used}",
    );
    setup_timers();
}

fn setup_timers() {
    ic_cdk_timers::set_timer_interval(MATCHING_INTERVAL, || async {
        crate::process_pending_orders();
    });
}
