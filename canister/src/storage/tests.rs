use super::state_snapshot;
use crate::state::StateSnapshot;
use oisy_trade_types_internal::Mode;

fn empty_snapshot() -> StateSnapshot {
    StateSnapshot {
        mode: Mode::GeneralAvailability,
        next_book_id: Default::default(),
        tokens: vec![],
        trading_pairs: vec![],
        order_books: vec![],
        ledger_fee_cache: vec![],
        pending_settling_events: None,
        max_orders_per_chunk: None,
        instruction_budget: None,
        fee_pool: None,
    }
}

#[test]
fn load_consumes_snapshot_so_a_skipped_pre_upgrade_traps() {
    let snapshot = empty_snapshot();
    state_snapshot::save(&snapshot);

    let first = state_snapshot::load().expect("first load should find the saved snapshot");
    assert_eq!(first, snapshot);

    let second = state_snapshot::load();
    assert!(
        second.is_none(),
        "second load should return None after the cell was consumed"
    );
}
