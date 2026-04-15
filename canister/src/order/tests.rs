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
        fn should_reject_wrong_length(s in ".{0,31}|.{33,64}") {
            prop_assert_eq!(s.parse::<OrderId>(), Err(OrderIdParseError));
        }

        #[test]
        fn should_reject_non_hex(s in "[^0-9a-fA-F]") {
            prop_assert_eq!(s.parse::<OrderId>(), Err(OrderIdParseError));
        }
    }
}

mod order_book {
    use crate::order::{MatchOrderError, MatchResult, OrderSeq, Price, Quantity};
    use crate::test_fixtures::{LOT_SIZE, TICK_SIZE, buy, fill, order_book, sell};

    mod validation {
        use super::*;
        use crate::test_fixtures::all_order_types;

        #[test]
        fn should_reject_invalid_orders_without_modifying_book() {
            let cases: Vec<(u64, u64, MatchOrderError)> = vec![
                (
                    TICK_SIZE.get() / 2,
                    LOT_SIZE.get(),
                    MatchOrderError::InvalidTickSize {
                        price: Price::new(TICK_SIZE.get() / 2),
                        tick_size: TICK_SIZE,
                    },
                ),
                (
                    0,
                    LOT_SIZE.get(),
                    MatchOrderError::InvalidTickSize {
                        price: Price::ZERO,
                        tick_size: TICK_SIZE,
                    },
                ),
                (
                    TICK_SIZE.get(),
                    LOT_SIZE.get() / 2,
                    MatchOrderError::InvalidLotSize {
                        quantity: Quantity::from(LOT_SIZE.get() / 2),
                        lot_size: LOT_SIZE,
                    },
                ),
                (
                    TICK_SIZE.get(),
                    0,
                    MatchOrderError::InvalidLotSize {
                        quantity: Quantity::ZERO,
                        lot_size: LOT_SIZE,
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
                let result = book.match_order(order).unwrap().0;
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
                (sell(1u64, 110u64, LOT_SIZE), buy(2u64, 100u64, LOT_SIZE)),
                (buy(1u64, 90u64, LOT_SIZE), sell(2u64, 100u64, LOT_SIZE)),
            ];
            for (first_order, resting_order) in orders {
                let mut book = order_book();
                book.match_order(first_order).unwrap();
                let resting_order_seq = resting_order.id();

                let result = book.match_order(resting_order).unwrap().0;
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
                        sell(1u64, 120u64, LOT_SIZE),
                        sell(2u64, 100u64, LOT_SIZE),
                        sell(3u64, 110u64, LOT_SIZE),
                    ],
                    buy(4u64, 120u64, 3 * u64::from(LOT_SIZE)),
                    vec![100, 110, 120],
                ),
                // Bids: inserted out of order, best (highest) matched first
                (
                    vec![
                        buy(1u64, 80u64, LOT_SIZE),
                        buy(2u64, 100u64, LOT_SIZE),
                        buy(3u64, 90u64, LOT_SIZE),
                    ],
                    sell(4u64, 80u64, 3 * u64::from(LOT_SIZE)),
                    vec![100, 90, 80],
                ),
            ];
            for (makers, taker, expected_prices) in cases {
                let mut book = order_book();
                for maker in makers {
                    book.match_order(maker).unwrap();
                }

                let result = book.match_order(taker).unwrap().0;

                let prices: Vec<u64> = result.fills().iter().map(|f| f.maker_price.get()).collect();
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
                        sell(1u64, 100u64, LOT_SIZE),
                        sell(2u64, 100u64, LOT_SIZE),
                        sell(3u64, 100u64, LOT_SIZE),
                    ],
                    buy(4u64, 100u64, LOT_SIZE),
                ),
                // Three bids, then a sell — should match the first bid
                (
                    vec![
                        buy(1u64, 100u64, LOT_SIZE),
                        buy(2u64, 100u64, LOT_SIZE),
                        buy(3u64, 100u64, LOT_SIZE),
                    ],
                    sell(4u64, 100u64, LOT_SIZE),
                ),
            ];
            for (makers, taker) in cases {
                let mut book = order_book();
                let first_maker_id = makers[0].id();
                for maker in makers {
                    book.match_order(maker).unwrap();
                }

                let result = book.match_order(taker).unwrap().0;

                assert_eq!(result.fills()[0].maker_order_seq, first_maker_id);
            }
        }

        #[test]
        fn should_fully_fill_against_equal_opposite() {
            let cases = vec![
                (
                    sell(1u64, 100u64, 2 * u64::from(LOT_SIZE)),
                    buy(2u64, 100u64, 2 * u64::from(LOT_SIZE)),
                ),
                (
                    buy(1u64, 100u64, 2 * u64::from(LOT_SIZE)),
                    sell(2u64, 100u64, 2 * u64::from(LOT_SIZE)),
                ),
            ];
            for (maker, taker) in cases {
                let mut book = order_book();
                let maker_order_seq = maker.id();
                book.match_order(maker).unwrap();

                let result = book.match_order(taker.clone()).unwrap().0;

                assert_eq!(
                    result,
                    MatchResult::Filled {
                        fills: vec![fill(
                            &taker,
                            maker_order_seq,
                            100u64,
                            2 * u64::from(LOT_SIZE)
                        )],
                    }
                );
                assert!(book.is_empty());
            }
        }

        #[test]
        fn should_fill_at_maker_price_when_taker_is_more_aggressive() {
            let cases = vec![
                // Ask at 90, buy at 100 — fills at maker's 90
                (
                    sell(1u64, 90u64, LOT_SIZE),
                    buy(2u64, 100u64, LOT_SIZE),
                    90u64,
                ),
                // Bid at 110, sell at 100 — fills at maker's 110
                (
                    buy(1u64, 110u64, LOT_SIZE),
                    sell(2u64, 100u64, LOT_SIZE),
                    110,
                ),
            ];
            for (maker, taker, expected_price) in cases {
                let mut book = order_book();
                let maker_order_seq = maker.id();
                book.match_order(maker).unwrap();

                let result = book.match_order(taker.clone()).unwrap().0;

                assert_eq!(
                    result,
                    MatchResult::Filled {
                        fills: vec![fill(
                            &taker,
                            maker_order_seq,
                            expected_price,
                            u64::from(LOT_SIZE)
                        )],
                    }
                );
                assert!(book.is_empty());
            }
        }

        #[test]
        fn should_partially_fill_and_rest_remainder() {
            let mut book = order_book();
            book.match_order(sell(1u64, 100u64, LOT_SIZE)).unwrap();

            let taker = buy(2u64, 100u64, 3 * u64::from(LOT_SIZE));
            let result = book.match_order(taker.clone()).unwrap().0;

            assert_eq!(
                result,
                MatchResult::PartiallyFilled {
                    fills: vec![fill(&taker, OrderSeq::ONE, 100u64, u64::from(LOT_SIZE))],
                    resting_order_seq: OrderSeq::new(2),
                }
            );
            let resting = book.best_bid().expect("should have a resting bid");
            assert_eq!(resting.id(), OrderSeq::new(2));
            assert_eq!(
                resting.remaining_quantity(),
                &Quantity::from(2 * u64::from(LOT_SIZE))
            );
        }

        #[test]
        fn should_fill_against_multiple_resting_orders() {
            let cases = vec![
                // Same price level: two asks at 100
                (
                    sell(1u64, 100u64, LOT_SIZE),
                    sell(2u64, 100u64, LOT_SIZE),
                    buy(3u64, 100u64, 2 * u64::from(LOT_SIZE)),
                    100u64,
                    100u64,
                ),
                // Across price levels: asks at 100 and 110
                (
                    sell(1u64, 100u64, LOT_SIZE),
                    sell(2u64, 110u64, LOT_SIZE),
                    buy(3u64, 110u64, 2 * u64::from(LOT_SIZE)),
                    100u64,
                    110u64,
                ),
            ];
            for (maker1, maker2, taker, price_fill_1, price_fill_2) in cases {
                let mut book = order_book();
                let maker1_id = maker1.id();
                let maker2_id = maker2.id();
                book.match_order(maker1).unwrap();
                book.match_order(maker2).unwrap();

                let result = book.match_order(taker.clone()).unwrap().0;

                assert_eq!(
                    result,
                    MatchResult::Filled {
                        fills: vec![
                            fill(&taker, maker1_id, price_fill_1, u64::from(LOT_SIZE)),
                            fill(&taker, maker2_id, price_fill_2, u64::from(LOT_SIZE)),
                        ],
                    }
                );
                assert!(book.is_empty());
            }
        }

        #[test]
        fn should_partially_fill_resting_order() {
            let mut book = order_book();
            book.match_order(sell(1u64, 100u64, 3 * u64::from(LOT_SIZE)))
                .unwrap();
            let taker1 = buy(2u64, 100u64, LOT_SIZE);
            let result = book.match_order(taker1.clone()).unwrap().0;
            assert_eq!(
                result,
                MatchResult::Filled {
                    fills: vec![fill(&taker1, OrderSeq::ONE, 100u64, u64::from(LOT_SIZE))],
                }
            );
            // The remaining 2 lots should still be matchable
            let taker2 = buy(3u64, 100u64, 2 * u64::from(LOT_SIZE));
            let result = book.match_order(taker2.clone()).unwrap().0;
            assert_eq!(
                result,
                MatchResult::Filled {
                    fills: vec![fill(
                        &taker2,
                        OrderSeq::ONE,
                        100u64,
                        2 * u64::from(LOT_SIZE)
                    )],
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
            book.match_order(buy(1u64, 80u64, LOT_SIZE)).unwrap();
            book.match_order(buy(2u64, 100u64, LOT_SIZE)).unwrap();
            book.match_order(buy(3u64, 90u64, LOT_SIZE)).unwrap();
            let best = book.best_bid().unwrap();
            assert_eq!(best.id(), OrderSeq::new(2));
            assert_eq!(best.price(), Price::new(100));
        }

        #[test]
        fn should_return_lowest_ask() {
            let mut book = order_book();
            book.match_order(sell(1u64, 120u64, LOT_SIZE)).unwrap();
            book.match_order(sell(2u64, 100u64, LOT_SIZE)).unwrap();
            book.match_order(sell(3u64, 110u64, LOT_SIZE)).unwrap();
            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderSeq::new(2));
            assert_eq!(best.price(), Price::new(100));
        }

        #[test]
        fn should_return_fifo_first_at_best_price() {
            let mut book = order_book();
            book.match_order(buy(1u64, 100u64, LOT_SIZE)).unwrap();
            book.match_order(buy(2u64, 100u64, 2 * u64::from(LOT_SIZE)))
                .unwrap();
            let best = book.best_bid().unwrap();
            assert_eq!(best.id(), OrderSeq::ONE);
        }

        #[test]
        fn should_update_after_full_fill() {
            let mut book = order_book();
            book.match_order(sell(1u64, 100u64, LOT_SIZE)).unwrap();
            book.match_order(sell(2u64, 110u64, LOT_SIZE)).unwrap();

            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderSeq::ONE);
            assert_eq!(best.price(), Price::new(100));

            // Fill the best ask
            book.match_order(buy(3u64, 100u64, LOT_SIZE)).unwrap();
            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderSeq::new(2));
            assert_eq!(best.price(), Price::new(110));
        }
    }

    mod replay {
        use crate::order::{OrderSeq, Price, Quantity};
        use crate::test_fixtures::{LOT_SIZE, buy, order_book, sell};

        fn tick() -> u64 {
            crate::test_fixtures::TICK_SIZE.get()
        }

        fn lot() -> u64 {
            u64::from(LOT_SIZE)
        }

        #[test]
        fn clear_pending_orders_should_drain_queue() {
            let mut book = order_book();
            book.add_pending_order(buy(0, tick(), lot()));
            book.add_pending_order(sell(1, tick(), lot()));
            assert_eq!(book.pending_orders_len(), 2);

            book.clear_pending_orders();

            assert_eq!(book.pending_orders_len(), 0);
        }

        #[test]
        fn reduce_resting_order_should_partially_reduce() {
            let mut book = order_book();
            book.add_pending_order(buy(0, tick(), 3 * lot()));
            book.process_pending_orders();
            assert_eq!(book.resting_orders_len(), 1);

            book.reduce_resting_order(OrderSeq::ZERO, &Quantity::from(lot()));

            let best = book.best_bid().unwrap();
            assert_eq!(*best.remaining_quantity(), Quantity::from(2 * lot()));
            assert_eq!(book.resting_orders_len(), 1);
        }

        #[test]
        fn reduce_resting_order_should_remove_when_fully_filled() {
            let mut book = order_book();
            book.add_pending_order(buy(0, tick(), lot()));
            book.process_pending_orders();
            assert_eq!(book.resting_orders_len(), 1);

            book.reduce_resting_order(OrderSeq::ZERO, &Quantity::from(lot()));

            assert_eq!(book.resting_orders_len(), 0);
            assert!(book.best_bid().is_none());
            assert_eq!(book.bids_len(), 0);
        }

        #[test]
        fn insert_order_should_add_to_bids() {
            let mut book = order_book();

            book.insert_order(buy(0, tick(), lot()));

            assert_eq!(book.resting_orders_len(), 1);
            let best = book.best_bid().unwrap();
            assert_eq!(best.id(), OrderSeq::ZERO);
            assert_eq!(best.price(), Price::new(tick()));
            assert_eq!(*best.remaining_quantity(), Quantity::from(lot()));
        }

        #[test]
        fn insert_order_should_add_to_asks() {
            let mut book = order_book();

            book.insert_order(sell(0, tick(), lot()));

            assert_eq!(book.resting_orders_len(), 1);
            let best = book.best_ask().unwrap();
            assert_eq!(best.id(), OrderSeq::ZERO);
            assert_eq!(best.price(), Price::new(tick()));
        }

        #[test]
        #[should_panic(expected = "BUG: order not found in resting_orders index")]
        fn reduce_resting_order_should_panic_on_unknown_seq() {
            let mut book = order_book();
            book.reduce_resting_order(OrderSeq::ZERO, &Quantity::from(lot()));
        }

        #[test]
        #[should_panic]
        fn insert_order_should_panic_on_duplicate() {
            let mut book = order_book();
            book.insert_order(buy(0, tick(), lot()));
            book.insert_order(buy(0, tick(), lot()));
        }
    }
}

