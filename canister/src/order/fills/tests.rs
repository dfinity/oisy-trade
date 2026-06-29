use super::{CursorNotFound, Trade, TradeHistory};
use crate::Timestamp;
use crate::ids::ParseFixedWithIdError;
use crate::order::{
    FillId, FillSeq, OrderBookId, OrderId, OrderSeq, PairToken, Price, Quantity, Side, TradeId,
};
use crate::user::UserId;
use ic_stable_structures::VectorMemory;

const USER: UserId = UserId::new(0);

#[test]
fn should_roundtrip_fill_id_through_display_and_parse() {
    let id = FillId::new(OrderBookId::new(3), FillSeq::new(42));
    let parsed: FillId = id.to_string().parse().unwrap();
    assert_eq!(parsed, id);
    assert_eq!(id.to_string().len(), 32);
    assert!(id.to_string().chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn should_reject_a_malformed_fill_id() {
    assert_eq!("".parse::<FillId>(), Err(ParseFixedWithIdError {}));
    assert_eq!(
        "0".repeat(31).parse::<FillId>(),
        Err(ParseFixedWithIdError {}),
        "too short"
    );
    assert_eq!(
        "0".repeat(33).parse::<FillId>(),
        Err(ParseFixedWithIdError {}),
        "too long"
    );
    assert_eq!(
        "z".repeat(32).parse::<FillId>(),
        Err(ParseFixedWithIdError {}),
        "non-hex"
    );
}

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

    store.append((taker, taker_leg()), USER, (maker, maker_leg()), USER);

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
    store.append(
        (TradeId::new(order_a, FillSeq::new(0)), taker_leg()),
        USER,
        (trade_id(1, 0), maker_leg()),
        USER,
    );
    store.append(
        (TradeId::new(order_a, FillSeq::new(1)), taker_leg()),
        USER,
        (trade_id(1, 1), maker_leg()),
        USER,
    );

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
        store.append(
            (TradeId::new(order_a, FillSeq::new(seq)), taker_leg()),
            USER,
            (trade_id(1, seq), maker_leg()),
            USER,
        );
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
    store.append(
        (trade_id(0, 0), taker_leg()),
        USER,
        (trade_id(1, 0), maker_leg()),
        USER,
    );
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
    store.append(
        (TradeId::new(order_a, FillSeq::ZERO), taker_leg()),
        USER,
        (trade_id(1, 0), maker_leg()),
        USER,
    );
    assert_eq!(
        store.trades_for_order(order_a, Some(FillSeq::new(99)), 10),
        Err(CursorNotFound)
    );
}

#[test]
fn should_return_empty_page_for_a_valid_cursor_with_no_older_trades() {
    let mut store = store();
    let order_a = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    store.append(
        (TradeId::new(order_a, FillSeq::ZERO), taker_leg()),
        USER,
        (trade_id(1, 0), maker_leg()),
        USER,
    );
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
        store.append(
            (TradeId::new(order_a, FillSeq::new(seq)), taker_leg()),
            USER,
            (trade_id(1, seq), maker_leg()),
            USER,
        );
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

    store.append(
        (TradeId::new(alice_a, FillSeq::new(0)), taker_leg()),
        alice,
        (TradeId::new(bob_order, FillSeq::new(0)), maker_leg()),
        bob,
    );
    store.append(
        (TradeId::new(alice_b, FillSeq::new(1)), taker_leg()),
        alice,
        (TradeId::new(bob_order, FillSeq::new(1)), maker_leg()),
        bob,
    );

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
        store.append(
            (TradeId::new(alice_order, FillSeq::new(seq)), taker_leg()),
            alice,
            (trade_id(1, seq), maker_leg()),
            other,
        );
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
    store.append(
        (trade_id(0, 0), taker_leg()),
        UserId::new(1),
        (trade_id(1, 0), maker_leg()),
        UserId::new(2),
    );
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
    store.append(
        (trade_id(0, 0), taker_leg()),
        alice,
        (bob_id, maker_leg()),
        bob,
    );
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
    store.append(
        (alice_id, taker_leg()),
        alice,
        (trade_id(1, 0), maker_leg()),
        UserId::new(2),
    );
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
        store.append(
            (TradeId::new(alice_order, FillSeq::new(seq)), taker_leg()),
            alice,
            (trade_id(1, seq), maker_leg()),
            UserId::new(2),
        );
    }
    assert_eq!(store.trades_after(alice, None, 2).unwrap().len(), 2);
}

#[test]
fn should_persist_a_record_with_no_counterparty_fields() {
    let Trade {
        side,
        price,
        quantity,
        notional,
        fee,
        fee_token,
        is_maker,
        timestamp,
    } = taker_leg();
    assert_eq!(side, Side::Buy);
    assert_eq!(price, Price::new(10_000_000));
    assert_eq!(quantity, Quantity::from_u128(200_000_000));
    assert_eq!(notional, Quantity::from_u128(20_000_000));
    assert_eq!(fee, Quantity::from_u128(200_000));
    assert_eq!(fee_token, PairToken::Base);
    assert!(!is_maker);
    assert_eq!(timestamp, Timestamp::new(42));
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

fn taker_leg() -> Trade {
    Trade {
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

fn maker_leg() -> Trade {
    Trade {
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
