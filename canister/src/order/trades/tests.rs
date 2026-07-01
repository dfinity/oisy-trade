use super::{CursorNotFound, TradeHistory, TradeRecord};
use crate::Timestamp;
use crate::order::{
    BasisPoint, FeeRates, FillEvent, FillId, FillSeq, OrderBookId, OrderId, OrderSeq, PairToken,
    Price, Quantity, Side, TradeId,
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

    append(&mut store, Side::Buy, 0, 1, 0, taker_user(), maker_user());

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

    append(&mut store, Side::Buy, 0, 1, 0, alice, bob);

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

    append(&mut store, Side::Sell, 0, 1, 0, taker_user(), maker_user());

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

/// A `trades_for_order` scenario for order A (taker seq 0): the taker fill
/// sequences appended (each paired with a maker leg on `maker_seq`), the cursor
/// and page length to query, and the expected fill sequences newest-first — or
/// `None` when [`CursorNotFound`] is expected.
struct TradesForOrderCase {
    desc: &'static str,
    inserts: Vec<(u64, u64)>,
    after: Option<u64>,
    length: usize,
    expected: Option<Vec<u64>>,
}

#[test]
fn should_page_one_orders_trades() {
    let cases = vec![
        TradesForOrderCase {
            desc: "newest-first, only order A's trades",
            inserts: vec![(1, 0), (2, 1)],
            after: None,
            length: 10,
            expected: Some(vec![1, 0]),
        },
        TradesForOrderCase {
            desc: "first page clamped by length",
            inserts: vec![(1, 0), (1, 1), (1, 2)],
            after: None,
            length: 2,
            expected: Some(vec![2, 1]),
        },
        TradesForOrderCase {
            desc: "page continues after cursor with next-older",
            inserts: vec![(1, 0), (1, 1), (1, 2)],
            after: Some(1),
            length: 2,
            expected: Some(vec![0]),
        },
        TradesForOrderCase {
            desc: "cursor that is not one of the order's trades is not found",
            inserts: vec![(1, 0)],
            after: Some(99),
            length: 10,
            expected: None,
        },
        TradesForOrderCase {
            desc: "valid cursor with no older trades is an empty page",
            inserts: vec![(1, 0)],
            after: Some(0),
            length: 10,
            expected: Some(vec![]),
        },
    ];

    for case in cases {
        let mut store = store();
        let order_a = OrderId::new(BOOK, OrderSeq::new(0));
        for (maker_seq, fill_seq) in &case.inserts {
            append(&mut store, Side::Buy, 0, *maker_seq, *fill_seq, USER, USER);
        }

        let result = store.trades_for_order(order_a, case.after.map(FillSeq::new), case.length);

        match case.expected {
            None => assert_eq!(
                result,
                Err(CursorNotFound),
                "BUG ({}): expected cursor not found",
                case.desc
            ),
            Some(seqs) => {
                let got: Vec<u64> = result
                    .unwrap_or_else(|_| panic!("BUG ({}): unexpected CursorNotFound", case.desc))
                    .iter()
                    .map(|(s, _)| s.get())
                    .collect();
                assert_eq!(got, seqs, "BUG ({}): page differs from expected", case.desc);
            }
        }
    }
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

fn append(
    store: &mut TradeHistory<VectorMemory>,
    taker_side: Side,
    taker_seq: u64,
    maker_seq: u64,
    fill_seq: u64,
    taker_user: UserId,
    maker_user: UserId,
) {
    let settled = FillEvent {
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