mod process_pending_orders {
    use crate::order::{Order, OrderSeq};
    use crate::test_fixtures::{LOT_SIZE, order_book};
    use std::collections::BTreeSet;

    fn buy(seq: u64, price: u64, quantity: u64) -> Order {
        crate::test_fixtures::buy(seq, price, quantity)
    }

    fn sell(seq: u64, price: u64, quantity: u64) -> Order {
        crate::test_fixtures::sell(seq, price, quantity)
    }

    #[test]
    fn should_return_empty_output_when_no_pending_orders() {
        let mut book = order_book();
        let output = book.process_pending_orders();

        assert!(output.fills.is_empty());
        assert!(output.resting_orders.is_empty());
        assert!(book.take_filled_orders().is_empty());
    }

    #[test]
    fn should_report_resting_order_when_no_match() {
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        book.add_pending_order(buy(0, 100, lot));

        let output = book.process_pending_orders();

        assert!(output.fills.is_empty());
        assert_eq!(output.resting_orders, BTreeSet::from([OrderSeq::ZERO]));
        assert!(book.take_filled_orders().is_empty());
    }

    #[test]
    fn should_report_filled_orders_after_exact_match() {
        let mut book = order_book();
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
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        // Sell 1 lot (maker), buy 3 lots (taker) -> taker partially fills, rests with 2
        book.add_pending_order(sell(0, 100, lot));
        book.add_pending_order(buy(1, 100, 3 * lot));

        let output = book.process_pending_orders();
        let filled = book.take_filled_orders();

        assert_eq!(output.fills.len(), 1);
        assert!(filled.contains(&OrderSeq::ZERO)); // maker fully filled
        assert!(!filled.contains(&OrderSeq::ONE)); // taker not fully filled
        assert_eq!(output.resting_orders, BTreeSet::from([OrderSeq::ONE])); // taker rests
    }

