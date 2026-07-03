use super::{CursorNotFound, TradeHistory, TradeRecord};
use crate::Timestamp;
use crate::order::{
    FillId, FillSeq, OrderBookId, OrderId, OrderSeq, PairToken, Price, Quantity, Side, TradeId,
};
use crate::test_fixtures::arbitrary::{arb_trade_record, check_minicbor_roundtrip};
use crate::test_fixtures::minicbor_encode;
use crate::user::UserId;
use ic_stable_structures::VectorMemory;
use proptest::{prop_assert, proptest};

const USER: UserId = UserId::new(0);
const BOOK: OrderBookId = OrderBookId::ZERO;
const TIMESTAMP: Timestamp = Timestamp::new(42);
const MAX_TRADE_RECORD_BINARY_SIZE: usize = 142;

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
    assert_eq!(
        taker_page[0].1,
        TradeRecord {
            side: Side::Sell,
            price: maker_price(),
            quantity: quantity(),
            notional: notional(),
            fee: taker_fee(),
            fee_token: PairToken::Quote,
            is_maker: false,
            timestamp: TIMESTAMP,
        },
        "sell-taker leg",
    );

    let maker_page = store.trades_for_order(maker_order, None, 10).unwrap();
    assert_eq!(
        maker_page[0].1,
        TradeRecord {
            side: Side::Buy,
            price: maker_price(),
            quantity: quantity(),
            notional: notional(),
            fee: maker_fee(),
            fee_token: PairToken::Base,
            is_maker: true,
            timestamp: TIMESTAMP,
        },
        "buy-maker leg",
    );
}

/// A `trades_for_order` scenario for order A (taker seq 0): the taker fill
/// sequences appended (each paired with a maker leg on `maker_seq`), the cursor
/// and page length to query, and the expected fill sequences newest-first — or
/// `Err(CursorNotFound)` when the cursor is not found. `after` is a full cursor
/// `(order_seq, fill_seq)`.
struct TradesForOrderCase {
    desc: &'static str,
    inserts: Vec<(u64, u64)>,
    after: Option<(u64, u64)>,
    length: usize,
    expected: Result<Vec<u64>, CursorNotFound>,
}

#[test]
fn should_page_one_orders_trades() {
    const ORDER_A: u64 = 0;
    let cases = vec![
        TradesForOrderCase {
            desc: "newest-first, only order A's trades",
            inserts: vec![(1, 0), (2, 1)],
            after: None,
            length: 10,
            expected: Ok(vec![1, 0]),
        },
        TradesForOrderCase {
            desc: "first page clamped by length",
            inserts: vec![(1, 0), (1, 1), (1, 2)],
            after: None,
            length: 2,
            expected: Ok(vec![2, 1]),
        },
        TradesForOrderCase {
            desc: "page continues after cursor with next-older",
            inserts: vec![(1, 0), (1, 1), (1, 2)],
            after: Some((ORDER_A, 1)),
            length: 2,
            expected: Ok(vec![0]),
        },
        TradesForOrderCase {
            desc: "cursor that is not one of the order's trades is not found",
            inserts: vec![(1, 0)],
            after: Some((ORDER_A, 99)),
            length: 10,
            expected: Err(CursorNotFound),
        },
        TradesForOrderCase {
            desc: "valid cursor with no older trades is an empty page",
            inserts: vec![(1, 0)],
            after: Some((ORDER_A, 0)),
            length: 10,
            expected: Ok(vec![]),
        },
    ];

    for case in cases {
        let mut store = store();
        let order_a = OrderId::new(BOOK, OrderSeq::new(ORDER_A));
        for (maker_seq, fill_seq) in &case.inserts {
            append(
                &mut store,
                Side::Buy,
                ORDER_A,
                *maker_seq,
                *fill_seq,
                USER,
                USER,
            );
        }

        let after = case.after.map(|(order_seq, fill_seq)| {
            TradeId::new(
                OrderId::new(BOOK, OrderSeq::new(order_seq)),
                FillSeq::new(fill_seq),
            )
        });
        let got = store
            .trades_for_order(order_a, after, case.length)
            .map(|page| page.iter().map(|(s, _)| s.get()).collect::<Vec<u64>>());

        assert_eq!(
            got, case.expected,
            "BUG ({}): page differs from expected",
            case.desc
        );
    }
}

