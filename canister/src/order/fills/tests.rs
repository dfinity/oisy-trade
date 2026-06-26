use super::{CursorNotFound, FillId, FillIdParseError, FillRecord, FillSeq, FillStore};
use crate::Timestamp;
use crate::order::{OrderBookId, OrderId, OrderSeq, PairToken, Price, Quantity, Side};
use crate::user::UserId;
use ic_stable_structures::VectorMemory;

const USER: UserId = UserId::new(0);

#[test]
fn should_roundtrip_fill_id_through_display_and_parse() {
    let id = FillId::new(order(7), FillSeq::new(42));
    let parsed: FillId = id.to_string().parse().unwrap();
    assert_eq!(parsed, id);
    assert_eq!(id.to_string().len(), 48);
    assert!(id.to_string().chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn should_reject_a_malformed_fill_id() {
    assert_eq!("".parse::<FillId>(), Err(FillIdParseError));
    assert_eq!(
        "0".repeat(47).parse::<FillId>(),
        Err(FillIdParseError),
        "too short"
    );
    assert_eq!(
        "0".repeat(49).parse::<FillId>(),
        Err(FillIdParseError),
        "too long"
    );
    assert_eq!(
        "z".repeat(48).parse::<FillId>(),
        Err(FillIdParseError),
        "non-hex"
    );
}

#[test]
fn should_append_two_side_projected_records_and_advance_seq_by_two() {
    let mut store = store();
    let order_a = order(0);
    let order_b = order(1);

    store.append(taker_leg(order_a), USER, maker_leg(order_b), USER);

    assert_eq!(store.len(), 2);
    assert_eq!(store.next_seq(), FillSeq::new(2));

    let a_fills = store.fills_after(order_a, None, 10).unwrap();
    assert_eq!(a_fills.len(), 1);
    let (a_seq, a_record) = &a_fills[0];
    assert_eq!(*a_seq, FillSeq::ZERO);
    assert_eq!(a_record, &taker_leg(order_a));

    let b_fills = store.fills_after(order_b, None, 10).unwrap();
    assert_eq!(b_fills.len(), 1);
    let (b_seq, b_record) = &b_fills[0];
    assert_eq!(*b_seq, FillSeq::new(1));
    assert_eq!(b_record, &maker_leg(order_b));
}

#[test]
fn should_return_one_orders_fills_newest_first_excluding_other_orders() {
    let mut store = store();
    let order_a = order(0);
    let order_b = order(1);

    // Two fills against order A (seqs 0, 2) interleaved with a fill against B.
    store.append(taker_leg(order_a), USER, maker_leg(order_b), USER);
    store.append(taker_leg(order_a), USER, maker_leg(order_b), USER);

    let a_fills = store.fills_after(order_a, None, 10).unwrap();
    let seqs: Vec<u64> = a_fills.iter().map(|(seq, _)| seq.get()).collect();
    assert_eq!(seqs, vec![2, 0], "newest-first, only order A's fills");
    assert!(a_fills.iter().all(|(_, record)| record.order_id == order_a));
}

#[test]
fn should_page_via_after_cursor() {
    let mut store = store();
    let order_a = order(0);
    let other = order(1);
    for _ in 0..3 {
        store.append(taker_leg(order_a), USER, maker_leg(other), USER);
    }
    // order A's seqs are 0, 2, 4 (newest 4).
    let first = store.fills_after(order_a, None, 2).unwrap();
    assert_eq!(
        first.iter().map(|(s, _)| s.get()).collect::<Vec<_>>(),
        vec![4, 2]
    );

    let cursor = first.last().unwrap().0;
    let second = store.fills_after(order_a, Some(cursor), 2).unwrap();
    assert_eq!(
        second.iter().map(|(s, _)| s.get()).collect::<Vec<_>>(),
        vec![0]
    );
}

#[test]
fn should_return_empty_page_for_unknown_order() {
    let mut store = store();
    store.append(taker_leg(order(0)), USER, maker_leg(order(1)), USER);
    let fills = store.fills_after(order(7), None, 10).unwrap();
    assert!(fills.is_empty());
}

#[test]
fn should_reject_a_cursor_that_is_not_one_of_the_orders_fills() {
    let mut store = store();
    let order_a = order(0);
    store.append(taker_leg(order_a), USER, maker_leg(order(1)), USER);
    // seq 1 belongs to order(1), not order_a.
    assert_eq!(
        store.fills_after(order_a, Some(FillSeq::new(1)), 10),
        Err(CursorNotFound)
    );
    // A seq that does not exist at all.
    assert_eq!(
        store.fills_after(order_a, Some(FillSeq::new(99)), 10),
        Err(CursorNotFound)
    );
}

#[test]
fn should_return_empty_page_for_a_valid_cursor_with_no_older_fills() {
    let mut store = store();
    let order_a = order(0);
    store.append(taker_leg(order_a), USER, maker_leg(order(1)), USER);
    // The only fill of order_a is at seq 0; nothing older.
    let fills = store.fills_after(order_a, Some(FillSeq::ZERO), 10).unwrap();
    assert!(fills.is_empty());
}

#[test]
fn should_clamp_to_requested_length() {
    let mut store = store();
    let order_a = order(0);
    for _ in 0..5 {
        store.append(taker_leg(order_a), USER, maker_leg(order(1)), USER);
    }
    let fills = store.fills_after(order_a, None, 2).unwrap();
    assert_eq!(fills.len(), 2);
}

#[test]
fn should_return_a_users_trades_across_orders_newest_first_scoped_to_owner() {
    let mut store = store();
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let alice_order_a = order(0);
    let alice_order_b = order(1);
    let bob_order = order(2);

    // alice takes against her order A (seq 0), bob's leg on order 2 (seq 1).
    store.append(taker_leg(alice_order_a), alice, maker_leg(bob_order), bob);
    // alice takes against her order B (seq 2), bob's leg again (seq 3).
    store.append(taker_leg(alice_order_b), alice, maker_leg(bob_order), bob);

    let alice_orders: Vec<OrderId> = store
        .trades_after(alice, None, 10)
        .unwrap()
        .iter()
        .map(|(_, r)| r.order_id)
        .collect();
    assert_eq!(
        alice_orders,
        vec![alice_order_b, alice_order_a],
        "alice's fills across both orders, newest-first",
    );

    let bob_orders: Vec<OrderId> = store
        .trades_after(bob, None, 10)
        .unwrap()
        .iter()
        .map(|(_, r)| r.order_id)
        .collect();
    assert_eq!(
        bob_orders,
        vec![bob_order, bob_order],
        "bob sees only his own legs",
    );
}

#[test]
fn should_page_a_users_trades_via_after_cursor() {
    let mut store = store();
    let alice = UserId::new(1);
    let other = UserId::new(2);
    for _ in 0..3 {
        store.append(taker_leg(order(0)), alice, maker_leg(order(1)), other);
    }
    // alice's seqs are 0, 2, 4 (newest 4).
    let first = store.trades_after(alice, None, 2).unwrap();
    assert_eq!(
        first.iter().map(|(s, _)| s.get()).collect::<Vec<_>>(),
        vec![4, 2]
    );

    let (last_seq, last_record) = first.last().unwrap();
    let cursor = FillId::new(last_record.order_id, *last_seq);
    let second = store.trades_after(alice, Some(cursor), 2).unwrap();
    assert_eq!(
        second.iter().map(|(s, _)| s.get()).collect::<Vec<_>>(),
        vec![0]
    );
}

#[test]
fn should_return_an_empty_account_page_for_an_unknown_user() {
    let mut store = store();
    store.append(
        taker_leg(order(0)),
        UserId::new(1),
        maker_leg(order(1)),
        UserId::new(2),
    );
    let trades = store.trades_after(UserId::new(7), None, 10).unwrap();
    assert!(trades.is_empty());
}

#[test]
fn should_reject_an_account_cursor_that_is_not_one_of_the_users_fills() {
    let mut store = store();
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    store.append(taker_leg(order(0)), alice, maker_leg(order(1)), bob);
    // seq 1 belongs to bob, not alice.
    let bob_cursor = FillId::new(order(1), FillSeq::new(1));
    assert_eq!(
        store.trades_after(alice, Some(bob_cursor), 10),
        Err(CursorNotFound)
    );
    // a seq that does not exist at all.
    let unknown = FillId::new(order(0), FillSeq::new(99));
    assert_eq!(
        store.trades_after(alice, Some(unknown), 10),
        Err(CursorNotFound)
    );
}

#[test]
fn should_return_an_empty_account_page_for_a_valid_cursor_with_no_older_fills() {
    let mut store = store();
    let alice = UserId::new(1);
    store.append(
        taker_leg(order(0)),
        alice,
        maker_leg(order(1)),
        UserId::new(2),
    );
    // alice's only fill is at seq 0; nothing older.
    let cursor = FillId::new(order(0), FillSeq::ZERO);
    let trades = store.trades_after(alice, Some(cursor), 10).unwrap();
    assert!(trades.is_empty());
}

#[test]
fn should_clamp_an_account_page_to_requested_length() {
    let mut store = store();
    let alice = UserId::new(1);
    for _ in 0..5 {
        store.append(
            taker_leg(order(0)),
            alice,
            maker_leg(order(1)),
            UserId::new(2),
        );
    }
    let trades = store.trades_after(alice, None, 2).unwrap();
    assert_eq!(trades.len(), 2);
}

#[test]
fn should_persist_a_record_with_no_counterparty_fields() {
    // The record type only carries this order's own view; this test pins the
    // field set so a counterparty field can't be added unnoticed.
    let record = taker_leg(order(0));
    let FillRecord {
        order_id,
        side,
        price,
        quantity,
        notional,
        fee,
        fee_token,
        is_maker,
        timestamp,
    } = record;
    assert_eq!(order_id, order(0));
    assert_eq!(side, Side::Buy);
    assert_eq!(price, Price::new(10_000_000));
    assert_eq!(quantity, Quantity::from_u128(200_000_000));
    assert_eq!(notional, Quantity::from_u128(20_000_000));
    assert_eq!(fee, Quantity::from_u128(200_000));
    assert_eq!(fee_token, PairToken::Base);
    assert!(!is_maker);
    assert_eq!(timestamp, Timestamp::new(42));
}

fn store() -> FillStore<VectorMemory> {
    FillStore::new(
        VectorMemory::default(),
        VectorMemory::default(),
        VectorMemory::default(),
    )
}

fn order(seq: u64) -> OrderId {
    OrderId::new(OrderBookId::ZERO, OrderSeq::new(seq))
}

fn taker_leg(order_id: OrderId) -> FillRecord {
    FillRecord {
        order_id,
        side: Side::Buy,
        price: Price::new(10_000_000),
        quantity: Quantity::from_u128(200_000_000),
        notional: Quantity::from_u128(20_000_000),
        fee: Quantity::from_u128(200_000),
        fee_token: PairToken::Base,
        is_maker: false,
        timestamp: Timestamp::new(42),
    }
}

fn maker_leg(order_id: OrderId) -> FillRecord {
    FillRecord {
        order_id,
        side: Side::Sell,
        price: Price::new(10_000_000),
        quantity: Quantity::from_u128(200_000_000),
        notional: Quantity::from_u128(20_000_000),
        fee: Quantity::from_u128(10_000),
        fee_token: PairToken::Quote,
        is_maker: true,
        timestamp: Timestamp::new(42),
    }
}
