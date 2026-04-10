use crate::state::audit;
use crate::state::event::EventType;
use crate::{MATCHING_INTERVAL, state, storage};
use dex_types_internal::DexArg;
use dex_types_internal::log::Priority;

pub fn init(arg: DexArg) {
    let init_arg = match arg {
        DexArg::Init(init_arg) => init_arg,
        DexArg::Upgrade(_) => {
            panic!("ERROR: expected Init argument");
        }
    };
    state::init_state(state::State::try_from(init_arg.clone()).expect("ERROR: invalid init args"));
    storage::record_event(EventType::Init(init_arg));
    setup_timers();
    canlog::log!(Priority::Info, "[init]: DEX canister initialized");
}

pub fn post_upgrade(arg: Option<DexArg>) {
    let start = ic_cdk::api::instruction_counter();

    let state = storage::with_event_iter(|events| audit::replay_events(events));
    state::init_state(state);

    match arg {
        Some(DexArg::Init(_)) => {
            panic!("ERROR: expected Upgrade argument");
        }
        Some(DexArg::Upgrade(Some(upgrade_arg))) => {
            state::with_state_mut(|s| audit::process_event(s, EventType::Upgrade(upgrade_arg)));
        }
        Some(DexArg::Upgrade(None)) | None => {}
    }

    let instructions_used = ic_cdk::api::instruction_counter() - start;
    canlog::log!(
        Priority::Info,
        "[post_upgrade]: replaying {} events consumed {} instructions",
        storage::total_event_count(),
        instructions_used,
    );
    setup_timers();
}

fn setup_timers() {
    ic_cdk_timers::set_timer_interval(MATCHING_INTERVAL, || async {
        crate::process_pending_orders();
    });
}
