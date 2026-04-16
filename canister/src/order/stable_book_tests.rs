use super::stable_book::*;
use crate::order::{
    Fill, LotSize, MatchOrderError, MatchResult, Order, OrderBookId, OrderSeq, PendingOrder, Price,
    Quantity, Side, TickSize,
};
use ic_stable_structures::VectorMemory;
use std::collections::BTreeSet;
use std::num::NonZeroU64;

const TICK_SIZE: TickSize = TickSize::new(NonZeroU64::new(10).unwrap());
const LOT_SIZE: LotSize = LotSize::new(NonZeroU64::new(1_000_000).unwrap());
const TEST_BOOK_ID: OrderBookId = OrderBookId::ZERO;

fn stable_order_book() -> StableOrderBook<VectorMemory> {
    StableOrderBook::new(
        TEST_BOOK_ID,
        TICK_SIZE,
        LOT_SIZE,
        VectorMemory::default(),
        VectorMemory::default(),
        VectorMemory::default(),
    )
}

fn buy(id: u64, price: u64, quantity: u64) -> Order {
    PendingOrder {
        side: Side::Buy,
        price: Price::new(price),
        quantity: Quantity::from(quantity),
    }
    .into_order(OrderSeq::new(id))
}

fn sell(id: u64, price: u64, quantity: u64) -> Order {
    PendingOrder {
        side: Side::Sell,
        price: Price::new(price),
        quantity: Quantity::from(quantity),
    }
    .into_order(OrderSeq::new(id))
}

fn fill(taker: &Order, maker_order_seq: OrderSeq, maker_price: u64, quantity: u64) -> Fill {
    Fill {
        taker_order_seq: taker.id(),
        taker_side: taker.side(),
        taker_price: taker.price(),
        maker_order_seq,
        maker_price: Price::new(maker_price),
        quantity: Quantity::from(quantity),
    }
}

// ---------------------------------------------------------------------------
// Storable round-trip tests
// ---------------------------------------------------------------------------

mod storable_roundtrips {
    use super::*;
    use ic_stable_structures::Storable;

    #[test]
    fn bid_key_ordering() {
        let high_price = BidKey { price: 200, seq: 1 };
        let low_price = BidKey { price: 100, seq: 1 };
        // Higher price sorts first (descending price).
        assert!(high_price < low_price);

        let early = BidKey { price: 100, seq: 1 };
        let late = BidKey { price: 100, seq: 2 };
        // Same price: earlier seq sorts first (FIFO).
        assert!(early < late);
    }

    #[test]
    fn ask_key_ordering() {
        let low_price = AskKey { price: 100, seq: 1 };
        let high_price = AskKey { price: 200, seq: 1 };
        // Lower price sorts first (ascending price).
        assert!(low_price < high_price);

        let early = AskKey { price: 100, seq: 1 };
        let late = AskKey { price: 100, seq: 2 };
        // Same price: earlier seq sorts first (FIFO).
        assert!(early < late);
    }

    #[test]
    fn storable_quantity_roundtrip_zero() {
        let qty = StorableQuantity(Quantity::ZERO);
        let bytes = qty.to_bytes();
        let restored = StorableQuantity::from_bytes(bytes);
        assert_eq!(restored, qty);
    }

    #[test]
    fn storable_quantity_roundtrip_small() {
        let qty = StorableQuantity(Quantity::from(42_000_000u64));
        let bytes = qty.to_bytes();
        let restored = StorableQuantity::from_bytes(bytes);
        assert_eq!(restored, qty);
    }

    #[test]
    fn storable_quantity_roundtrip_large() {
        let qty = StorableQuantity(Quantity::from(u64::MAX));
        let bytes = qty.to_bytes();
        let restored = StorableQuantity::from_bytes(bytes);
        assert_eq!(restored, qty);
    }

    #[test]
    fn storable_side_price_roundtrip() {
        for side in [Side::Buy, Side::Sell] {
            let sp = StorableSidePrice {
                side,
                price: Price::new(123_456_789),
            };
            let bytes = sp.to_bytes();
            let restored = StorableSidePrice::from_bytes(bytes);
            assert_eq!(restored, sp);
        }
    }

    #[test]
    fn bid_key_storable_roundtrip() {
        let key = BidKey {
            price: 999,
            seq: 42,
        };
        let bytes = key.to_bytes();
        let restored = BidKey::from_bytes(bytes);
        assert_eq!(restored, key);
    }

