use super::{CursorNotFound, FillRecord, FillSeq, FillStore};
use crate::Timestamp;
use crate::order::{OrderBookId, OrderId, OrderSeq, PairToken, Price, Quantity, Side};
use ic_stable_structures::VectorMemory;

#[test]
fn should_append_two_side_projected_records_and_advance_seq_by_two() {
    let mut store = store();
    let order_a = order(0);
    let order_b = order(1);

    store.append(taker_leg(order_a), maker_leg(order_b));

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
    store.append(taker_leg(order_a), maker_leg(order_b));
    store.append(taker_leg(order_a), maker_leg(order_b));

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
        store.append(taker_leg(order_a), maker_leg(other));
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
    store.append(taker_leg(order(0)), maker_leg(order(1)));
    let fills = store.fills_after(order(7), None, 10).unwrap();
    assert!(fills.is_empty());
}

#[test]
fn should_reject_a_cursor_that_is_not_one_of_the_orders_fills() {
    let mut store = store();
    let order_a = order(0);
    store.append(taker_leg(order_a), maker_leg(order(1)));
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
    store.append(taker_leg(order_a), maker_leg(order(1)));
    // The only fill of order_a is at seq 0; nothing older.
    let fills = store.fills_after(order_a, Some(FillSeq::ZERO), 10).unwrap();
    assert!(fills.is_empty());
}

#[test]
fn should_clamp_to_requested_length() {
    let mut store = store();
    let order_a = order(0);
    for _ in 0..5 {
        store.append(taker_leg(order_a), maker_leg(order(1)));
    }
    let fills = store.fills_after(order_a, None, 2).unwrap();
    assert_eq!(fills.len(), 2);
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

#[test]
fn should_round_trip_a_trade_cursor_back_into_a_fill_seq() {
    let seq = FillSeq::new(7);
    let trade = taker_leg(order(0)).into_trade(seq);
    assert_eq!(
        trade.cursor.parse::<FillSeq>(),
        Ok(seq),
        "a Trade.cursor must decode with the same encoding get_my_trades accepts for `after`"
    );
}

fn store() -> FillStore<VectorMemory> {
    FillStore::new(VectorMemory::default(), VectorMemory::default())
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
