use crate::Timestamp;
use crate::order::{
    OrderBookId, OrderHistory, OrderId, OrderRecord, OrderSeq, OrderStatus, OrderUpdate, Price,
    Quantity, Side, TimeInForce,
};
use crate::user::UserId;
use candid::Principal;
use ic_stable_structures::VectorMemory;

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
        placed_by: None,
    }
}

proptest::proptest! {
    #[test]
    fn should_roundtrip_order_record_through_cbor(
        record in crate::test_fixtures::arbitrary::arb_order_record(),
    ) {
        let mut buf = vec![];
        minicbor::encode(&record, &mut buf).unwrap();
        let decoded: OrderRecord = minicbor::decode(&buf).unwrap();
        proptest::prelude::prop_assert_eq!(record, decoded);
    }
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
