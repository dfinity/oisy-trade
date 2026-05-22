mod order_id {
    use crate::order::{OrderBookId, OrderId, OrderIdParseError, OrderSeq};
    use crate::test_fixtures::arbitrary::arb_order_id;
    use ic_stable_structures::Storable;
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

        #[test]
        fn should_roundtrip_order_id_through_stable_bytes(
            id in arb_order_id(),
        ) {
            let bytes = id.to_bytes();
            let decoded = OrderId::from_bytes(bytes);
            prop_assert_eq!(decoded, id);
        }

        /// Big-endian encoding makes the stable-memory key order match the
        /// lexicographic byte order of `(book_id, seq)`, so `StableBTreeMap`
        /// iteration traverses orders in the same order as a heap `BTreeMap`
        /// keyed by `OrderId`.
        #[test]
        fn should_preserve_order_under_be_encoding(
            a in any::<(u64, u64)>(),
            b in any::<(u64, u64)>(),
        ) {
            let id_a = OrderId::new(OrderBookId::new(a.0), OrderSeq::new(a.1));
            let id_b = OrderId::new(OrderBookId::new(b.0), OrderSeq::new(b.1));
            let bytes_a = id_a.to_bytes();
            let bytes_b = id_b.to_bytes();
            prop_assert_eq!(id_a.cmp(&id_b), bytes_a.cmp(&bytes_b));
        }
    }
}

mod quantity {
    use crate::order::{LotSize, Quantity};
    use candid::Nat;
    use proptest::prelude::*;
    use std::num::NonZeroU64;

    // CBOR unsigned integer: 1 byte (0–23), 2 bytes (24–255), ..., 9 bytes (u64::MAX)
    const MAX_U64_CBOR_SIZE: usize = 9;
    // CBOR PosBignum: Tag(2) [1] + bytes(≤16) [1 + ≤16] = ≤18 bytes
    const MAX_U128_CBOR_SIZE: usize = 18;
    // CBOR PosBignum: Tag(2) [1] + bytes(32) [2 + 32, since len > 23] = 35 bytes
    const MAX_U256_CBOR_SIZE: usize = 35;

    #[test]
    fn should_reach_exact_max_size() {
        let max_u64 = Quantity::from(u64::MAX);
        assert_eq!(encode(&max_u64).len(), MAX_U64_CBOR_SIZE);

        let max_u128 = Quantity::from_u128(u128::MAX);
        assert_eq!(encode(&max_u128).len(), MAX_U128_CBOR_SIZE);

        let max_u256 = Quantity::MAX;
        assert_eq!(encode(&max_u256).len(), MAX_U256_CBOR_SIZE);
    }

    #[test]
    fn should_checked_sub_with_carry() {
        // 2^128
        let a = Quantity::new(1, 0);
        // 1
        let b = Quantity::new(0, 1);
        assert_eq!(a.checked_sub(&b), Some(Quantity::new(0, u128::MAX)));

        assert_eq!(a.checked_sub(&a), Some(Quantity::ZERO));

        // 2^128 + 1
        let c = Quantity::new(1, 1);
        assert_eq!(a.checked_sub(&c), None);

        // other.high == u128::MAX with borrow: high + borrow would overflow u128
        // without checked_add.
        let d = Quantity::new(u128::MAX, 0);
        let e = Quantity::new(u128::MAX, 1);
        assert_eq!(d.checked_sub(&e), None);
    }

