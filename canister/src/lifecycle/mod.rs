use crate::balance::TokenBalance;
use crate::order::{FillStore, OrderHistory};
use crate::state::audit;
use crate::state::event::EventType;
use crate::state::{State, StateSnapshot};
use crate::user::UserRegistry;
use crate::{MATCHING_INTERVAL, Runtime, state, storage};
use oisy_trade_types_internal::OisyTradeArg;
use oisy_trade_types_internal::log::Priority;

#[cfg(test)]
mod tests;

pub fn init(arg: OisyTradeArg, runtime: &impl Runtime) {
    let init_arg = match arg {
        OisyTradeArg::Init(init_arg) => init_arg,
        OisyTradeArg::Upgrade(_) => {
            panic!("ERROR: expected Init argument");
        }
    };
    let order_history = OrderHistory::new(
        storage::order_history_memory(),
        storage::user_orders_memory(),
    );
    let fill_store = FillStore::new(
        storage::fills_memory(),
        storage::fills_by_user_memory(),
        storage::fills_seq_memory(),
    );
    let balances = TokenBalance::new(storage::balances_memory());
    let user_registry = UserRegistry::new(storage::user_registry_memory());
    state::init_state(
        State::new(
            init_arg.clone(),
            order_history,
            fill_store,
            user_registry,
            balances,
        )
        .expect("ERROR: invalid init args"),
    );
    storage::record_event(runtime.time(), EventType::Init(init_arg));
    setup_timers();
    canlog::log!(Priority::Info, "[init]: OISY TRADE canister initialized");
}

pub fn pre_upgrade(runtime: &impl Runtime) {
    #[cfg(feature = "canbench-rs")]
    let _scope = canbench_rs::bench_scope("pre_upgrade");
    let start = runtime.instruction_counter();
    let snapshot = {
        #[cfg(feature = "canbench-rs")]
        let _scope = canbench_rs::bench_scope("pre_upgrade::from_state");
        state::with_state(StateSnapshot::from_state)
    };
    let snapshot_bytes = {
        #[cfg(feature = "canbench-rs")]
        let _scope = canbench_rs::bench_scope("pre_upgrade::save_snapshot");
        storage::state_snapshot::save(&snapshot)
    };
    let instructions_used = runtime.instruction_counter() - start;
    canlog::log!(
        Priority::Info,
        "[pre_upgrade]: state snapshot written ({snapshot_bytes} bytes), total instructions used: {instructions_used}"
    );
}

pub fn post_upgrade(arg: Option<OisyTradeArg>, runtime: &impl Runtime) {
    #[cfg(feature = "canbench-rs")]
    let _scope = canbench_rs::bench_scope("post_upgrade");
    let start = runtime.instruction_counter();

    let (order_history, fill_store, balances, user_registry) = {
        #[cfg(feature = "canbench-rs")]
        let _scope = canbench_rs::bench_scope("post_upgrade::load_stable_memory");
        (
            OrderHistory::new(
                storage::order_history_memory(),
                storage::user_orders_memory(),
            ),
            FillStore::new(
                storage::fills_memory(),
                storage::fills_by_user_memory(),
                storage::fills_seq_memory(),
            ),
            TokenBalance::new(storage::balances_memory()),
            UserRegistry::new(storage::user_registry_memory()),
        )
    };

    let snapshot = {
        #[cfg(feature = "canbench-rs")]
        let _scope = canbench_rs::bench_scope("post_upgrade::load_snapshot");
        storage::state_snapshot::load().expect(
            "missing state snapshot at post_upgrade — pre_upgrade trapped or was skipped; \
             manual recovery required",
        )
    };
    {
        #[cfg(feature = "canbench-rs")]
        let _scope = canbench_rs::bench_scope("post_upgrade::into_state");
        state::init_state(snapshot.into_state(order_history, fill_store, balances, user_registry));
    }

    match arg {
        Some(OisyTradeArg::Init(_)) => {
            panic!("ERROR: expected Upgrade argument");
        }
        Some(OisyTradeArg::Upgrade(Some(upgrade_arg))) => {
            state::with_state_mut(|s| {
                let permit = s.permissions().permit_admin();
                audit::process_event(s, EventType::Upgrade(upgrade_arg), permit.into(), runtime)
            });
        }
        Some(OisyTradeArg::Upgrade(None)) | None => {}
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
        crate::drive_matching();
    });
}