/// A cursor-ownership scenario over a single match whose taker and maker legs
/// share one `FillSeq` but belong to different orders: the order being paged, the
/// order whose id the `after` cursor embeds, and whether the cursor is accepted.
struct CursorOwnershipCase {
    desc: &'static str,
    queried_order: OrderId,
    cursor_order: OrderId,
    accepted: bool,
}

#[test]
fn should_reject_a_cursor_from_the_other_leg_of_the_same_fill() {
    let taker_order = OrderId::new(BOOK, OrderSeq::new(0));
    let maker_order = OrderId::new(BOOK, OrderSeq::new(1));
    let fill_seq = FillSeq::new(0);

    let cases = vec![
        CursorOwnershipCase {
            desc: "paging the taker order with the maker leg's cursor (shared FillSeq)",
            queried_order: taker_order,
            cursor_order: maker_order,
            accepted: false,
        },
        CursorOwnershipCase {
            desc: "paging the maker order with the taker leg's cursor (shared FillSeq)",
            queried_order: maker_order,
            cursor_order: taker_order,
            accepted: false,
        },
        CursorOwnershipCase {
            desc: "control: the taker order's own cursor pages correctly",
            queried_order: taker_order,
            cursor_order: taker_order,
            accepted: true,
        },
        CursorOwnershipCase {
            desc: "control: the maker order's own cursor pages correctly",
            queried_order: maker_order,
            cursor_order: maker_order,
            accepted: true,
        },
    ];

    for case in cases {
        let mut store = store();
        append(&mut store, Side::Buy, 0, 1, 0, USER, USER);

        let cursor = TradeId::new(case.cursor_order, fill_seq);
        let got = store.trades_for_order(case.queried_order, Some(cursor), 10);

        if case.accepted {
            assert_eq!(
                got,
                Ok(vec![]),
                "BUG ({}): the order's own cursor must page (empty, no older trades)",
                case.desc
            );
        } else {
            assert_eq!(
                got,
                Err(CursorNotFound),
                "BUG ({}): a cursor from the other leg must be rejected",
                case.desc
            );
        }
    }
}

#[test]
fn should_have_correct_size_for_minicbor_encoded_trade_record() {
    let encoded = minicbor_encode(&max_size_trade_record());
    assert_eq!(encoded.len(), MAX_TRADE_RECORD_BINARY_SIZE);
}

#[test]
fn should_round_trip_a_trade_id_back_through_its_public_cursor() {
    let id = TradeId::new(OrderId::new(BOOK, OrderSeq::new(3)), FillSeq::new(7));
    let trade = TradeRecord {
        side: Side::Buy,
        price: maker_price(),
        quantity: quantity(),
        notional: notional(),
        fee: taker_fee(),
        fee_token: PairToken::Base,
        is_maker: false,
        timestamp: TIMESTAMP,
    }
    .into_public(id);
    assert_eq!(
        trade.id.parse::<TradeId>(),
        Ok(id),
        "a Trade.id must decode with the same encoding get_my_trades accepts for `after`"
    );
}

proptest! {
    #[test]
    fn should_encode_decode_minicbor(record in arb_trade_record()) {
        check_minicbor_roundtrip(&record)?;
    }

    #[test]
    fn should_not_exceed_max_trade_record_binary_size(record in arb_trade_record()) {
        let encoded = minicbor_encode(&record);
        prop_assert!(encoded.len() <= MAX_TRADE_RECORD_BINARY_SIZE);
    }
}