    proptest! {
        #[test]
        fn checked_add_commutative(a in arb_quantity(), b in arb_quantity()) {
            prop_assert_eq!(a.checked_add(b), b.checked_add(a));
        }

        #[test]
        fn checked_add_identity(a in arb_quantity()) {
            prop_assert_eq!(a.checked_add(Quantity::ZERO), Some(a));
        }

        #[test]
        fn checked_add_then_sub_roundtrip(a in arb_quantity(), b in arb_quantity()) {
            if let Some(sum) = a.checked_add(b) {
                prop_assert_eq!(sum.checked_sub(&b), Some(a));
                prop_assert_eq!(sum.checked_sub(&a), Some(b));
            }
        }

        #[test]
        fn checked_sub_self_is_zero(a in arb_quantity()) {
            prop_assert_eq!(a.checked_sub(&a), Some(Quantity::ZERO));
        }

        #[test]
        fn checked_sub_returns_none_on_underflow(
            a in arb_small_quantity(),
            b in arb_small_quantity(),
        ) {
            if a < b {
                prop_assert_eq!(a.checked_sub(&b), None);
            }
        }

        #[test]
        fn checked_mul_u64_identity(a in arb_quantity()) {
            prop_assert_eq!(a.checked_mul_u64(1), Some(a));
        }

        #[test]
        fn checked_mul_u64_zero(a in arb_quantity()) {
            prop_assert_eq!(a.checked_mul_u64(0), Some(Quantity::ZERO));
        }

        #[test]
        fn checked_mul_u64_distributes_over_add(
            a in arb_small_quantity(),
            b in arb_small_quantity(),
            c in 1..1000u64,
        ) {
            // With small quantities and c < 1000, no overflow is possible.
            // (a + b) * c == a * c + b * c
            let left = a.checked_add(b).and_then(|s| s.checked_mul_u64(c));
            let right = a.checked_mul_u64(c)
                .and_then(|ac| b.checked_mul_u64(c).and_then(|bc| ac.checked_add(bc)));
            prop_assert_eq!(left, right);
        }

        #[test]
        fn nat_roundtrip(a in arb_quantity()) {
            let nat: Nat = a.into();
            let back = Quantity::try_from(nat).unwrap();
            prop_assert_eq!(a, back);
        }

        #[test]
        fn nat_exceeding_u256_max_fails(offset in 1..u64::MAX) {
            let max_nat: Nat = Quantity::MAX.into();
            let too_large = max_nat + Nat::from(offset);
            prop_assert!(Quantity::try_from(too_large).is_err());
        }

        #[test]
        fn from_u64_roundtrip(v in any::<u64>()) {
            let q = Quantity::from(v);
            let nat: Nat = q.into();
            prop_assert_eq!(nat, Nat::from(v));
        }

        #[test]
        fn ordering_consistent_with_nat(a in arb_small_quantity(), b in arb_small_quantity()) {
            let nat_a: Nat = a.into();
            let nat_b: Nat = b.into();
            prop_assert_eq!(a.cmp(&b), nat_a.cmp(&nat_b));
        }

        #[test]
        fn is_multiple_of_consistent(
            base in 1..u128::MAX,
            lot in 1..10_000u64,
        ) {
            let lot_size = LotSize::new(NonZeroU64::new(lot).unwrap());
            let q = Quantity::from_u128(base);

            prop_assert_eq!(base.is_multiple_of(lot as u128), q.is_multiple_of(lot_size));

            let q = Quantity::from_u128(base).checked_mul_u64(lot).unwrap();
            prop_assert!(q.is_multiple_of(lot_size));
        }

        #[test]
        fn is_zero_iff_default(high in any::<u128>(), low in any::<u128>()) {
            let q = Quantity::new(high, low);
            prop_assert_eq!(q.is_zero(), high == 0 && low == 0);
        }

        #[test]
        fn cbor_roundtrip(a in arb_quantity()) {
            let mut buf = Vec::new();
            minicbor::encode(a, &mut buf).unwrap();
            let decoded: Quantity = minicbor::decode(&buf).unwrap();
            prop_assert_eq!(a, decoded);
        }

        #[test]
        fn cbor_u64_quantity_is_compact(a in arb_u64_quantity()) {
            let encoded = encode(&a);
            prop_assert!(
                encoded.len() <= MAX_U64_CBOR_SIZE,
                "u64 quantity encoded as {} bytes, max expected {}",
                encoded.len(),
                MAX_U64_CBOR_SIZE,
            );
        }

        #[test]
        fn cbor_u128_quantity_is_compact(a in arb_u128_quantity()) {
            let encoded = encode(&a);
            prop_assert!(
                encoded.len() <= MAX_U128_CBOR_SIZE,
                "u128 quantity encoded as {} bytes, max expected {}",
                encoded.len(),
                MAX_U128_CBOR_SIZE,
            );
        }

        #[test]
        fn cbor_any_quantity_fits_max(a in arb_quantity()) {
            let encoded = encode(&a);
            prop_assert!(
                encoded.len() <= MAX_U256_CBOR_SIZE,
                "quantity encoded as {} bytes, max expected {}",
                encoded.len(),
                MAX_U256_CBOR_SIZE,
            );
        }
    }

