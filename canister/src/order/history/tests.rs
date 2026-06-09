use crate::order::{
    OrderBookId, OrderHistory, OrderId, OrderRecord, OrderSeq, OrderStatus, Price, Quantity, Side,
};
use crate::test_fixtures::arbitrary::arb_order_record;
use candid::Principal;
use ic_stable_structures::{Storable, VectorMemory};
use proptest::{prop_assert_eq, proptest};

fn history() -> OrderHistory<VectorMemory> {
    OrderHistory::new(VectorMemory::default())
}

fn order_id(seq: u64) -> OrderId {
    OrderId::new(OrderBookId::ZERO, OrderSeq::new(seq))
}

fn test_record() -> OrderRecord {
    OrderRecord {
        owner: Principal::anonymous(),
        side: Side::Buy,
        price: Price::new(100),
        quantity: Quantity::from(1_000_000u64),
        status: OrderStatus::Pending,
        timestamp: crate::Timestamp::EPOCH,
    }
}

#[test]
fn insert_once_and_get() {
    let mut history = history();
    let id = order_id(0);
    let record = test_record();
    history.insert_once(id, record.clone());

    assert_eq!(history.get(&id), Some(record));
}

#[test]
#[should_panic(expected = "duplicate order ID")]
fn insert_once_panics_on_duplicate() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(id, test_record());
    history.insert_once(id, test_record());
}

#[test]
fn get_returns_none_for_missing() {
    let history = history();
    assert_eq!(history.get(&order_id(42)), None);
}

#[test]
fn set_status_updates_status() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(id, test_record());

    assert_eq!(
        history.get(&id).map(|r| r.status),
        Some(OrderStatus::Pending),
    );
    history.set_status(&id, OrderStatus::Filled);
    assert_eq!(
        history.get(&id).map(|r| r.status),
        Some(OrderStatus::Filled),
    );
}

proptest! {
    #[test]
    fn should_store_order_record(
        record in arb_order_record(),
    ) {
        let bytes = record.to_bytes();
        let decoded = OrderRecord::from_bytes(bytes);
        prop_assert_eq!(decoded, record);
    }
}
