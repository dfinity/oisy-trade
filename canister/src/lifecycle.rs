use crate::state::audit;
use crate::state::event::EventType;
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
    state::init_state(state::State::try_from(init_arg.clone()).expect("ERROR: invalid init args"));
    storage::record_event(runtime.time(), EventType::Init(init_arg));
    setup_timers();
    canlog::log!(Priority::Info, "[init]: DEX canister initialized");
}

pub fn pre_upgrade() {
    state::with_state(|s| {
        storage::save_order_books(s.order_books());
        storage::save_balances(s.balances());
    });
    canlog::log!(
        Priority::Info,
        "[pre_upgrade]: state saved to stable memory"
    );
}

pub fn post_upgrade(arg: Option<DexArg>, runtime: &impl Runtime) {
    let start = runtime.instruction_counter();

    let state = storage::with_event_iter(|events| audit::replay_events(events));
    state::init_state(state);
    let replayed_events = storage::total_event_count();

    state::with_state_mut(|s| {
        if let Some(order_books) = storage::load_order_books() {
            s.set_order_books(order_books);
        }
        if let Some(balances) = storage::load_balances() {
            s.set_balances(balances);
        }
    });

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
        "[post_upgrade]: replayed {} events, total instructions used: {}",
        replayed_events,
        instructions_used,
    );
    setup_timers();
}

fn setup_timers() {
    ic_cdk_timers::set_timer_interval(MATCHING_INTERVAL, || async {
        crate::process_pending_orders();
    });
}
