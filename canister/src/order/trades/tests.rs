use super::{CursorNotFound, TradeHistory, TradeRecord};
use crate::Timestamp;
use crate::order::{
    BasisPoint, FeeRates, FillId, FillSeq, OrderBookId, OrderId, OrderSeq, PairToken, Price,
    Quantity, SettledFill, Side, TradeId,
};
use crate::test_fixtures::arbitrary::{arb_trade_record, check_minicbor_roundtrip};
use crate::user::UserId;
use ic_stable_structures::VectorMemory;
use proptest::proptest;
use std::num::NonZeroU64;

const USER: UserId = UserId::new(0);
const BOOK: OrderBookId = OrderBookId::ZERO;
const TIMESTAMP: Timestamp = Timestamp::new(42);

#[test]
fn should_derive_fill_id_from_trade_id_dropping_the_order_seq() {
    let book = OrderBookId::new(7);
    let taker = TradeId::new(OrderId::new(book, OrderSeq::new(2)), FillSeq::new(9));
    let maker = TradeId::new(OrderId::new(book, OrderSeq::new(5)), FillSeq::new(9));
    assert_eq!(taker.fill_id(), maker.fill_id(), "two legs share a FillId");
    assert_eq!(taker.fill_id(), FillId::new(book, FillSeq::new(9)));
}

#[test]
fn should_project_and_append_both_legs_of_a_settlement() {
    let mut store = store();
    let taker_order = OrderId::new(BOOK, OrderSeq::new(0));
    let maker_order = OrderId::new(BOOK, OrderSeq::new(1));

    append_buy_taker(&mut store, 0, 1, 0, taker_user(), maker_user());

    assert_eq!(store.len(), 2);

    let taker_trades = store.trades_for_order(taker_order, None, 10).unwrap();
    assert_eq!(taker_trades.len(), 1);
    assert_eq!(taker_trades[0].0, FillSeq::ZERO);
    assert_eq!(
        taker_trades[0].1,
        TradeRecord {
            side: Side::Buy,
            price: maker_price(),
            quantity: quantity(),
            notional: notional(),
            fee: taker_fee(),
            fee_token: PairToken::Base,
            is_maker: false,
            timestamp: TIMESTAMP,
        },
        "taker leg projected from the settlement",
    );

    let maker_trades = store.trades_for_order(maker_order, None, 10).unwrap();
    assert_eq!(maker_trades.len(), 1);
    assert_eq!(maker_trades[0].0, FillSeq::ZERO);
    assert_eq!(
        maker_trades[0].1,
        TradeRecord {
            side: Side::Sell,
            price: maker_price(),
            quantity: quantity(),
            notional: notional(),
            fee: maker_fee(),
            fee_token: PairToken::Quote,
            is_maker: true,
            timestamp: TIMESTAMP,
        },
        "maker leg projected from the settlement",
    );
}

#[test]
fn should_index_each_leg_under_its_own_owner() {
    let mut store = store();
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let taker_order = OrderId::new(BOOK, OrderSeq::new(0));
    let maker_order = OrderId::new(BOOK, OrderSeq::new(1));

    append_buy_taker(&mut store, 0, 1, 0, alice, bob);

    let alice_trades = store.trades_after(alice, None, 10).unwrap();
    assert_eq!(
        alice_trades.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![TradeId::new(taker_order, FillSeq::ZERO)],
        "alice owns the taker leg",
    );
    assert!(!alice_trades[0].1.is_maker, "taker leg is not a maker");

    let bob_trades = store.trades_after(bob, None, 10).unwrap();
    assert_eq!(
        bob_trades.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![TradeId::new(maker_order, FillSeq::ZERO)],
        "bob owns the maker leg",
    );
    assert!(bob_trades[0].1.is_maker, "maker leg is a maker");
}

#[test]
fn should_swap_sides_for_a_sell_taker() {
    let mut store = store();
    let taker_order = OrderId::new(BOOK, OrderSeq::new(0));
    let maker_order = OrderId::new(BOOK, OrderSeq::new(1));

    append_sell_taker(&mut store, 0, 1, 0, taker_user(), maker_user());

    let taker_page = store.trades_for_order(taker_order, None, 10).unwrap();
    let taker = &taker_page[0].1;
    assert_eq!(taker.side, Side::Sell);
    assert_eq!(taker.fee_token, PairToken::Quote);
    assert!(!taker.is_maker);

    let maker_page = store.trades_for_order(maker_order, None, 10).unwrap();
    let maker = &maker_page[0].1;
    assert_eq!(maker.side, Side::Buy);
    assert_eq!(maker.fee_token, PairToken::Base);
    assert!(maker.is_maker);
}

