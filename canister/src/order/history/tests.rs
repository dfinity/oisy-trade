use super::{SeqOrderRecord, USER_ORDER_KEY_LEN, UserOrderKey};
use crate::Timestamp;
use crate::order::{
    OrderBookId, OrderHistory, OrderId, OrderRecord, OrderSeq, OrderStatus, OrderUpdate, Price,
    Quantity, Side, TimeInForce,
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
        filled_quantity: Quantity::ZERO,
        status: OrderStatus::Pending,
        created_at: Timestamp::EPOCH,
        last_updated_at: None,
        time_in_force: TimeInForce::FillOrKill,
        filled_quote: Quantity::ZERO,
        filled_fee: Quantity::ZERO,
    }
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
fn record_roundtrips_fill_or_kill_through_history() {
    let mut history = history();
    let id = order_id(0);
    let record = test_record();
    assert_eq!(record.time_in_force, TimeInForce::FillOrKill);
    history.insert_once(UserId::new(0), id, record.clone());

    let loaded = history.get(&id).unwrap();
    assert_eq!(loaded.time_in_force, TimeInForce::FillOrKill);
    assert_eq!(loaded, record);
}

#[test]
fn public_record_surfaces_time_in_force() {
    let record = test_record();
    let public: oisy_trade_types::OrderRecord = record.into();
    assert_eq!(
        public.time_in_force,
        oisy_trade_types::TimeInForce::FillOrKill
    );

    let mut gtc = test_record();
    gtc.time_in_force = TimeInForce::GoodTilCanceled;
    let public: oisy_trade_types::OrderRecord = gtc.into();
    assert_eq!(
        public.time_in_force,
        oisy_trade_types::TimeInForce::GoodTilCanceled
    );
}

#[test]
fn apply_update_status_only() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(UserId::new(0), id, test_record());

    history.apply_update(
        &id,
        OrderUpdate::status(OrderStatus::Filled),
        Timestamp::new(7),
    );
    let record = history.get(&id).expect("record present");
    assert_eq!(record.status, OrderStatus::Filled);
    assert_eq!(record.filled_quantity, Quantity::ZERO);
    assert_eq!(record.last_updated_at, Some(Timestamp::new(7)));
}

#[test]
fn apply_update_delta_only() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(UserId::new(0), id, test_record());

    history.apply_update(
        &id,
        OrderUpdate::filled(Quantity::from(400_000u64)),
        Timestamp::new(9),
    );
    let record = history.get(&id).expect("record present");
    // Status untouched, only the fill advanced.
    assert_eq!(record.status, OrderStatus::Pending);
    assert_eq!(record.filled_quantity, Quantity::from(400_000u64));
    assert_eq!(record.last_updated_at, Some(Timestamp::new(9)));
}

#[test]
fn apply_update_status_and_delta_in_one_write() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(UserId::new(0), id, test_record());

    history.apply_update(
        &id,
        OrderUpdate {
            status: Some(OrderStatus::Open),
            filled_delta: Quantity::from(300_000u64),
            quote_delta: Quantity::from(3u64),
            fee_delta: Quantity::from(1u64),
        },
        Timestamp::new(11),
    );
    history.apply_update(
        &id,
        OrderUpdate {
            status: Some(OrderStatus::Filled),
            filled_delta: Quantity::from(700_000u64),
            quote_delta: Quantity::from(7u64),
            fee_delta: Quantity::from(2u64),
        },
        Timestamp::new(13),
    );
    let record = history.get(&id).expect("record present");
    // The fill, quote, and fee deltas all accumulate within the same single
    // read-modify-write; status reflects the latest update.
    assert_eq!(record.status, OrderStatus::Filled);
    assert_eq!(record.filled_quantity, Quantity::from(1_000_000u64));
    assert_eq!(record.filled_quote, Quantity::from(10u64));
    assert_eq!(record.filled_fee, Quantity::from(3u64));
    assert_eq!(record.last_updated_at, Some(Timestamp::new(13)));
}

#[test]
fn apply_update_accumulates_quote_and_fee_in_one_write() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(UserId::new(0), id, test_record());

    // A fill-only update that carries quote and fee deltas writes all three
    // scalars (and `last_updated_at`) in a single read-modify-write, leaving
    // status untouched.
    history.apply_update(
        &id,
        OrderUpdate {
            status: None,
            filled_delta: Quantity::from(200_000u64),
            quote_delta: Quantity::from(20_000_000u64),
            fee_delta: Quantity::from(10_000u64),
        },
        Timestamp::new(5),
    );
    let record = history.get(&id).expect("record present");
    assert_eq!(record.status, OrderStatus::Pending);
    assert_eq!(record.filled_quantity, Quantity::from(200_000u64));
    assert_eq!(record.filled_quote, Quantity::from(20_000_000u64));
    assert_eq!(record.filled_fee, Quantity::from(10_000u64));
    assert_eq!(record.last_updated_at, Some(Timestamp::new(5)));
}