    fn arb_quantity() -> impl Strategy<Value = Quantity> {
        (any::<u128>(), any::<u128>()).prop_map(|(high, low)| Quantity::new(high, low))
    }

    fn arb_u64_quantity() -> impl Strategy<Value = Quantity> {
        any::<u64>().prop_map(Quantity::from)
    }

    // Alias for tests that don't care about the specific range.
    fn arb_small_quantity() -> impl Strategy<Value = Quantity> {
        arb_u64_quantity()
    }

    fn arb_u128_quantity() -> impl Strategy<Value = Quantity> {
        any::<u128>().prop_map(Quantity::from_u128)
    }

    fn encode(quantity: &Quantity) -> Vec<u8> {
        let mut buf = Vec::new();
        minicbor::encode(quantity, &mut buf).unwrap();
        buf
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
                (sell(1u64, 110u64, LOT_SIZE), buy(2u64, 100u64, LOT_SIZE)),
                (buy(1u64, 90u64, LOT_SIZE), sell(2u64, 100u64, LOT_SIZE)),
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

                let result = book.match_order(taker).unwrap();

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

                let result = book.match_order(taker).unwrap();

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

                let result = book.match_order(taker.clone()).unwrap();

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

                let result = book.match_order(taker.clone()).unwrap();

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
            let result = book.match_order(taker.clone()).unwrap();

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

                let result = book.match_order(taker.clone()).unwrap();

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
            let result = book.match_order(taker1.clone()).unwrap();
            assert_eq!(
                result,
                MatchResult::Filled {
                    fills: vec![fill(&taker1, OrderSeq::ONE, 100u64, u64::from(LOT_SIZE))],
                }
            );
            // The remaining 2 lots should still be matchable
            let taker2 = buy(3u64, 100u64, 2 * u64::from(LOT_SIZE));
            let result = book.match_order(taker2.clone()).unwrap();
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

    mod levels {
        use super::*;

        fn lot(n: u64) -> u64 {
            n * u64::from(LOT_SIZE)
        }

        #[test]
        fn should_return_empty_iterators_on_empty_book() {
            let book = order_book();
            assert_eq!(book.bid_levels(10).count(), 0);
            assert_eq!(book.ask_levels(10).count(), 0);
        }

        #[test]
        fn should_aggregate_single_order_per_level() {
            let mut book = order_book();
            book.match_order(buy(1u64, 100u64, lot(1))).unwrap();
            book.match_order(buy(2u64, 90u64, lot(2))).unwrap();
            book.match_order(sell(3u64, 110u64, lot(3))).unwrap();
            book.match_order(sell(4u64, 120u64, lot(4))).unwrap();

            let bids: Vec<_> = book.bid_levels(10).collect();
            assert_eq!(
                bids,
                vec![
                    (Price::new(100), Quantity::from(lot(1))),
                    (Price::new(90), Quantity::from(lot(2))),
                ]
            );
            let asks: Vec<_> = book.ask_levels(10).collect();
            assert_eq!(
                asks,
                vec![
                    (Price::new(110), Quantity::from(lot(3))),
                    (Price::new(120), Quantity::from(lot(4))),
                ]
            );
        }

        #[test]
        fn should_sum_quantities_across_orders_at_the_same_price() {
            let mut book = order_book();
            book.match_order(buy(1u64, 100u64, lot(1))).unwrap();
            book.match_order(buy(2u64, 100u64, lot(3))).unwrap();
            book.match_order(sell(3u64, 110u64, lot(2))).unwrap();
            book.match_order(sell(4u64, 110u64, lot(5))).unwrap();

            let bids: Vec<_> = book.bid_levels(10).collect();
            assert_eq!(bids, vec![(Price::new(100), Quantity::from(lot(4)))]);
            let asks: Vec<_> = book.ask_levels(10).collect();
            assert_eq!(asks, vec![(Price::new(110), Quantity::from(lot(7)))]);
        }

        #[test]
        fn should_truncate_to_limit() {
            let mut book = order_book();
            for (seq, price) in [(1u64, 100u64), (2u64, 90u64), (3u64, 80u64)] {
                book.match_order(buy(seq, price, lot(1))).unwrap();
            }
            for (seq, price) in [(4u64, 110u64), (5u64, 120u64), (6u64, 130u64)] {
                book.match_order(sell(seq, price, lot(1))).unwrap();
            }

            let bid_prices: Vec<_> = book.bid_levels(2).map(|(p, _)| p.get()).collect();
            assert_eq!(bid_prices, vec![100, 90]);
            let ask_prices: Vec<_> = book.ask_levels(2).map(|(p, _)| p.get()).collect();
            assert_eq!(ask_prices, vec![110, 120]);
        }

        #[test]
        fn should_return_all_levels_when_limit_exceeds_depth() {
            let mut book = order_book();
            book.match_order(buy(1u64, 100u64, lot(1))).unwrap();
            book.match_order(sell(2u64, 110u64, lot(1))).unwrap();

            assert_eq!(book.bid_levels(usize::MAX).count(), 1);
            assert_eq!(book.ask_levels(usize::MAX).count(), 1);
        }

        #[test]
        fn should_return_empty_iterators_when_limit_is_zero() {
            let mut book = order_book();
            book.match_order(buy(1u64, 100u64, lot(1))).unwrap();
            book.match_order(sell(2u64, 110u64, lot(1))).unwrap();

            assert_eq!(book.bid_levels(0).count(), 0);
            assert_eq!(book.ask_levels(0).count(), 0);
        }

        #[test]
        fn should_exclude_pending_orders() {
            let mut book = order_book();
            book.add_pending_order(buy(0u64, 100u64, lot(1)));
            book.add_pending_order(sell(1u64, 110u64, lot(1)));

            assert_eq!(book.bid_levels(10).count(), 0);
            assert_eq!(book.ask_levels(10).count(), 0);
        }

        #[test]
        fn should_saturate_aggregated_quantity_on_overflow() {
            use crate::order::{LotSize, OrderBook, OrderBookId, PendingOrder, Side, TickSize};
            use std::num::NonZeroU64;

            // lot_size = 1 lets us rest Quantity::MAX-sized orders (which
            // wouldn't be multiples of the default LOT_SIZE).
            let mut book = OrderBook::new(
                OrderBookId::ZERO,
                TickSize::new(NonZeroU64::new(1).unwrap()),
                LotSize::new(NonZeroU64::new(1).unwrap()),
            );
            let max_buy = |seq: u64| {
                PendingOrder {
                    side: Side::Buy,
                    price: Price::new(1),
                    quantity: Quantity::MAX,
                }
                .into_order(OrderSeq::new(seq))
            };
            book.match_order(max_buy(0)).unwrap();
            book.match_order(max_buy(1)).unwrap();

            let (_, quantity) = book.bid_levels(1).next().unwrap();
            assert_eq!(quantity, Quantity::MAX);
        }
    }
}

mod process_pending_orders {
    use crate::order::{MatchingOutput, Order, OrderBook, OrderSeq};
    use crate::test_fixtures::{LOT_SIZE, order_book};
    use std::collections::BTreeSet;

