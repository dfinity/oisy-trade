use super::{CursorNotFound, TradeHistory, TradeRecord};
use crate::Timestamp;
use crate::order::{
    FillId, FillSeq, OrderBookId, OrderId, OrderSeq, PairToken, Price, Quantity, Side, TradeId,
};
use crate::test_fixtures::arbitrary::{arb_trade_record, check_minicbor_roundtrip};
use crate::user::UserId;
use ic_stable_structures::VectorMemory;
use proptest::proptest;

const USER: UserId = UserId::new(0);

#[test]
fn should_derive_fill_id_from_trade_id_dropping_the_order_seq() {
    let book = OrderBookId::new(7);
    let taker = TradeId::new(OrderId::new(book, OrderSeq::new(2)), FillSeq::new(9));
    let maker = TradeId::new(OrderId::new(book, OrderSeq::new(5)), FillSeq::new(9));
    assert_eq!(taker.fill_id(), maker.fill_id(), "two legs share a FillId");
    assert_eq!(taker.fill_id(), FillId::new(book, FillSeq::new(9)));
}

#[test]
fn should_append_two_side_projected_records_keyed_by_trade_id() {
    let mut store = store();
    let taker = trade_id(0, 0);
    let maker = trade_id(1, 0);

    store.insert((taker, taker_leg()), USER);
    store.insert((maker, maker_leg()), USER);

    assert_eq!(store.len(), 2);

    let taker_trades = store.trades_for_order(taker.order_id(), None, 10).unwrap();
    assert_eq!(taker_trades.len(), 1);
    assert_eq!(taker_trades[0].0, FillSeq::ZERO);
    assert_eq!(taker_trades[0].1, taker_leg());

    let maker_trades = store.trades_for_order(maker.order_id(), None, 10).unwrap();
    assert_eq!(maker_trades.len(), 1);
    assert_eq!(maker_trades[0].0, FillSeq::ZERO);
    assert_eq!(maker_trades[0].1, maker_leg());
}

#[test]
fn should_return_one_orders_trades_newest_first_excluding_other_orders() {
    let mut store = store();
    let order_a = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    // Two matches against order A (fill_seqs 0, 1) each paired with order B.
    store.insert((TradeId::new(order_a, FillSeq::new(0)), taker_leg()), USER);
    store.insert((trade_id(1, 0), maker_leg()), USER);
    store.insert((TradeId::new(order_a, FillSeq::new(1)), taker_leg()), USER);
    store.insert((trade_id(1, 1), maker_leg()), USER);

    let seqs: Vec<u64> = store
        .trades_for_order(order_a, None, 10)
        .unwrap()
        .iter()
        .map(|(s, _)| s.get())
        .collect();
    assert_eq!(seqs, vec![1, 0], "newest-first, only order A's trades");
}

#[test]
fn should_page_one_orders_trades_via_after_cursor() {
    let mut store = store();
    let order_a = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    for seq in 0..3 {
        store.insert((TradeId::new(order_a, FillSeq::new(seq)), taker_leg()), USER);
        store.insert((trade_id(1, seq), maker_leg()), USER);
    }
    let first = store.trades_for_order(order_a, None, 2).unwrap();
    assert_eq!(
        first.iter().map(|(s, _)| s.get()).collect::<Vec<_>>(),
        vec![2, 1]
    );
    let cursor = first.last().unwrap().0;
    let second = store.trades_for_order(order_a, Some(cursor), 2).unwrap();
    assert_eq!(
        second.iter().map(|(s, _)| s.get()).collect::<Vec<_>>(),
        vec![0]
    );
}

