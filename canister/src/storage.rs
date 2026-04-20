use crate::state::event::{Event, EventType};
use ic_stable_structures::DefaultMemoryImpl;
use ic_stable_structures::StableLog;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use std::cell::RefCell;

const EVENT_LOG_INDEX_MEMORY_ID: MemoryId = MemoryId::new(0);
const EVENT_LOG_DATA_MEMORY_ID: MemoryId = MemoryId::new(1);
const ORDER_HISTORY_MEMORY_ID: MemoryId = MemoryId::new(2);
const BALANCES_MEMORY_ID: MemoryId = MemoryId::new(3);

pub type VMem = VirtualMemory<DefaultMemoryImpl>;
type EventLog = StableLog<Event, VMem, VMem>;

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
}

pub fn record_event(timestamp: u64, payload: EventType) {
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