    fn buy(seq: u64, price: u64, quantity: u64) -> Order {
        crate::test_fixtures::buy(seq, price, quantity)
    }

    fn sell(seq: u64, price: u64, quantity: u64) -> Order {
        crate::test_fixtures::sell(seq, price, quantity)
    }

    fn process_all_pending_orders(book: &mut OrderBook) -> MatchingOutput {
        let seqs: Vec<OrderSeq> = book.pending_order_seqs().collect();
        book.process_pending_orders(&seqs)
    }

    #[test]
    fn should_return_empty_output_when_no_pending_orders() {
        let mut book = order_book();
        let output = process_all_pending_orders(&mut book);

        assert!(output.fills.is_empty());
        assert!(output.resting_orders.is_empty());
        assert!(output.filled_orders.is_empty());
    }

    #[test]
    fn should_report_resting_order_when_no_match() {
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        book.add_pending_order(buy(0, 100, lot));

        let output = process_all_pending_orders(&mut book);

        assert!(output.fills.is_empty());
        assert_eq!(output.resting_orders, BTreeSet::from([OrderSeq::ZERO]));
        assert!(output.filled_orders.is_empty());
    }

    #[test]
    fn should_report_filled_orders_after_exact_match() {
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        book.add_pending_order(sell(0, 100, lot));
        book.add_pending_order(buy(1, 100, lot));

        let output = process_all_pending_orders(&mut book);

        assert_eq!(output.fills.len(), 1);
        assert!(output.filled_orders.contains(&OrderSeq::ZERO)); // maker
        assert!(output.filled_orders.contains(&OrderSeq::ONE)); // taker
        assert!(output.resting_orders.is_empty());
    }

