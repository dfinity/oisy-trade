mod order_id {
    use crate::order::{OrderBookId, OrderId, OrderIdParseError, OrderSeq};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn should_roundtrip_through_display_and_parse(book_id: u64, seq: u64) {
            let id = OrderId::new(OrderBookId::new(book_id), OrderSeq::new(seq));
            let parsed: OrderId = id.to_string().parse().unwrap();
            prop_assert_eq!(parsed, id);
        }

        #[test]
        fn should_always_encode_as_32_char_hex(book_id: u64, seq: u64) {
            let id = OrderId::new(OrderBookId::new(book_id), OrderSeq::new(seq));
            let s = id.to_string();
            prop_assert_eq!(s.len(), 32);
            prop_assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
        }

        #[test]
        fn should_reject_wrong_length(s in "[0-9a-f]{0,31}|[0-9a-f]{33,64}") {
            prop_assert_eq!(s.parse::<OrderId>(), Err(OrderIdParseError));
        }

        #[test]
        fn should_reject_non_hex(s in "[g-z]{32}") {
            prop_assert_eq!(s.parse::<OrderId>(), Err(OrderIdParseError));
        }
    }
}

mod order_book {
    use crate::order::{Fill, MatchOrderError, MatchResult, OrderBook, OrderSeq, Price, Quantity};
    use crate::test_fixtures::{LOT_SIZE, TEST_BOOK_ID, TICK_SIZE, buy, order_book, sell};

    mod validation {
        use super::*;
        use crate::test_fixtures::all_order_types;

        #[test]
        #[should_panic(expected = "tick_size must be non-zero")]
        fn should_panic_on_zero_tick_size() {
            OrderBook::new(TEST_BOOK_ID, Price::ZERO, Quantity::new(LOT_SIZE));
        }

        #[test]
        #[should_panic(expected = "lot_size must be non-zero")]
        fn should_panic_on_zero_lot_size() {
            OrderBook::new(TEST_BOOK_ID, Price::new(TICK_SIZE), Quantity::ZERO);
        }

        #[test]
        fn should_reject_invalid_orders_without_modifying_book() {
            let cases: Vec<(u64, u64, MatchOrderError)> = vec![
                (
                    TICK_SIZE / 2,
                    LOT_SIZE,
                    MatchOrderError::InvalidTickSize {
                        price: Price::new(TICK_SIZE / 2),
                        tick_size: Price::new(TICK_SIZE),
                    },
                ),
                (
                    0,
                    LOT_SIZE,
                    MatchOrderError::InvalidTickSize {
                        price: Price::ZERO,
                        tick_size: Price::new(TICK_SIZE),
                    },
                ),
                (
                    TICK_SIZE,
                    LOT_SIZE / 2,
                    MatchOrderError::InvalidLotSize {
                        quantity: Quantity::new(LOT_SIZE / 2),
                        lot_size: Quantity::new(LOT_SIZE),
                    },
                ),
                (
                    TICK_SIZE,
                    0,
                    MatchOrderError::InvalidLotSize {
                        quantity: Quantity::ZERO,
                        lot_size: Quantity::new(LOT_SIZE),
                    },
                ),
            ];
            for (price, quantity, expected_err) in cases {
                let mut book = order_book();
                let expected_book = book.clone();
                for order in all_order_types(price, quantity) {
                    assert_eq!(book.match_order(order), Err(expected_err.clone()));
                    assert_eq!(
                        book, expected_book,
                        "Rejected order should not modify the order book"
                    );
                }
            }
        }

        #[test]
        fn should_accept_valid_order() {
            let mut book = order_book();
            for order in all_order_types(TICK_SIZE, LOT_SIZE) {
                let result = book.match_order(order);
                assert!(result.is_ok());
            }
        }
    }

    mod resting {
        use super::*;
        use crate::test_fixtures::all_order_types;

        #[test]
        fn should_rest_in_empty_book() {
            for order in all_order_types(TICK_SIZE, LOT_SIZE) {
                let mut book = order_book();
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
            let orders = vec![
                (sell(1, 110, LOT_SIZE), buy(2, 100, LOT_SIZE)),
                (buy(1, 90, LOT_SIZE), sell(2, 100, LOT_SIZE)),
            ];
            for (first_order, resting_order) in orders {
                let mut book = order_book();
                book.match_order(first_order).unwrap();
                let resting_order_seq = resting_order.id();

                let result = book.match_order(resting_order).unwrap();
                assert_eq!(result, MatchResult::Resting { resting_order_seq });
            }
        }
    }