    #[test]
    fn take_filled_orders_should_drain() {
        let mut book = order_book();
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

mod history {
    use crate::order::{
        OrderBookId, OrderHistory, OrderId, OrderRecord, OrderSeq, Price, Quantity, Side, TokenId,
        TradingPair,
    };
    use candid::Principal;
    use dex_types::OrderStatus;

    fn test_id(seq: u64) -> OrderId {
        OrderId::new(OrderBookId::ZERO, OrderSeq::new(seq))
    }

    fn test_record() -> OrderRecord {
        OrderRecord {
            owner: Principal::anonymous(),
            pair: TradingPair {
                base: TokenId::new(Principal::anonymous()),
                quote: TokenId::new(Principal::anonymous()),
            },
            side: Side::Buy,
            price: Price::new(100),
            quantity: Quantity::from(1_000_000u64),
            status: OrderStatus::Pending,
        }
    }

    #[test]
    fn insert_once_and_get() {
        let mut history = OrderHistory::new();
        let id = test_id(0);
        let record = test_record();
        history.insert_once(id, record.clone());

        assert_eq!(history.get(&id), Some(&record));
    }

    #[test]
    #[should_panic(expected = "duplicate order ID")]
    fn insert_once_panics_on_duplicate() {
        let mut history = OrderHistory::new();
        let id = test_id(0);
        history.insert_once(id, test_record());
        history.insert_once(id, test_record());
    }

    #[test]
    fn get_status_returns_not_found_for_missing() {
        let history = OrderHistory::new();
        assert_eq!(history.get_status(&test_id(42)), OrderStatus::NotFound);
    }

    #[test]
    fn get_status_mut_updates_status() {
        let mut history = OrderHistory::new();
        let id = test_id(0);
        history.insert_once(id, test_record());

        assert_eq!(history.get_status(&id), OrderStatus::Pending);
        *history.get_status_mut(&id).unwrap() = OrderStatus::Filled;
        assert_eq!(history.get_status(&id), OrderStatus::Filled);
    }
}