    #[test]
    fn should_report_partial_fill_with_resting_remainder() {
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        // Sell 1 lot (maker), buy 3 lots (taker) -> taker partially fills, rests with 2
        book.add_pending_order(sell(0, 100, lot));
        book.add_pending_order(buy(1, 100, 3 * lot));

        let output = process_all_pending_orders(&mut book);

        assert_eq!(output.fills.len(), 1);
        assert!(output.filled_orders.contains(&OrderSeq::ZERO)); // maker fully filled
        assert!(!output.filled_orders.contains(&OrderSeq::ONE)); // taker not fully filled
        assert_eq!(output.resting_orders, BTreeSet::from([OrderSeq::ONE])); // taker rests
    }

    #[test]
    fn should_drain_filled_orders_between_rounds() {
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        book.add_pending_order(sell(0, 100, lot));
        book.add_pending_order(buy(1, 100, lot));

        let first = process_all_pending_orders(&mut book);
        assert!(!first.filled_orders.is_empty());

        let second = process_all_pending_orders(&mut book);
        assert!(second.filled_orders.is_empty());
    }
}

mod remove_order {
    use crate::order::{
        OrderBookSnapshot, OrderSeq, Price, PriceLevel, Quantity, RemovedOrder, Side,
    };
    use crate::test_fixtures::arbitrary::{arb_non_matching_pending_order, arb_pending_order};
    use crate::test_fixtures::{LOT_SIZE, buy, order_book, sell};
    use proptest::collection::vec;
    use proptest::prelude::*;

    #[test]
    fn should_return_none_when_order_is_absent() {
        let mut book = order_book();
        assert_eq!(book.remove_order(OrderSeq::new(42)), None);
    }

    proptest! {
        #[test]
        fn should_remove_any_pending_order_and_preserve_fifo(
            pendings in vec(arb_pending_order(), 1..100),
            cancel_index in any::<prop::sample::Index>(),
        ) {
            let total = pendings.len();
            let idx = cancel_index.index(total);
            let expected = RemovedOrder {
                side: pendings[idx].side,
                price: pendings[idx].price,
                remaining_quantity: pendings[idx].quantity,
            };

            let mut book = order_book();
            for (i, p) in pendings.into_iter().enumerate() {
                book.add_pending_order(p.into_order(OrderSeq::new(i as u64)));
            }

            let removed = book.remove_order(OrderSeq::new(idx as u64)).unwrap();
            prop_assert_eq!(removed, expected);
            prop_assert_eq!(book.pending_orders_len(), total - 1);
            prop_assert_eq!(book.resting_orders_len(), 0);

            let expected_remaining: Vec<_> = (0..total)
                .filter(|&i| i != idx)
                .map(|i| OrderSeq::new(i as u64))
                .collect();
            let actual_remaining: Vec<_> = book.pending_order_seqs().collect();
            prop_assert_eq!(actual_remaining, expected_remaining);
        }

        /// For any non-matching book of arbitrary resting orders, cancelling
        /// any single order returns the exact resting payload, preserves the
        /// priority-then-FIFO order on the cancelled side, and leaves the
        /// opposite side untouched.
        #[test]
        fn should_remove_any_resting_order_and_preserve_fifo_on_same_side(
            orders in vec(arb_non_matching_pending_order(), 1..100),
            cancel_index in any::<prop::sample::Index>(),
        ) {
            let total = orders.len();
            let idx = cancel_index.index(total);
            let cancelled_side = orders[idx].side;
            let expected = RemovedOrder {
                side: cancelled_side,
                price: orders[idx].price,
                remaining_quantity: orders[idx].quantity,
            };
            let cancel_seq = OrderSeq::new(idx as u64);

            let mut book = order_book();
            for (i, p) in orders.into_iter().enumerate() {
                book.match_order(p.into_order(OrderSeq::new(i as u64))).unwrap();
            }
            let before = OrderBookSnapshot::from(&book);

            let removed = book.remove_order(cancel_seq).unwrap();
            prop_assert_eq!(removed, expected);
            prop_assert_eq!(book.resting_orders_len(), total - 1);

            let after = OrderBookSnapshot::from(&book);
            let (cancelled_before, cancelled_after, untouched_before, untouched_after) =
                match cancelled_side {
                    Side::Buy => (&before.bids, &after.bids, &before.asks, &after.asks),
                    Side::Sell => (&before.asks, &after.asks, &before.bids, &after.bids),
                };

            prop_assert_eq!(
                untouched_before, untouched_after,
                "opposite side must be unchanged",
            );

            let expected_seqs: Vec<_> = resting_seqs(cancelled_before)
                .into_iter()
                .filter(|s| *s != cancel_seq)
                .collect();
            prop_assert_eq!(resting_seqs(cancelled_after), expected_seqs);
        }
    }