    mod matching {
        use super::*;

        #[test]
        fn should_match_best_price_first() {
            let cases = vec![
                // Asks: inserted out of order, best (lowest) matched first
                (
                    vec![
                        sell(1, 120, LOT_SIZE),
                        sell(2, 100, LOT_SIZE),
                        sell(3, 110, LOT_SIZE),
                    ],
                    buy(4, 120, 3 * LOT_SIZE),
                    vec![100, 110, 120],
                ),
                // Bids: inserted out of order, best (highest) matched first
                (
                    vec![
                        buy(1, 80, LOT_SIZE),
                        buy(2, 100, LOT_SIZE),
                        buy(3, 90, LOT_SIZE),
                    ],
                    sell(4, 80, 3 * LOT_SIZE),
                    vec![100, 90, 80],
                ),
            ];
            for (makers, taker, expected_prices) in cases {
                let mut book = order_book();
                for maker in makers {
                    book.match_order(maker).unwrap();
                }

                let result = book.match_order(taker).unwrap();

                let prices: Vec<u64> = result.fills().iter().map(|f| f.price.get()).collect();
                assert_eq!(prices, expected_prices);
                assert!(book.is_empty());
            }
        }

        #[test]
        fn should_match_in_fifo_order_at_same_price() {
            let cases = vec![
                // Three asks, then a buy — should match the first ask
                (
                    vec![
                        sell(1, 100, LOT_SIZE),
                        sell(2, 100, LOT_SIZE),
                        sell(3, 100, LOT_SIZE),
                    ],
                    buy(4, 100, LOT_SIZE),
                ),
                // Three bids, then a sell — should match the first bid
                (
                    vec![
                        buy(1, 100, LOT_SIZE),
                        buy(2, 100, LOT_SIZE),
                        buy(3, 100, LOT_SIZE),
                    ],
                    sell(4, 100, LOT_SIZE),
                ),
            ];
            for (makers, taker) in cases {
                let mut book = order_book();
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
            let cases = vec![
                (sell(1, 100, 2 * LOT_SIZE), buy(2, 100, 2 * LOT_SIZE)),
                (buy(1, 100, 2 * LOT_SIZE), sell(2, 100, 2 * LOT_SIZE)),
            ];
            for (maker, taker) in cases {
                let mut book = order_book();
                let maker_order_seq = maker.id();
                book.match_order(maker).unwrap();

                let result = book.match_order(taker).unwrap();

                assert_eq!(
                    result,
                    MatchResult::Filled {
                        fills: vec![Fill {
                            maker_order_seq,
                            price: Price::new(100),
                            quantity: Quantity::new(2 * LOT_SIZE),
                        }],
                    }
                );
                assert!(book.is_empty());
            }
        }

        #[test]
        fn should_fill_at_maker_price_when_taker_is_more_aggressive() {
            let cases = vec![
                // Ask at 90, buy at 100 — fills at maker's 90
                (sell(1, 90, LOT_SIZE), buy(2, 100, LOT_SIZE), 90),
                // Bid at 110, sell at 100 — fills at maker's 110
                (buy(1, 110, LOT_SIZE), sell(2, 100, LOT_SIZE), 110),
            ];
            for (maker, taker, expected_price) in cases {
                let mut book = order_book();
                let maker_order_seq = maker.id();
                book.match_order(maker).unwrap();

                let result = book.match_order(taker).unwrap();

                assert_eq!(
                    result,
                    MatchResult::Filled {
                        fills: vec![Fill {
                            maker_order_seq,
                            price: Price::new(expected_price),
                            quantity: Quantity::new(LOT_SIZE),
                        }],
                    }
                );
                assert!(book.is_empty());
            }
        }

        #[test]
        fn should_partially_fill_and_rest_remainder() {
            let mut book = order_book();
            book.match_order(sell(1, 100, LOT_SIZE)).unwrap();

            let result = book.match_order(buy(2, 100, 3 * LOT_SIZE)).unwrap();

            assert_eq!(
                result,
                MatchResult::PartiallyFilled {
                    fills: vec![Fill {
                        maker_order_seq: OrderSeq::new(1),
                        price: Price::new(100),
                        quantity: Quantity::new(LOT_SIZE),
                    }],
                    resting_order_seq: OrderSeq::new(2),
                }
            );
            let resting = book.best_bid().expect("should have a resting bid");
            assert_eq!(resting.id(), OrderSeq::new(2));
            assert_eq!(resting.remaining_quantity(), Quantity::new(2 * LOT_SIZE));
        }

        #[test]
        fn should_fill_against_multiple_resting_orders() {
            let cases = vec![
                // Same price level: two asks at 100
                (
                    sell(1, 100, LOT_SIZE),
                    sell(2, 100, LOT_SIZE),
                    buy(3, 100, 2 * LOT_SIZE),
                    100,
                    100,
                ),
                // Across price levels: asks at 100 and 110
                (
                    sell(1, 100, LOT_SIZE),
                    sell(2, 110, LOT_SIZE),
                    buy(3, 110, 2 * LOT_SIZE),
                    100,
                    110,
                ),
            ];
            for (maker1, maker2, taker, price_fill_1, price_fill_2) in cases {
                let mut book = order_book();
                let maker1_id = maker1.id();
                let maker2_id = maker2.id();
                book.match_order(maker1).unwrap();
                book.match_order(maker2).unwrap();

                let result = book.match_order(taker).unwrap();

                assert_eq!(
                    result,
                    MatchResult::Filled {
                        fills: vec![
                            Fill {
                                maker_order_seq: maker1_id,
                                price: Price::new(price_fill_1),
                                quantity: Quantity::new(LOT_SIZE),
                            },
                            Fill {
                                maker_order_seq: maker2_id,
                                price: Price::new(price_fill_2),
                                quantity: Quantity::new(LOT_SIZE),
                            },
                        ],
                    }
                );
                assert!(book.is_empty());
            }
        }

        #[test]
        fn should_partially_fill_resting_order() {
            let mut book = order_book();
            book.match_order(sell(1, 100, 3 * LOT_SIZE)).unwrap();
            let result = book.match_order(buy(2, 100, LOT_SIZE)).unwrap();
            assert_eq!(
                result,
                MatchResult::Filled {
                    fills: vec![Fill {
                        maker_order_seq: OrderSeq::new(1),
                        price: Price::new(100),
                        quantity: Quantity::new(LOT_SIZE),
                    }],
                }
            );
            // The remaining 2 lots should still be matchable
            let result = book.match_order(buy(3, 100, 2 * LOT_SIZE)).unwrap();
            assert_eq!(
                result,
                MatchResult::Filled {
                    fills: vec![Fill {
                        maker_order_seq: OrderSeq::new(1),
                        price: Price::new(100),
                        quantity: Quantity::new(2 * LOT_SIZE),
                    }],
                }
            );
            assert!(book.is_empty());
        }
    }