#[test]
#[should_panic(expected = "BUG: filled_quote overflow")]
fn apply_update_traps_on_filled_quote_overflow() {
    // The monotonic `filled_quote` invariant is enforced by an always-on trap,
    // not a `debug_assert!` compiled out of the release canister: starting from
    // `Quantity::MAX`, any positive `quote_delta` overflows and must panic even
    // when tests run in release config.
    let mut record = test_record();
    record.filled_quote = Quantity::MAX;
    OrderUpdate {
        status: None,
        filled_delta: Quantity::ZERO,
        quote_delta: Quantity::from(1u64),
        fee_delta: Quantity::ZERO,
    }
    .apply(&mut record);
}

#[test]
#[should_panic(expected = "BUG: filled_fee overflow")]
fn apply_update_traps_on_filled_fee_overflow() {
    let mut record = test_record();
    record.filled_fee = Quantity::MAX;
    OrderUpdate {
        status: None,
        filled_delta: Quantity::ZERO,
        quote_delta: Quantity::ZERO,
        fee_delta: Quantity::from(1u64),
    }
    .apply(&mut record);
}

#[test]
fn apply_update_is_a_noop_when_update_is_a_noop() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(UserId::new(0), id, test_record());

    // An empty update is a no-op: nothing written, `last_updated_at` stays None.
    history.apply_update(&id, OrderUpdate::default(), Timestamp::new(99));
    assert_eq!(history.get(&id), Some(test_record()));

    // A status equal to the current one with a zero delta is also a no-op.
    history.apply_update(
        &id,
        OrderUpdate::status(OrderStatus::Pending),
        Timestamp::new(99),
    );
    assert_eq!(history.get(&id), Some(test_record()));
}

#[test]
fn apply_update_does_not_change_last_updated_at_on_noop() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(UserId::new(0), id, test_record());

    // First, a real update stamps `last_updated_at`.
    history.apply_update(
        &id,
        OrderUpdate::status(OrderStatus::Open),
        Timestamp::new(5),
    );
    let after_real = history.get(&id).expect("record present");
    assert_eq!(after_real.last_updated_at, Some(Timestamp::new(5)));

    // A subsequent no-op (same status, zero delta) leaves the record — and so
    // `last_updated_at` — untouched, despite a later `now`.
    history.apply_update(
        &id,
        OrderUpdate::status(OrderStatus::Open),
        Timestamp::new(42),
    );
    assert_eq!(history.get(&id), Some(after_real));
}

#[test]
#[should_panic(expected = "BUG: filled_quantity")]
fn apply_update_traps_when_filled_exceeds_quantity() {
    let mut history = history();
    let id = order_id(0);
    history.insert_once(UserId::new(0), id, test_record());

    // quantity is 1_000_000; overfilling by one lot must trap — an always-on
    // check, not a `debug_assert!` compiled out of the release canister.
    history.apply_update(
        &id,
        OrderUpdate::filled(Quantity::from(1_000_001u64)),
        Timestamp::new(1),
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
fn orders_after_foreign_cursor_yields_empty() {
    let mut history = history();
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    history.insert_once(alice, order_id(0), test_record());
    history.insert_once(bob, order_id(1), test_record());
    history.insert_once(alice, order_id(2), test_record());

    // bob's order is a real, known `OrderId`, but it isn't alice's — paging
    // alice's history after it must not skip into the middle of her orders.
    assert_eq!(
        history.orders_after(alice, Some(order_id(1)), 10),
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
        keys in prop::collection::vec(arb_user_order_key(), 0..100),
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

fn arb_user_order_key() -> impl Strategy<Value = UserOrderKey> {
    (any::<u64>(), any::<u64>())
        .prop_map(|(user, seq)| UserOrderKey::from_seq(UserId::new(user), seq))
}

fn arb_seq_order_record() -> impl Strategy<Value = SeqOrderRecord> {
    (any::<u64>(), arb_order_record()).prop_map(|(seq, record)| SeqOrderRecord { seq, record })
}