    #[test]
    fn should_delete_empty_price_level_when_last_resting_removed() {
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        book.match_order(sell(0u64, 100u64, lot)).unwrap();
        book.match_order(sell(1u64, 110u64, 2 * lot)).unwrap();
        book.match_order(buy(2u64, 90u64, lot)).unwrap();
        book.match_order(buy(3u64, 80u64, 3 * lot)).unwrap();

        assert_eq!(
            book.remove_order(OrderSeq::ZERO).unwrap(),
            RemovedOrder {
                side: Side::Sell,
                price: Price::new(100),
                remaining_quantity: Quantity::from(lot),
            }
        );
        assert_eq!(book.asks_len(), 1);
        assert_eq!(book.resting_orders_len(), 3);
        assert_eq!(book.best_ask().unwrap().price(), Price::new(110));
        assert_eq!(book.best_bid().unwrap().price(), Price::new(90));

        assert_eq!(
            book.remove_order(OrderSeq::new(2)).unwrap(),
            RemovedOrder {
                side: Side::Buy,
                price: Price::new(90),
                remaining_quantity: Quantity::from(lot),
            }
        );
        assert_eq!(book.bids_len(), 1);
        assert_eq!(book.resting_orders_len(), 2);
        assert_eq!(book.best_ask().unwrap().price(), Price::new(110));
        assert_eq!(book.best_bid().unwrap().price(), Price::new(80));
    }

    #[test]
    fn should_report_residual_for_partially_filled_resting_order() {
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        // Rest a 3-lot sell; cross with a 1-lot buy to partially fill.
        book.match_order(sell(0u64, 100u64, 3 * lot)).unwrap();
        book.match_order(buy(1u64, 100u64, lot)).unwrap();

        let removed = book.remove_order(OrderSeq::ZERO).unwrap();

        assert_eq!(
            removed,
            RemovedOrder {
                side: Side::Sell,
                price: Price::new(100),
                remaining_quantity: Quantity::from(2 * lot),
            }
        );
        assert_eq!(book.resting_orders_len(), 0);
        assert!(book.is_empty());
    }

    #[test]
    fn should_return_none_for_fully_filled_order() {
        let mut book = order_book();
        let lot = u64::from(LOT_SIZE);
        book.match_order(sell(0u64, 100u64, lot)).unwrap();
        book.match_order(buy(1u64, 100u64, lot)).unwrap();

        // Both orders are fully filled; removing either must be a no-op.
        assert_eq!(book.remove_order(OrderSeq::ZERO), None);
        assert_eq!(book.remove_order(OrderSeq::ONE), None);
    }

    /// Flatten resting-side price levels into an in-priority seq list
    /// (price-prioritized across levels, FIFO within each level).
    fn resting_seqs(levels: &[PriceLevel]) -> Vec<OrderSeq> {
        levels
            .iter()
            .flat_map(|level| level.orders.iter().map(|o| o.id()))
            .collect()
    }
}

mod history {
    use crate::order::{
        OrderBookId, OrderHistory, OrderId, OrderRecord, OrderSeq, OrderStatus, Price, Quantity,
        Side,
    };
    use crate::test_fixtures::arbitrary::arb_order_record;
    use candid::Principal;
    use ic_stable_structures::{Storable, VectorMemory};
    use proptest::{prop_assert_eq, proptest};