    mod best_bid_best_ask {
        use super::*;

        #[test]
        fn should_return_none_on_empty_book() {
            let book = order_book();
            assert!(book.best_bid().is_none());
            assert!(book.best_ask().is_none());
        }

        #[test]
        fn should_return_highest_bid() {
            let mut book = order_book();
            book.match_order(buy(1, 80, LOT_SIZE)).unwrap();
            book.match_order(buy(2, 100, LOT_SIZE)).unwrap();
            book.match_order(buy(3, 90, LOT_SIZE)).unwrap();
            let best = book.best_bid().unwrap();
            assert_eq!(best.id(), OrderSeq::new(2));
            assert_eq!(best.price(), Price::new(100));
        }

        #[test]
        fn should_return_lowest_ask() {
            let mut book = order_book();
            book.match_order(sell(1, 120, LOT_SIZE)).unwrap();
            book.match_order(sell(2, 100, LOT_SIZE)).unwrap();
            book.match_order(sell(3, 110, LOT_SIZE)).unwrap();
            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderSeq::new(2));
            assert_eq!(best.price(), Price::new(100));
        }

        #[test]
        fn should_return_fifo_first_at_best_price() {
            let mut book = order_book();
            book.match_order(buy(1, 100, LOT_SIZE)).unwrap();
            book.match_order(buy(2, 100, 2 * LOT_SIZE)).unwrap();
            let best = book.best_bid().unwrap();
            assert_eq!(best.id(), OrderSeq::new(1));
        }

        #[test]
        fn should_update_after_full_fill() {
            let mut book = order_book();
            book.match_order(sell(1, 100, LOT_SIZE)).unwrap();
            book.match_order(sell(2, 110, LOT_SIZE)).unwrap();

            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderSeq::new(1));
            assert_eq!(best.price(), Price::new(100));

            // Fill the best ask
            book.match_order(buy(3, 100, LOT_SIZE)).unwrap();
            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderSeq::new(2));
            assert_eq!(best.price(), Price::new(110));
        }
    }
}