/// Stable memory grows one 64 KiB page at a time, and only when the current
/// page can no longer fit another entry — writes land in the already-claimed
/// page until then. This pins, for worst-case fills (both legs a maximum-size
/// `TradeRecord`; the `TradeId`/`ByUserKey` index entries are fixed-width), how
/// many the primary region's first page absorbs before it has to `grow`.
#[test]
fn should_fill_a_stable_memory_page_with_exact_number_of_trades() {
    /// Number of worst-case fills a single 64 KiB page of the primary map absorbs
    /// before it must grow. Each fill writes two maximum-size `TradeRecord` legs.
    const WORST_CASE_FILLS_PER_PAGE: u64 = 152;

    const WASM_PAGE_SIZE: usize = 65_536;

    use std::cell::RefCell;
    use std::rc::Rc;

    // `VectorMemory` is `Rc<RefCell<Vec<u8>>>`: a byte vector standing in for a
    // stable-memory region. The primary map and the per-user index need
    // distinct regions; the large records fill the primary region first.
    let primary: VectorMemory = Rc::new(RefCell::new(Vec::new()));
    let by_user: VectorMemory = Rc::new(RefCell::new(Vec::new()));
    let mut store = TradeHistory::new(primary.clone(), by_user.clone());

    // `TradeHistory::new` claims the first page for the map header.
    let one_page = primary.borrow().len();
    assert_eq!(one_page, WASM_PAGE_SIZE, "the store starts at one page");

    // Write worst-case fills until the primary region has to grow: the loop's
    // last iteration is the "one more" fill that overflows the full page.
    let mut fills = 0u64;
    while primary.borrow().len() == one_page {
        append_max_size_fill(&mut store, fills);
        fills += 1;
    }

    assert_eq!(
        fills - 1,
        WORST_CASE_FILLS_PER_PAGE,
        "worst-case fills the page absorbs before growing",
    );
    assert_eq!(
        primary.borrow().len(),
        2 * one_page,
        "the next fill grew the primary region by exactly one page",
    );
    assert_eq!(
        by_user.borrow().len(),
        one_page,
        "the fixed-width index entries stay within their first page",
    );
}

fn max_size_trade_record() -> TradeRecord {
    TradeRecord {
        side: Side::Buy,
        price: Price::MAX,
        quantity: Quantity::MAX,
        notional: Quantity::MAX,
        fee: Quantity::MAX,
        fee_token: PairToken::Base,
        is_maker: false,
        timestamp: Timestamp::MAX,
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
    let seq = FillSeq::new(fill_seq);
    let maker_side = match taker_side {
        Side::Buy => Side::Sell,
        Side::Sell => Side::Buy,
    };
    let taker_id = TradeId::new(OrderId::new(BOOK, OrderSeq::new(taker_seq)), seq);
    let taker_leg = TradeRecord {
        side: taker_side,
        price: maker_price(),
        quantity: quantity(),
        notional: notional(),
        fee: taker_fee(),
        fee_token: fee_token(taker_side),
        is_maker: false,
        timestamp: TIMESTAMP,
    };
    let maker_id = TradeId::new(OrderId::new(BOOK, OrderSeq::new(maker_seq)), seq);
    let maker_leg = TradeRecord {
        side: maker_side,
        price: maker_price(),
        quantity: quantity(),
        notional: notional(),
        fee: maker_fee(),
        fee_token: fee_token(maker_side),
        is_maker: true,
        timestamp: TIMESTAMP,
    };
    store.append(
        (taker_id, taker_leg),
        taker_user,
        (maker_id, maker_leg),
        maker_user,
    );
}

/// Appends one fill whose two legs are both maximum-size `TradeRecord`s, under
/// distinct ids, so every primary entry is as large as the schema allows.
fn append_max_size_fill(store: &mut TradeHistory<VectorMemory>, i: u64) {
    let fill_seq = FillSeq::new(u64::MAX - i);
    let book = OrderBookId::new(u64::MAX);
    let taker_id = TradeId::new(OrderId::new(book, OrderSeq::new(2 * i)), fill_seq);
    let maker_id = TradeId::new(OrderId::new(book, OrderSeq::new(2 * i + 1)), fill_seq);
    store.append(
        (taker_id, max_size_trade_record()),
        taker_user(),
        (maker_id, max_size_trade_record()),
        maker_user(),
    );
}

fn fee_token(side: Side) -> PairToken {
    match side {
        Side::Buy => PairToken::Base,
        Side::Sell => PairToken::Quote,
    }
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