    fn history() -> OrderHistory<VectorMemory> {
        OrderHistory::new(VectorMemory::default())
    }

    fn test_id(seq: u64) -> OrderId {
        OrderId::new(OrderBookId::ZERO, OrderSeq::new(seq))
    }

    fn test_record() -> OrderRecord {
        OrderRecord {
            owner: Principal::anonymous(),
            side: Side::Buy,
            price: Price::new(100),
            quantity: Quantity::from(1_000_000u64),
            status: OrderStatus::Pending,
        }
    }

    #[test]
    fn insert_once_and_get() {
        let mut history = history();
        let id = test_id(0);
        let record = test_record();
        history.insert_once(id, record.clone());

        assert_eq!(history.get(&id), Some(record));
    }

    #[test]
    #[should_panic(expected = "duplicate order ID")]
    fn insert_once_panics_on_duplicate() {
        let mut history = history();
        let id = test_id(0);
        history.insert_once(id, test_record());
        history.insert_once(id, test_record());
    }

    #[test]
    fn get_returns_none_for_missing() {
        let history = history();
        assert_eq!(history.get(&test_id(42)), None);
    }

    #[test]
    fn set_status_updates_status() {
        let mut history = history();
        let id = test_id(0);
        history.insert_once(id, test_record());

        assert_eq!(
            history.get(&id).map(|r| r.status),
            Some(OrderStatus::Pending),
        );
        history.set_status(&id, OrderStatus::Filled);
        assert_eq!(
            history.get(&id).map(|r| r.status),
            Some(OrderStatus::Filled),
        );
    }

    proptest! {
        #[test]
        fn should_store_order_record(
            record in arb_order_record(),
        ) {
            let bytes = record.to_bytes();
            let decoded = OrderRecord::from_bytes(bytes);
            prop_assert_eq!(decoded, record);
        }
    }
}

mod book_snapshot {
    use crate::order::{OrderBook, OrderBookSnapshot};
    use crate::test_fixtures::{LOT_SIZE, TEST_BOOK_ID, TICK_SIZE, arbitrary::arb_pending_order};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn should_roundtrip_through_snapshot(
            to_process in prop::collection::vec(arb_pending_order(), 0..100),
            to_leave_pending in prop::collection::vec(arb_pending_order(), 0..50),
        ) {
            let mut book = OrderBook::new(TEST_BOOK_ID, TICK_SIZE, LOT_SIZE);
            for pending in to_process {
                let seq = book.next_seq();
                book.add_pending_order(pending.into_order(seq));
            }
            let seqs: Vec<_> = book.pending_order_seqs().collect();
            let _ = book.process_pending_orders(&seqs);
            for pending in to_leave_pending {
                let seq = book.next_seq();
                book.add_pending_order(pending.into_order(seq));
            }

            let snapshot = OrderBookSnapshot::from(&book);
            let restored = OrderBook::from(snapshot);

            prop_assert_eq!(book, restored);
        }
    }
}

mod levels_consistency {
    use crate::order::{OrderBook, OrderSeq};
    use crate::test_fixtures::{
        LOT_SIZE, TEST_BOOK_ID, TICK_SIZE, arbitrary::arb_non_matching_orders,
    };
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn top_of_depth_matches_ticker(orders in arb_non_matching_orders()) {
            let mut book = OrderBook::new(TEST_BOOK_ID, TICK_SIZE, LOT_SIZE);
            for (i, pending) in orders.into_iter().enumerate() {
                book.match_order(pending.into_order(OrderSeq::new(i as u64))).unwrap();
            }
            let ticker_bid = book.bid_levels(1).next();
            let ticker_ask = book.ask_levels(1).next();
            let depth_bids: Vec<_> = book.bid_levels(usize::MAX).collect();
            let depth_asks: Vec<_> = book.ask_levels(usize::MAX).collect();

            prop_assert_eq!(ticker_bid, depth_bids.first().copied());
            prop_assert_eq!(ticker_ask, depth_asks.first().copied());
        }
    }
}
