use super::UserOrderKey;
use crate::order::{
    OrderBookId, OrderHistory, OrderId, OrderRecord, OrderSeq, OrderStatus, Price, Quantity, Side,
};
use crate::test_fixtures::arbitrary::arb_order_record;
use crate::user::UserId;
use candid::Principal;
use ic_stable_structures::{Storable, VectorMemory};
use proptest::{prop_assert_eq, proptest};

fn history() -> OrderHistory<VectorMemory> {
    OrderHistory::new(VectorMemory::default(), VectorMemory::default())
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
    history.insert_once(id, UserId::new(0), record.clone());

    assert_eq!(history.get(&id), Some(record));
}

#[test]
#[should_panic(expected = "duplicate order ID")]
fn insert_once_panics_on_duplicate() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(id, UserId::new(0), test_record());
    history.insert_once(id, UserId::new(0), test_record());
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
    history.insert_once(id, UserId::new(0), test_record());

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

#[test]
fn orders_after_returns_newest_first() {
    let mut history = history();
    let owner = UserId::new(7);
    history.insert_once(order_id(0), owner, test_record());
    history.insert_once(order_id(1), owner, test_record());
    history.insert_once(order_id(2), owner, test_record());

    assert_eq!(
        history.orders_after(owner, None, 10),
        vec![order_id(2), order_id(1), order_id(0)]
    );
}

#[test]
fn orders_after_paginates_by_cursor() {
    let mut history = history();
    let owner = UserId::new(7);
    for seq in 0..5 {
        history.insert_once(order_id(seq), owner, test_record());
    }
    // Newest first: seq 4, 3, 2, 1, 0. The cursor is the previous page's
    // last order.
    assert_eq!(
        history.orders_after(owner, None, 2),
        vec![order_id(4), order_id(3)]
    );
    assert_eq!(
        history.orders_after(owner, Some(order_id(3)), 2),
        vec![order_id(2), order_id(1)]
    );
    assert_eq!(
        history.orders_after(owner, Some(order_id(1)), 2),
        vec![order_id(0)]
    );
    assert_eq!(
        history.orders_after(owner, Some(order_id(0)), 2),
        Vec::<OrderId>::new()
    );
}

#[test]
fn orders_after_unknown_cursor_yields_empty() {
    let mut history = history();
    let owner = UserId::new(7);
    history.insert_once(order_id(0), owner, test_record());

    assert_eq!(
        history.orders_after(owner, Some(order_id(99)), 10),
        Vec::<OrderId>::new()
    );
}

#[test]
fn orders_after_isolates_owners() {
    let mut history = history();
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    // Interleaved global sequence: alice, bob, alice.
    history.insert_once(order_id(0), alice, test_record());
    history.insert_once(order_id(1), bob, test_record());
    history.insert_once(order_id(2), alice, test_record());

    assert_eq!(
        history.orders_after(alice, None, 10),
        vec![order_id(2), order_id(0)]
    );
    assert_eq!(history.orders_after(bob, None, 10), vec![order_id(1)]);
    assert_eq!(
        history.orders_after(UserId::new(3), None, 10),
        Vec::<OrderId>::new()
    );
}

#[test]
fn orders_after_orders_across_books_by_global_seq() {
    let mut history = history();
    let owner = UserId::new(1);
    let book0_first = OrderId::new(OrderBookId::ZERO, OrderSeq::new(5));
    let book1 = OrderId::new(OrderBookId::new(1), OrderSeq::new(0));
    let book0_second = OrderId::new(OrderBookId::ZERO, OrderSeq::new(6));
    history.insert_once(book0_first, owner, test_record());
    history.insert_once(book1, owner, test_record());
    history.insert_once(book0_second, owner, test_record());

    assert_eq!(
        history.orders_after(owner, None, 10),
        vec![book0_second, book1, book0_first]
    );
}

/// `UserOrderKey`'s derived `Ord` must agree with its `Storable` byte order,
/// since `StableBTreeMap` relies on that consistency for range scans.
#[test]
fn user_order_key_ord_matches_storable_bytes() {
    let keys = [
        UserOrderKey::from_seq(UserId::new(2), 0),
        UserOrderKey::from_seq(UserId::new(1), 0),
        UserOrderKey::from_seq(UserId::new(1), 5),
        UserOrderKey::from_seq(UserId::new(1), 9),
        UserOrderKey::newest(UserId::new(0)),
        UserOrderKey::oldest(UserId::new(0)),
    ];
    for a in &keys {
        for b in &keys {
            assert_eq!(
                a.cmp(b),
                a.to_bytes().cmp(&b.to_bytes()),
                "Ord disagrees with Storable bytes for {a:?} vs {b:?}"
            );
        }
        assert_eq!(
            UserOrderKey::from_bytes(a.to_bytes()),
            *a,
            "Storable round-trip mismatch for {a:?}"
        );
    }
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
