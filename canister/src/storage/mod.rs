use crate::Timestamp;
use crate::state::event::{Event, EventType};
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{Cell, StableLog};
use std::cell::RefCell;

const EVENT_LOG_INDEX_MEMORY_ID: MemoryId = MemoryId::new(0);
const EVENT_LOG_DATA_MEMORY_ID: MemoryId = MemoryId::new(1);
const ORDER_HISTORY_MEMORY_ID: MemoryId = MemoryId::new(2);
const BALANCES_MEMORY_ID: MemoryId = MemoryId::new(3);
const STATE_SNAPSHOT_MEMORY_ID: MemoryId = MemoryId::new(4);
const USER_REGISTRY_MEMORY_ID: MemoryId = MemoryId::new(5);
const USER_ORDERS_MEMORY_ID: MemoryId = MemoryId::new(6);

pub type VMem = VirtualMemory<DefaultMemoryImpl>;
type EventLog = StableLog<Event, VMem, VMem>;
type StateSnapshotCell = Cell<Vec<u8>, VMem>;

thread_local! {
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> = RefCell::new(
        MemoryManager::init(DefaultMemoryImpl::default())
    );

    static EVENTS: RefCell<EventLog> = MEMORY_MANAGER.with(|m| {
        RefCell::new(
            StableLog::init(
                m.borrow().get(EVENT_LOG_INDEX_MEMORY_ID),
                m.borrow().get(EVENT_LOG_DATA_MEMORY_ID),
            )
        )
    });

    static STATE_SNAPSHOT: RefCell<StateSnapshotCell> = MEMORY_MANAGER.with(|m| {
        RefCell::new(Cell::init(m.borrow().get(STATE_SNAPSHOT_MEMORY_ID), Vec::new()))
    });
}

pub fn record_event(timestamp: Timestamp, payload: EventType) {
    EVENTS
        .with(|events| events.borrow().append(&Event { timestamp, payload }))
        .expect("recording an event should succeed");
}

pub fn total_event_count() -> u64 {
    EVENTS.with(|events| events.borrow().len())
}

pub fn get_event(idx: u64) -> Option<Event> {
    EVENTS.with(|events| events.borrow().get(idx))
}

pub fn with_event_iter<F, R>(f: F) -> R
where
    F: for<'a> FnOnce(Box<dyn Iterator<Item = Event> + 'a>) -> R,
{
    EVENTS.with(|events| f(Box::new(events.borrow().iter())))
}

/// Returns the virtual memory slice dedicated to the order-history map.
/// Used to construct the production `OrderHistory<VMem>` on canister
/// `init` / `post_upgrade`.
pub fn order_history_memory() -> VMem {
    MEMORY_MANAGER.with(|m| m.borrow().get(ORDER_HISTORY_MEMORY_ID))
}

/// Returns the virtual memory slice dedicated to the balances map.
/// Used to construct the production `TokenBalance<VMem>` on canister
/// `init` / `post_upgrade`.
pub fn balances_memory() -> VMem {
    MEMORY_MANAGER.with(|m| m.borrow().get(BALANCES_MEMORY_ID))
}

/// Returns the virtual memory slice dedicated to the user registry.
/// Used to construct the production `UserRegistry<VMem>` on canister
/// `init` / `post_upgrade`.
pub fn user_registry_memory() -> VMem {
    MEMORY_MANAGER.with(|m| m.borrow().get(USER_REGISTRY_MEMORY_ID))
}

/// Returns the virtual memory slice that backs `OrderHistory`'s per-user
/// order index (the second region passed to `OrderHistory::new`). Distinct
/// from [`order_history_memory`], which backs the primary order store.
pub fn user_orders_memory() -> VMem {
    MEMORY_MANAGER.with(|m| m.borrow().get(USER_ORDERS_MEMORY_ID))
}
pub mod state_snapshot {
    use super::STATE_SNAPSHOT;
    use crate::state::StateSnapshot;

    /// Writes `snapshot` to the snapshot cell, overwriting any previous
    /// value. Returns the number of encoded bytes so callers can log/monitor
    /// snapshot size. Called from `pre_upgrade`.
    pub fn save(snapshot: &StateSnapshot) -> usize {
        let mut buf = vec![];
        minicbor::encode(snapshot, &mut buf).expect("state snapshot encoding should succeed");
        let size = buf.len();
        STATE_SNAPSHOT.with(|cell| {
            let _ = cell.borrow_mut().set(buf);
        });
        size
    }

    /// Reads the snapshot written by the previous canister version, or
    /// `None` on a fresh install (the cell still holds the `Vec::new()`
    /// default). On a successful decode the cell is cleared, so if a future
    /// upgrade skips `pre_upgrade`, the next `post_upgrade` observes an
    /// empty cell and traps instead of restoring stale state from an older
    /// version. Clearing does not reclaim stable memory pages (stable memory
    /// on the IC can't shrink) but it does remove the stale payload so later
    /// loads avoid reading and decoding it until the next `pre_upgrade`
    /// writes a new snapshot.
    pub fn load() -> Option<StateSnapshot> {
        STATE_SNAPSHOT.with(|cell| {
            let mut cell = cell.borrow_mut();
            if cell.get().is_empty() {
                return None;
            }
            let snapshot = minicbor::decode::<StateSnapshot>(cell.get().as_slice())
                .expect("state snapshot decoding should succeed");
            let _ = cell.set(Vec::new());
            Some(snapshot)
        })
    }
}

#[cfg(test)]
mod tests;
