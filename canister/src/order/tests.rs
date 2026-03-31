mod order_book {
    use crate::order::{Fill, MatchOrderError, MatchResult, OrderBook, OrderId, Price, Quantity};
    use crate::test_fixtures::{LOT_SIZE, TICK_SIZE, buy, order_book, sell};

    mod validation {
        use super::*;
        use crate::test_fixtures::all_order_types;

        #[test]
        #[should_panic(expected = "tick_size must be non-zero")]
        fn should_panic_on_zero_tick_size() {
            OrderBook::new(Price::ZERO, Quantity::new(LOT_SIZE));
        }

        #[test]
        #[should_panic(expected = "lot_size must be non-zero")]
        fn should_panic_on_zero_lot_size() {
            OrderBook::new(Price::new(TICK_SIZE), Quantity::ZERO);
        }

        #[test]
        fn should_reject_price_not_multiple_of_tick_size() {
            let mut book = order_book();
            let invalid_price = TICK_SIZE / 2;

            for order in all_order_types(invalid_price, LOT_SIZE) {
                let result = book.match_order(order);

                assert_eq!(
                    result,
                    Err(MatchOrderError::InvalidTickSize {
                        price: Price::from(invalid_price),
                        tick_size: Price::new(TICK_SIZE),
                    })
                );
            }
        }

        #[test]
        fn should_reject_quantity_not_multiple_of_lot_size() {
            let mut book = order_book();
            let invalid_lot_size = LOT_SIZE / 2;

            for order in all_order_types(TICK_SIZE, invalid_lot_size) {
                let result = book.match_order(order);

                assert_eq!(
                    result,
                    Err(MatchOrderError::InvalidLotSize {
                        quantity: Quantity::new(invalid_lot_size),
                        lot_size: Quantity::new(LOT_SIZE),
                    })
                );
            }
        }

        #[test]
        fn should_reject_zero_price() {
            let mut book = order_book();
            for order in all_order_types(0, LOT_SIZE) {
                assert_eq!(
                    book.match_order(order),
                    Err(MatchOrderError::InvalidTickSize {
                        price: Price::ZERO,
                        tick_size: Price::new(TICK_SIZE),
                    })
                );
            }
        }

        #[test]
        fn should_reject_zero_quantity() {
            let mut book = order_book();
            for order in all_order_types(TICK_SIZE, 0) {
                assert_eq!(
                    book.match_order(order),
                    Err(MatchOrderError::InvalidLotSize {
                        quantity: Quantity::ZERO,
                        lot_size: Quantity::new(LOT_SIZE),
                    })
                );
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
                        resting_order_id: order_id,
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
                let resting_order_id = resting_order.id();

                let result = book.match_order(resting_order).unwrap();
                assert_eq!(result, MatchResult::Resting { resting_order_id });
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

                assert_eq!(result.fills()[0].maker_order_id, first_maker_id);
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
                let maker_order_id = maker.id();
                book.match_order(maker).unwrap();

                let result = book.match_order(taker).unwrap();

                assert_eq!(
                    result,
                    MatchResult::Filled {
                        fills: vec![Fill {
                            maker_order_id,
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
                let maker_order_id = maker.id();
                book.match_order(maker).unwrap();

                let result = book.match_order(taker).unwrap();

                assert_eq!(
                    result,
                    MatchResult::Filled {
                        fills: vec![Fill {
                            maker_order_id,
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
                        maker_order_id: OrderId::from(1),
                        price: Price::new(100),
                        quantity: Quantity::new(LOT_SIZE),
                    }],
                    resting_order_id: OrderId::from(2),
                }
            );
            let resting = book.best_bid().expect("should have a resting bid");
            assert_eq!(resting.id(), OrderId::from(2));
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
                                maker_order_id: maker1_id,
                                price: Price::new(price_fill_1),
                                quantity: Quantity::new(LOT_SIZE),
                            },
                            Fill {
                                maker_order_id: maker2_id,
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
                        maker_order_id: OrderId::from(1),
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
                        maker_order_id: OrderId::from(1),
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
            assert_eq!(best.id(), OrderId::from(2));
            assert_eq!(best.price(), Price::new(100));
        }

        #[test]
        fn should_return_lowest_ask() {
            let mut book = order_book();
            book.match_order(sell(1, 120, LOT_SIZE)).unwrap();
            book.match_order(sell(2, 100, LOT_SIZE)).unwrap();
            book.match_order(sell(3, 110, LOT_SIZE)).unwrap();
            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderId::from(2));
            assert_eq!(best.price(), Price::new(100));
        }

        #[test]
        fn should_return_fifo_first_at_best_price() {
            let mut book = order_book();
            book.match_order(buy(1, 100, LOT_SIZE)).unwrap();
            book.match_order(buy(2, 100, 2 * LOT_SIZE)).unwrap();
            let best = book.best_bid().unwrap();
            assert_eq!(best.id(), OrderId::from(1));
        }

        #[test]
        fn should_update_after_full_fill() {
            let mut book = order_book();
            book.match_order(sell(1, 100, LOT_SIZE)).unwrap();
            book.match_order(sell(2, 110, LOT_SIZE)).unwrap();

            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderId::from(1));
            assert_eq!(best.price(), Price::new(100));

            // Fill the best ask
            book.match_order(buy(3, 100, LOT_SIZE)).unwrap();
            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderId::from(2));
            assert_eq!(best.price(), Price::new(110));
        }
    }

    mod get_order {
        use super::*;

        #[test]
        fn should_return_none_for_empty_book() {
            let book = order_book();
            assert_eq!(book.get_order(OrderId::from(1)), None);
        }

        #[test]
        fn should_find_resting_order() {
            let mut book = order_book();
            book.match_order(buy(1, 100, LOT_SIZE)).unwrap();

            let order = book.get_order(OrderId::from(1)).unwrap();

            assert_eq!(order.id(), OrderId::from(1));
            assert_eq!(order.price(), Price::new(100));
            assert_eq!(order.remaining_quantity(), Quantity::new(LOT_SIZE));
        }

        #[test]
        fn should_return_none_for_fully_filled_order() {
            let mut book = order_book();
            book.match_order(sell(1, 100, LOT_SIZE)).unwrap();
            book.match_order(buy(2, 100, LOT_SIZE)).unwrap();

            assert_eq!(book.get_order(OrderId::from(1)), None);
            assert_eq!(book.get_order(OrderId::from(2)), None);
        }

        #[test]
        fn should_reflect_partial_fill_on_resting_order() {
            let mut book = order_book();
            book.match_order(sell(1, 100, 3 * LOT_SIZE)).unwrap();
            book.match_order(buy(2, 100, LOT_SIZE)).unwrap();

            let order = book.get_order(OrderId::from(1)).unwrap();
            assert_eq!(order.remaining_quantity(), Quantity::new(2 * LOT_SIZE));
            assert_eq!(book.get_order(OrderId::from(2)), None);
        }

        #[test]
        fn should_return_none_for_nonexistent_id() {
            let mut book = order_book();
            book.match_order(buy(1, 100, LOT_SIZE)).unwrap();

            assert!(book.get_order(OrderId::from(999)).is_none());
        }
    }
}