    #[test]
    fn ask_key_storable_roundtrip() {
        let key = AskKey {
            price: 999,
            seq: 42,
        };
        let bytes = key.to_bytes();
        let restored = AskKey::from_bytes(bytes);
        assert_eq!(restored, key);
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

mod validation {
    use super::*;

    #[test]
    fn should_reject_invalid_orders_without_modifying_book() {
        let lot = u64::from(LOT_SIZE);
        let tick = TICK_SIZE.get();
        let cases: Vec<(u64, u64, MatchOrderError)> = vec![
            (
                tick / 2,
                lot,
                MatchOrderError::InvalidTickSize {
                    price: Price::new(tick / 2),
                    tick_size: TICK_SIZE,
                },
            ),
            (
                0,
                lot,
                MatchOrderError::InvalidTickSize {
                    price: Price::ZERO,
                    tick_size: TICK_SIZE,
                },
            ),
            (
                tick,
                lot / 2,
                MatchOrderError::InvalidLotSize {
                    quantity: Quantity::from(lot / 2),
                    lot_size: LOT_SIZE,
                },
            ),
            (
                tick,
                0,
                MatchOrderError::InvalidLotSize {
                    quantity: Quantity::ZERO,
                    lot_size: LOT_SIZE,
                },
            ),
        ];
        for (price, quantity, expected_err) in cases {
            let mut book = stable_order_book();
            for order in [buy(1, price, quantity), sell(2, price, quantity)] {
                assert_eq!(book.match_order(order), Err(expected_err.clone()));
                assert!(
                    book.is_empty(),
                    "Rejected order should not modify the order book"
                );
            }
        }
    }

    #[test]
    fn should_accept_valid_order() {
        let mut book = stable_order_book();
        let lot = u64::from(LOT_SIZE);
        let tick = TICK_SIZE.get();
        for order in [buy(1, tick, lot), sell(2, tick, lot)] {
            let result = book.match_order(order);
            assert!(result.is_ok());
        }
    }
}

// ---------------------------------------------------------------------------
// Resting
// ---------------------------------------------------------------------------

mod resting {
    use super::*;

    #[test]
    fn should_rest_in_empty_book() {
        let lot = u64::from(LOT_SIZE);
        let tick = TICK_SIZE.get();
        for order in [buy(1, tick, lot), sell(2, tick, lot)] {
            let mut book = stable_order_book();
            let order_id = order.id();
            let result = book.match_order(order).unwrap();
            assert_eq!(
                result,
                MatchResult::Resting {
                    resting_order_seq: order_id,
                }
            );
        }
    }

    #[test]
    fn should_rest_buy_when_no_cross() {
        let lot = u64::from(LOT_SIZE);
        let orders = vec![
            (sell(1, 110, lot), buy(2, 100, lot)),
            (buy(1, 90, lot), sell(2, 100, lot)),
        ];
        for (first_order, resting_order) in orders {
            let mut book = stable_order_book();
            book.match_order(first_order).unwrap();
            let resting_order_seq = resting_order.id();

            let result = book.match_order(resting_order).unwrap();
            assert_eq!(result, MatchResult::Resting { resting_order_seq });
        }
    }
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

mod matching {
    use super::*;

    #[test]
    fn should_match_best_price_first() {
        let lot = u64::from(LOT_SIZE);
        let cases = vec![
            (
                vec![sell(1, 120, lot), sell(2, 100, lot), sell(3, 110, lot)],
                buy(4, 120, 3 * lot),
                vec![100, 110, 120],
            ),
            (
                vec![buy(1, 80, lot), buy(2, 100, lot), buy(3, 90, lot)],
                sell(4, 80, 3 * lot),
                vec![100, 90, 80],
            ),
        ];
        for (makers, taker, expected_prices) in cases {
            let mut book = stable_order_book();
            for maker in makers {
                book.match_order(maker).unwrap();
            }

            let result = book.match_order(taker).unwrap();

            let prices: Vec<u64> = result.fills().iter().map(|f| f.maker_price.get()).collect();
            assert_eq!(prices, expected_prices);
            assert!(book.is_empty());
        }
    }

    #[test]
    fn should_match_in_fifo_order_at_same_price() {
        let lot = u64::from(LOT_SIZE);
        let cases = vec![
            (
                vec![sell(1, 100, lot), sell(2, 100, lot), sell(3, 100, lot)],
                buy(4, 100, lot),
            ),
            (
                vec![buy(1, 100, lot), buy(2, 100, lot), buy(3, 100, lot)],
                sell(4, 100, lot),
            ),
        ];
        for (makers, taker) in cases {
            let mut book = stable_order_book();
            let first_maker_id = makers[0].id();
            for maker in makers {
                book.match_order(maker).unwrap();
            }

            let result = book.match_order(taker).unwrap();

            assert_eq!(result.fills()[0].maker_order_seq, first_maker_id);
        }
    }

    #[test]
    fn should_fully_fill_against_equal_opposite() {
        let lot = u64::from(LOT_SIZE);
        let cases = vec![
            (sell(1, 100, 2 * lot), buy(2, 100, 2 * lot)),
            (buy(1, 100, 2 * lot), sell(2, 100, 2 * lot)),
        ];
        for (maker, taker) in cases {
            let mut book = stable_order_book();
            let maker_order_seq = maker.id();
            book.match_order(maker).unwrap();

            let result = book.match_order(taker.clone()).unwrap();

            assert_eq!(
                result,
                MatchResult::Filled {
                    fills: vec![fill(&taker, maker_order_seq, 100, 2 * lot)],
                }
            );
            assert!(book.is_empty());
        }
    }

    #[test]
    fn should_fill_at_maker_price_when_taker_is_more_aggressive() {
        let lot = u64::from(LOT_SIZE);
        let cases = vec![
            (sell(1, 90, lot), buy(2, 100, lot), 90u64),
            (buy(1, 110, lot), sell(2, 100, lot), 110),
        ];
        for (maker, taker, expected_price) in cases {
            let mut book = stable_order_book();
            let maker_order_seq = maker.id();
            book.match_order(maker).unwrap();

            let result = book.match_order(taker.clone()).unwrap();

            assert_eq!(
                result,
                MatchResult::Filled {
                    fills: vec![fill(&taker, maker_order_seq, expected_price, lot)],
                }
            );
            assert!(book.is_empty());
        }
    }

    #[test]
    fn should_partially_fill_and_rest_remainder() {
        let lot = u64::from(LOT_SIZE);
        let mut book = stable_order_book();
        book.match_order(sell(1, 100, lot)).unwrap();

        let taker = buy(2, 100, 3 * lot);
        let result = book.match_order(taker.clone()).unwrap();

        assert_eq!(
            result,
            MatchResult::PartiallyFilled {
                fills: vec![fill(&taker, OrderSeq::ONE, 100, lot)],
                resting_order_seq: OrderSeq::new(2),
            }
        );
        let resting = book.best_bid().expect("should have a resting bid");
        assert_eq!(resting.id(), OrderSeq::new(2));
        assert_eq!(resting.remaining_quantity(), &Quantity::from(2 * lot));
    }

    #[test]
    fn should_fill_against_multiple_resting_orders() {
        let lot = u64::from(LOT_SIZE);
        let cases = vec![
            (
                sell(1, 100, lot),
                sell(2, 100, lot),
                buy(3, 100, 2 * lot),
                100,
                100,
            ),
            (
                sell(1, 100, lot),
                sell(2, 110, lot),
                buy(3, 110, 2 * lot),
                100,
                110,
            ),
        ];
        for (maker1, maker2, taker, price_fill_1, price_fill_2) in cases {
            let mut book = stable_order_book();
            let maker1_id = maker1.id();
            let maker2_id = maker2.id();
            book.match_order(maker1).unwrap();
            book.match_order(maker2).unwrap();

            let result = book.match_order(taker.clone()).unwrap();

            assert_eq!(
                result,
                MatchResult::Filled {
                    fills: vec![
                        fill(&taker, maker1_id, price_fill_1, lot),
                        fill(&taker, maker2_id, price_fill_2, lot),
                    ],
                }
            );
            assert!(book.is_empty());
        }
    }

    #[test]
    fn should_partially_fill_resting_order() {
        let lot = u64::from(LOT_SIZE);
        let mut book = stable_order_book();
        book.match_order(sell(1, 100, 3 * lot)).unwrap();
        let taker1 = buy(2, 100, lot);
        let result = book.match_order(taker1.clone()).unwrap();
        assert_eq!(
            result,
            MatchResult::Filled {
                fills: vec![fill(&taker1, OrderSeq::ONE, 100, lot)],
            }
        );
        let taker2 = buy(3, 100, 2 * lot);
        let result = book.match_order(taker2.clone()).unwrap();
        assert_eq!(
            result,
            MatchResult::Filled {
                fills: vec![fill(&taker2, OrderSeq::ONE, 100, 2 * lot)],
            }
        );
        assert!(book.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Best bid / best ask
// ---------------------------------------------------------------------------

mod best_bid_best_ask {
    use super::*;

    #[test]
    fn should_return_none_on_empty_book() {
        let book = stable_order_book();
        assert!(book.best_bid().is_none());
        assert!(book.best_ask().is_none());
    }

    #[test]
    fn should_return_highest_bid() {
        let lot = u64::from(LOT_SIZE);
        let mut book = stable_order_book();
        book.match_order(buy(1, 80, lot)).unwrap();
        book.match_order(buy(2, 100, lot)).unwrap();
        book.match_order(buy(3, 90, lot)).unwrap();
        let best = book.best_bid().unwrap();
        assert_eq!(best.id(), OrderSeq::new(2));
        assert_eq!(best.price(), Price::new(100));
    }

    #[test]
    fn should_return_lowest_ask() {
        let lot = u64::from(LOT_SIZE);
        let mut book = stable_order_book();
        book.match_order(sell(1, 120, lot)).unwrap();
        book.match_order(sell(2, 100, lot)).unwrap();
        book.match_order(sell(3, 110, lot)).unwrap();
        let best = book.best_ask().unwrap();
        assert_eq!(best.id(), OrderSeq::new(2));
        assert_eq!(best.price(), Price::new(100));
    }

    #[test]
    fn should_return_fifo_first_at_best_price() {
        let lot = u64::from(LOT_SIZE);
        let mut book = stable_order_book();
        book.match_order(buy(1, 100, lot)).unwrap();
        book.match_order(buy(2, 100, 2 * lot)).unwrap();
        let best = book.best_bid().unwrap();
        assert_eq!(best.id(), OrderSeq::ONE);
    }

    #[test]
    fn should_update_after_full_fill() {
        let lot = u64::from(LOT_SIZE);
        let mut book = stable_order_book();
        book.match_order(sell(1, 100, lot)).unwrap();
        book.match_order(sell(2, 110, lot)).unwrap();

        let best = book.best_ask().unwrap();
        assert_eq!(best.id(), OrderSeq::ONE);
        assert_eq!(best.price(), Price::new(100));

        book.match_order(buy(3, 100, lot)).unwrap();
        let best = book.best_ask().unwrap();
        assert_eq!(best.id(), OrderSeq::new(2));
        assert_eq!(best.price(), Price::new(110));
    }
}

// ---------------------------------------------------------------------------
// process_pending_orders
// ---------------------------------------------------------------------------

mod process_pending_orders {
    use super::*;

    #[test]
    fn should_return_empty_output_when_no_pending_orders() {
        let mut book = stable_order_book();
        let output = book.process_pending_orders();

        assert!(output.fills.is_empty());
        assert!(output.resting_orders.is_empty());
        assert!(book.take_filled_orders().is_empty());
    }

    #[test]
    fn should_report_resting_order_when_no_match() {
        let mut book = stable_order_book();
        let lot = u64::from(LOT_SIZE);
        book.add_pending_order(buy(0, 100, lot));

        let output = book.process_pending_orders();

        assert!(output.fills.is_empty());
        assert_eq!(output.resting_orders, BTreeSet::from([OrderSeq::ZERO]));
        assert!(book.take_filled_orders().is_empty());
    }

    #[test]
    fn should_report_filled_orders_after_exact_match() {
        let mut book = stable_order_book();
        let lot = u64::from(LOT_SIZE);
        book.add_pending_order(sell(0, 100, lot));
        book.add_pending_order(buy(1, 100, lot));

        let output = book.process_pending_orders();
        let filled = book.take_filled_orders();

        assert_eq!(output.fills.len(), 1);
        assert!(filled.contains(&OrderSeq::ZERO)); // maker
        assert!(filled.contains(&OrderSeq::ONE)); // taker
        assert!(output.resting_orders.is_empty());
    }

    #[test]
    fn should_report_partial_fill_with_resting_remainder() {
        let mut book = stable_order_book();
        let lot = u64::from(LOT_SIZE);
        book.add_pending_order(sell(0, 100, lot));
        book.add_pending_order(buy(1, 100, 3 * lot));

        let output = book.process_pending_orders();
        let filled = book.take_filled_orders();

        assert_eq!(output.fills.len(), 1);
        assert!(filled.contains(&OrderSeq::ZERO)); // maker fully filled
        assert!(!filled.contains(&OrderSeq::ONE)); // taker not fully filled
        assert_eq!(output.resting_orders, BTreeSet::from([OrderSeq::ONE]));
    }

    #[test]
    fn take_filled_orders_should_drain() {
        let mut book = stable_order_book();
        let lot = u64::from(LOT_SIZE);
        book.add_pending_order(sell(0, 100, lot));
        book.add_pending_order(buy(1, 100, lot));
        book.process_pending_orders();

        let first_call = book.take_filled_orders();
        let second_call = book.take_filled_orders();

        assert_eq!(first_call.len(), 2);
        assert!(second_call.is_empty());
    }
}