#[test]
fn should_return_one_orders_trades_newest_first_excluding_other_orders() {
    let mut store = store();
    let order_a = OrderId::new(BOOK, OrderSeq::new(0));
    append_buy_taker(&mut store, 0, 1, 0, USER, USER);
    append_buy_taker(&mut store, 0, 2, 1, USER, USER);

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
    let order_a = OrderId::new(BOOK, OrderSeq::new(0));
    for seq in 0..3 {
        append_buy_taker(&mut store, 0, 1, seq, USER, USER);
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
    append_buy_taker(&mut store, 0, 1, 0, USER, USER);
    let unknown = OrderId::new(BOOK, OrderSeq::new(7));
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
    let order_a = OrderId::new(BOOK, OrderSeq::new(0));
    append_buy_taker(&mut store, 0, 1, 0, USER, USER);
    assert_eq!(
        store.trades_for_order(order_a, Some(FillSeq::new(99)), 10),
        Err(CursorNotFound)
    );
}

#[test]
fn should_return_empty_page_for_a_valid_cursor_with_no_older_trades() {
    let mut store = store();
    let order_a = OrderId::new(BOOK, OrderSeq::new(0));
    append_buy_taker(&mut store, 0, 1, 0, USER, USER);
    let trades = store
        .trades_for_order(order_a, Some(FillSeq::ZERO), 10)
        .unwrap();
    assert!(trades.is_empty());
}

#[test]
fn should_clamp_one_orders_page_to_requested_length() {
    let mut store = store();
    let order_a = OrderId::new(BOOK, OrderSeq::new(0));
    for seq in 0..5 {
        append_buy_taker(&mut store, 0, 1, seq, USER, USER);
    }
    assert_eq!(store.trades_for_order(order_a, None, 2).unwrap().len(), 2);
}

#[test]
fn should_return_a_users_trades_across_orders_newest_first_scoped_to_owner() {
    let mut store = store();
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    let alice_a = OrderId::new(BOOK, OrderSeq::new(0));
    let alice_b = OrderId::new(BOOK, OrderSeq::new(1));
    let bob_order = OrderId::new(BOOK, OrderSeq::new(2));

    append_buy_taker(&mut store, 0, 2, 0, alice, bob);
    append_buy_taker(&mut store, 1, 2, 1, alice, bob);

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
    for seq in 0..3 {
        append_buy_taker(&mut store, 0, 1, seq, alice, other);
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
    append_buy_taker(&mut store, 0, 1, 0, UserId::new(1), UserId::new(2));
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
    append_buy_taker(&mut store, 0, 1, 0, alice, bob);
    let bob_id = TradeId::new(OrderId::new(BOOK, OrderSeq::new(1)), FillSeq::ZERO);
    assert_eq!(
        store.trades_after(alice, Some(bob_id), 10),
        Err(CursorNotFound)
    );
    let unknown = TradeId::new(OrderId::new(BOOK, OrderSeq::new(0)), FillSeq::new(99));
    assert_eq!(
        store.trades_after(alice, Some(unknown), 10),
        Err(CursorNotFound)
    );
}

#[test]
fn should_return_an_empty_account_page_for_a_valid_cursor_with_no_older_trades() {
    let mut store = store();
    let alice = UserId::new(1);
    append_buy_taker(&mut store, 0, 1, 0, alice, UserId::new(2));
    let alice_id = TradeId::new(OrderId::new(BOOK, OrderSeq::new(0)), FillSeq::ZERO);
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
    for seq in 0..5 {
        append_buy_taker(&mut store, 0, 1, seq, alice, UserId::new(2));
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

fn append_buy_taker(
    store: &mut TradeHistory<VectorMemory>,
    taker_seq: u64,
    maker_seq: u64,
    fill_seq: u64,
    taker_user: UserId,
    maker_user: UserId,
) {
    append(
        store,
        Side::Buy,
        taker_seq,
        maker_seq,
        fill_seq,
        taker_user,
        maker_user,
    );
}

fn append_sell_taker(
    store: &mut TradeHistory<VectorMemory>,
    taker_seq: u64,
    maker_seq: u64,
    fill_seq: u64,
    taker_user: UserId,
    maker_user: UserId,
) {
    append(
        store,
        Side::Sell,
        taker_seq,
        maker_seq,
        fill_seq,
        taker_user,
        maker_user,
    );
}

fn append(
    store: &mut TradeHistory<VectorMemory>,
    taker_side: Side,
    taker_seq: u64,
    maker_seq: u64,
    fill_seq: u64,
    taker_user: UserId,
    maker_user: UserId,
) {
    let settled = SettledFill {
        fill_seq: FillSeq::new(fill_seq),
        taker_order_seq: OrderSeq::new(taker_seq),
        maker_order_seq: OrderSeq::new(maker_seq),
        quantity: quantity(),
        fee_rates: fee_rates(),
    };
    let [taker_leg, maker_leg] =
        settled.trade_legs(BOOK, taker_side, maker_price(), base_scale(), TIMESTAMP);
    store.append(taker_leg, taker_user, maker_leg, maker_user);
}

fn fee_rates() -> FeeRates {
    FeeRates {
        maker: BasisPoint::new(5).unwrap(),
        taker: BasisPoint::new(10).unwrap(),
    }
}

fn base_scale() -> NonZeroU64 {
    NonZeroU64::new(100_000_000).unwrap()
}

fn maker_price() -> Price {
    Price::new(10_000_000)
}

fn quantity() -> Quantity {
    Quantity::from_u128(200_000_000)
}

fn notional() -> Quantity {
    Quantity::from_u128(20_000_000)
}

fn taker_fee() -> Quantity {
    Quantity::from_u128(200_000)
}

fn maker_fee() -> Quantity {
    Quantity::from_u128(10_000)
}

fn taker_user() -> UserId {
    UserId::new(1)
}

fn maker_user() -> UserId {
    UserId::new(2)
}
