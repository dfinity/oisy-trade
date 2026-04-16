use crate::order::{OrderBook, OrderBookId};
use crate::state::event::{Event, EventType};
use ic_stable_structures::Memory;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableLog};
use std::cell::RefCell;
use std::collections::BTreeMap;

const EVENT_LOG_INDEX_MEMORY_ID: MemoryId = MemoryId::new(0);
const EVENT_LOG_DATA_MEMORY_ID: MemoryId = MemoryId::new(1);
const ORDER_BOOK_MEMORY_ID: MemoryId = MemoryId::new(2);

const WASM_PAGE_SIZE: u64 = 65_536;

type VMem = VirtualMemory<DefaultMemoryImpl>;
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

// ---------------------------------------------------------------------------
// Order book stable memory persistence
// ---------------------------------------------------------------------------

fn with_order_book_memory<R>(f: impl FnOnce(VMem) -> R) -> R {
    MEMORY_MANAGER.with(|m| f(m.borrow().get(ORDER_BOOK_MEMORY_ID)))
}

/// Write raw bytes to the order book stable memory region.
///
/// Format: 8-byte little-endian length prefix followed by the payload.
pub fn write_bytes_to_stable(bytes: &[u8]) {
    with_order_book_memory(|mem| {
        let total = 8 + bytes.len() as u64;
        let needed_pages = (total + WASM_PAGE_SIZE - 1) / WASM_PAGE_SIZE;
        let current_pages = mem.size();
        if needed_pages > current_pages {
            mem.grow(needed_pages - current_pages);
        }
        mem.write(0, &(bytes.len() as u64).to_le_bytes());
        mem.write(8, bytes);
    })
}

/// Read raw bytes previously written by [`write_bytes_to_stable`].
///
/// Returns an empty `Vec` if the region has never been written to.
pub fn read_bytes_from_stable() -> Vec<u8> {
    with_order_book_memory(|mem| {
        if mem.size() == 0 {
            return Vec::new();
        }
        let mut len_buf = [0u8; 8];
        mem.read(0, &mut len_buf);
        let len = u64::from_le_bytes(len_buf) as usize;
        if len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; len];
        mem.read(8, &mut buf);
        buf
    })
}

/// Serialize all order books and write them to stable memory.
pub fn save_order_books(order_books: &BTreeMap<OrderBookId, OrderBook>) {
    #[cfg(feature = "canbench-rs")]
    let _p = canbench_rs::bench_scope("encode");
    let bytes = minicbor::to_vec(order_books).expect("order book encoding should always succeed");
    #[cfg(feature = "canbench-rs")]
    let _q = canbench_rs::bench_scope("write_stable");
    write_bytes_to_stable(&bytes);
}

/// Load order books from stable memory, if previously saved.
pub fn load_order_books() -> Option<BTreeMap<OrderBookId, OrderBook>> {
    #[cfg(feature = "canbench-rs")]
    let _p = canbench_rs::bench_scope("read_stable");
    let bytes = read_bytes_from_stable();
    if bytes.is_empty() {
        return None;
    }
    #[cfg(feature = "canbench-rs")]
    let _q = canbench_rs::bench_scope("decode");
    Some(minicbor::decode(&bytes).unwrap_or_else(|e| panic!("failed to decode order books: {e}")))
}
