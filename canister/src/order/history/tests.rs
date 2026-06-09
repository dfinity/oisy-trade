use super::{SeqOrderRecord, USER_ORDER_KEY_LEN, UserOrderKey};
use crate::order::{
    OrderBookId, OrderHistory, OrderId, OrderRecord, OrderSeq, OrderStatus, Price, Quantity, Side,
};
use crate::test_fixtures::arbitrary::arb_order_record;
use crate::user::UserId;
use candid::Principal;
use ic_stable_structures::{Storable, VectorMemory};
use proptest::prelude::*;

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

fn arb_user_order_key() -> impl Strategy<Value = UserOrderKey> {
    (any::<u64>(), any::<u64>())
        .prop_map(|(user, seq)| UserOrderKey::from_seq(UserId::new(user), seq))
}

fn arb_seq_order_record() -> impl Strategy<Value = SeqOrderRecord> {
    (any::<u64>(), arb_order_record()).prop_map(|(seq, record)| SeqOrderRecord { seq, record })
}

#[test]
fn insert_once_and_get() {
    let mut history = history();
    let id = order_id(0);
    let record = test_record();
    history.insert_once(UserId::new(0), id, record.clone());

    assert_eq!(history.get(&id), Some(record));
}

#[test]
#[should_panic(expected = "duplicate order ID")]
fn insert_once_panics_on_duplicate() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(UserId::new(0), id, test_record());
    history.insert_once(UserId::new(0), id, test_record());
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
    history.insert_once(UserId::new(0), id, test_record());

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
    history.insert_once(owner, order_id(0), test_record());
    history.insert_once(owner, order_id(1), test_record());
    history.insert_once(owner, order_id(2), test_record());

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
        history.insert_once(owner, order_id(seq), test_record());
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
    history.insert_once(owner, order_id(0), test_record());

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
    history.insert_once(alice, order_id(0), test_record());
    history.insert_once(bob, order_id(1), test_record());
    history.insert_once(alice, order_id(2), test_record());

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
    history.insert_once(owner, book0_first, test_record());
    history.insert_once(owner, book1, test_record());
    history.insert_once(owner, book0_second, test_record());

    assert_eq!(
        history.orders_after(owner, None, 10),
        vec![book0_second, book1, book0_first]
    );
}

proptest! {
    /// `UserOrderKey`'s derived `Ord` must agree with its `Storable` byte order,
    /// since `StableBTreeMap` relies on that consistency for range scans.
    #[test]
    fn user_order_key_ord_matches_storable_bytes(
        keys in prop::collection::vec(arb_user_order_key(), 0..16),
    ) {
        for a in &keys {
            for b in &keys {
                prop_assert_eq!(
                    a.cmp(b),
                    a.to_bytes().cmp(&b.to_bytes()),
                    "Ord disagrees with Storable bytes for {:?} vs {:?}",
                    a,
                    b
                );
            }
        }
    }

    #[test]
    fn user_order_key_roundtrips_through_storable(key in arb_user_order_key()) {
        prop_assert_eq!(UserOrderKey::from_bytes(key.to_bytes()), key);
    }

    #[test]
    fn user_order_key_encodes_to_fixed_len(key in arb_user_order_key()) {
        prop_assert_eq!(key.to_bytes().len(), USER_ORDER_KEY_LEN);
    }

    #[test]
    fn seq_order_record_roundtrips_through_storable(entry in arb_seq_order_record()) {
        prop_assert_eq!(SeqOrderRecord::from_bytes(entry.to_bytes()), entry);
    }
}
