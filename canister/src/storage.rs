use crate::order::{OrderId, OrderRecord};
use crate::state::event::{Event, EventType};
use dex_types::OrderStatus;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableLog};
use std::cell::RefCell;

const EVENT_LOG_INDEX_MEMORY_ID: MemoryId = MemoryId::new(0);
const EVENT_LOG_DATA_MEMORY_ID: MemoryId = MemoryId::new(1);
const ORDER_HISTORY_MEMORY_ID: MemoryId = MemoryId::new(2);

type VMem = VirtualMemory<DefaultMemoryImpl>;
type EventLog = StableLog<Event, VMem, VMem>;
type OrderHistoryMap = StableBTreeMap<OrderId, OrderRecord, VMem>;

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

    static ORDER_HISTORY: RefCell<OrderHistoryMap> = MEMORY_MANAGER.with(|m| {
        RefCell::new(StableBTreeMap::init(m.borrow().get(ORDER_HISTORY_MEMORY_ID)))
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

pub mod order_history {
    use super::{ORDER_HISTORY, OrderId, OrderRecord, OrderStatus};

    /// Insert a new order record. Panics if the order ID already exists.
    pub fn insert_once(id: OrderId, record: OrderRecord) {
        ORDER_HISTORY.with(|map| {
            let mut map = map.borrow_mut();
            assert!(!map.contains_key(&id), "BUG: duplicate order ID {id}",);
            map.insert(id, record);
        });
    }

    /// Returns a copy of the record for the given order, or `None` if absent.
    pub fn get(id: &OrderId) -> Option<OrderRecord> {
        ORDER_HISTORY.with(|map| map.borrow().get(id))
    }

    /// Returns the status of the given order, or [`OrderStatus::NotFound`] if absent.
    pub fn get_status(id: &OrderId) -> OrderStatus {
        ORDER_HISTORY.with(|map| {
            map.borrow()
                .get(id)
                .map(|r| r.status.clone())
                .unwrap_or(OrderStatus::NotFound)
        })
    }

    /// Updates the status of an existing order. Panics if the order is unknown.
    pub fn set_status(id: &OrderId, status: OrderStatus) {
        ORDER_HISTORY.with(|map| {
            let mut map = map.borrow_mut();
            let mut record = map
                .get(id)
                .unwrap_or_else(|| panic!("BUG: order {id} missing from order_history"));
            record.status = status;
            map.insert(*id, record);
        });
    }

    /// Removes every entry. Intended only for unit tests so that iterations
    /// of a proptest or consecutive `#[test]`s on the same thread start from
    /// a clean map.
    #[cfg(test)]
    pub fn clear_for_test() {
        ORDER_HISTORY.with(|map| {
            let mut map = map.borrow_mut();
            let ids: Vec<OrderId> = map.iter().map(|entry| *entry.key()).collect();
            for id in ids {
                map.remove(&id);
            }
        });
    }
}