#[test]
fn should_return_empty_page_for_unknown_order() {
    let mut store = store();
    store.insert((trade_id(0, 0), taker_leg()), USER);
    store.insert((trade_id(1, 0), maker_leg()), USER);
    let unknown = OrderId::new(OrderBookId::ZERO, OrderSeq::new(7));
    assert!(
        store
            .trades_for_order(unknown, None, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn should_reject_a_cursor_that_is_not_one_of_the_orders_trades() {
    let mut store = store();
    let order_a = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    store.insert((TradeId::new(order_a, FillSeq::ZERO), taker_leg()), USER);
    store.insert((trade_id(1, 0), maker_leg()), USER);
    assert_eq!(
        store.trades_for_order(order_a, Some(FillSeq::new(99)), 10),
        Err(CursorNotFound)
    );
}

#[test]
fn should_return_empty_page_for_a_valid_cursor_with_no_older_trades() {
    let mut store = store();
    let order_a = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    store.insert((TradeId::new(order_a, FillSeq::ZERO), taker_leg()), USER);
    store.insert((trade_id(1, 0), maker_leg()), USER);
    let trades = store
        .trades_for_order(order_a, Some(FillSeq::ZERO), 10)
        .unwrap();
    assert!(trades.is_empty());
}

#[test]
fn should_clamp_one_orders_page_to_requested_length() {
    let mut store = store();
    let order_a = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    for seq in 0..5 {
        store.insert((TradeId::new(order_a, FillSeq::new(seq)), taker_leg()), USER);
        store.insert((trade_id(1, seq), maker_leg()), USER);
    }
    assert_eq!(store.trades_for_order(order_a, None, 2).unwrap().len(), 2);
}

#[test]
fn should_return_a_users_trades_across_orders_newest_first_scoped_to_owner() {
    let mut store = store();
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let alice_a = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    let alice_b = OrderId::new(OrderBookId::ZERO, OrderSeq::new(1));
    let bob_order = OrderId::new(OrderBookId::ZERO, OrderSeq::new(2));

    store.insert((TradeId::new(alice_a, FillSeq::new(0)), taker_leg()), alice);
    store.insert((TradeId::new(bob_order, FillSeq::new(0)), maker_leg()), bob);
    store.insert((TradeId::new(alice_b, FillSeq::new(1)), taker_leg()), alice);
    store.insert((TradeId::new(bob_order, FillSeq::new(1)), maker_leg()), bob);

    let alice_orders: Vec<OrderId> = store
        .trades_after(alice, None, 10)
        .unwrap()
        .iter()
        .map(|(id, _)| id.order_id())
        .collect();
    assert_eq!(
        alice_orders,
        vec![alice_b, alice_a],
        "alice's trades across both orders, newest-first",
    );

    let bob_orders: Vec<OrderId> = store
        .trades_after(bob, None, 10)
        .unwrap()
        .iter()
        .map(|(id, _)| id.order_id())
        .collect();
    assert_eq!(
        bob_orders,
        vec![bob_order, bob_order],
        "bob sees only his own legs"
    );
}

#[test]
fn should_page_a_users_trades_via_after_cursor() {
    let mut store = store();
    let alice = UserId::new(1);
    let other = UserId::new(2);
    let alice_order = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    for seq in 0..3 {
        store.insert(
            (TradeId::new(alice_order, FillSeq::new(seq)), taker_leg()),
            alice,
        );
        store.insert((trade_id(1, seq), maker_leg()), other);
    }
    let first = store.trades_after(alice, None, 2).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|(id, _)| id.seq().get())
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    let cursor = first.last().unwrap().0;
    let second = store.trades_after(alice, Some(cursor), 2).unwrap();
    assert_eq!(
        second
            .iter()
            .map(|(id, _)| id.seq().get())
            .collect::<Vec<_>>(),
        vec![0]
    );
    let last_cursor = second.last().unwrap().0;
    assert!(
        store
            .trades_after(alice, Some(last_cursor), 2)
            .unwrap()
            .is_empty(),
        "paging past the oldest trade yields an empty page"
    );
}

#[test]
fn should_return_an_empty_account_page_for_an_unknown_user() {
    let mut store = store();
    store.insert((trade_id(0, 0), taker_leg()), UserId::new(1));
    store.insert((trade_id(1, 0), maker_leg()), UserId::new(2));
    assert!(
        store
            .trades_after(UserId::new(7), None, 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn should_reject_an_account_cursor_that_is_not_one_of_the_users_trades() {
    let mut store = store();
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let bob_id = trade_id(1, 0);
    store.insert((trade_id(0, 0), taker_leg()), alice);
    store.insert((bob_id, maker_leg()), bob);
    assert_eq!(
        store.trades_after(alice, Some(bob_id), 10),
        Err(CursorNotFound)
    );
    let unknown = trade_id(0, 99);
    assert_eq!(
        store.trades_after(alice, Some(unknown), 10),
        Err(CursorNotFound)
    );
}

#[test]
fn should_return_an_empty_account_page_for_a_valid_cursor_with_no_older_trades() {
    let mut store = store();
    let alice = UserId::new(1);
    let alice_id = trade_id(0, 0);
    store.insert((alice_id, taker_leg()), alice);
    store.insert((trade_id(1, 0), maker_leg()), UserId::new(2));
    assert!(
        store
            .trades_after(alice, Some(alice_id), 10)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn should_clamp_an_account_page_to_requested_length() {
    let mut store = store();
    let alice = UserId::new(1);
    let alice_order = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    for seq in 0..5 {
        store.insert(
            (TradeId::new(alice_order, FillSeq::new(seq)), taker_leg()),
            alice,
        );
        store.insert((trade_id(1, seq), maker_leg()), UserId::new(2));
    }
    assert_eq!(store.trades_after(alice, None, 2).unwrap().len(), 2);
}

proptest! {
    #[test]
    fn should_encode_decode_minicbor(record in arb_trade_record()) {
        check_minicbor_roundtrip(&record)?;
    }
}

fn store() -> TradeHistory<VectorMemory> {
    TradeHistory::new(VectorMemory::default(), VectorMemory::default())
}

fn trade_id(order_seq: u64, fill_seq: u64) -> TradeId {
    TradeId::new(
        OrderId::new(OrderBookId::ZERO, OrderSeq::new(order_seq)),
        FillSeq::new(fill_seq),
    )
}

fn taker_leg() -> TradeRecord {
    TradeRecord {
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

fn maker_leg() -> TradeRecord {
    TradeRecord {
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
