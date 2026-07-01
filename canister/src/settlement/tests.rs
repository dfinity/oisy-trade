mod settled_fill {
    use crate::Timestamp;
    use crate::order::{FeeRates, Fill, OrderBookId, PairToken, Side, TradeRecord};
    use crate::settlement::FillSettlement;
    use crate::state::event::BalanceOperation;
    use crate::test_fixtures::arbitrary::{arb_fee_rates, arb_fill};
    use proptest::prelude::*;
    use std::num::NonZeroU64;

    const BOOK: OrderBookId = OrderBookId::ZERO;
    const BASE_SCALE: u64 = 100_000_000;
    const TIMESTAMP: Timestamp = Timestamp::new(42);

    proptest! {
        /// The two trade legs the settling phase rebuilds from the lean
        /// `FillEvent` — with the side recovered from the taker order and the
        /// execution price from the maker order — carry exactly the realized
        /// notional and fees the matching phase computed for the balance
        /// operations, so persisted trades can never diverge from the transfers.
        #[test]
        fn rebuilds_legs_identical_to_the_matching_computation(
            (fill, fee_rates) in (0..1_000u64).prop_flat_map(arb_fill).prop_flat_map(|fill| {
                arb_fee_rates().prop_map(move |rates| (fill.clone(), rates))
            }),
        ) {
            let base_scale = NonZeroU64::new(BASE_SCALE).unwrap();
            let settlement = FillSettlement::new(fill.clone(), fee_rates, base_scale);

            let mut ops = Vec::new();
            settlement.push_balance_operations(&mut ops);
            let (mut quote_amount, mut quote_fee, mut base_fee) = (None, None, None);
            for op in &ops {
                if let BalanceOperation::Transfer { token, amount, fee, .. } = op {
                    match token {
                        PairToken::Quote => {
                            quote_amount = Some(*amount);
                            quote_fee = Some(fee.unwrap_or(crate::order::Quantity::ZERO));
                        }
                        PairToken::Base => {
                            base_fee = Some(fee.unwrap_or(crate::order::Quantity::ZERO));
                        }
                    }
                }
            }
            let notional = quote_amount.expect("a fill emits a quote transfer");
            let (expected_taker_fee, expected_maker_fee) = match fill.taker_side {
                Side::Buy => (base_fee.unwrap(), quote_fee.unwrap()),
                Side::Sell => (quote_fee.unwrap(), base_fee.unwrap()),
            };

            let [taker_leg, maker_leg] = settlement.fill_event().trade_legs(
                BOOK,
                fill.taker_side,
                fill.maker_price,
                base_scale,
                TIMESTAMP,
            );

            let (_, taker) = taker_leg;
            let expected_taker = TradeRecord {
                side: fill.taker_side,
                price: fill.maker_price,
                quantity: fill.quantity,
                notional,
                fee: expected_taker_fee,
                fee_token: match fill.taker_side {
                    Side::Buy => PairToken::Base,
                    Side::Sell => PairToken::Quote,
                },
                is_maker: false,
                timestamp: TIMESTAMP,
            };
            prop_assert_eq!(taker, expected_taker);

            let (_, maker) = maker_leg;
            let expected_maker = TradeRecord {
                side: match fill.taker_side {
                    Side::Buy => Side::Sell,
                    Side::Sell => Side::Buy,
                },
                price: fill.maker_price,
                quantity: fill.quantity,
                notional,
                fee: expected_maker_fee,
                fee_token: match fill.taker_side {
                    Side::Buy => PairToken::Quote,
                    Side::Sell => PairToken::Base,
                },
                is_maker: true,
                timestamp: TIMESTAMP,
            };
            prop_assert_eq!(maker, expected_maker);
        }
    }

    /// The two legs share the match's `FillSeq` and differ only by their owning
    /// `OrderId`, so a `FillId` is derivable from either.
    #[test]
    fn both_legs_share_the_fill_seq() {
        let price = crate::order::Price::new(10_000_000);
        let fill = Fill {
            fill_seq: crate::order::FillSeq::new(7),
            taker_order_seq: crate::order::OrderSeq::new(2),
            taker_side: Side::Buy,
            taker_price: price,
            maker_order_seq: crate::order::OrderSeq::new(5),
            maker_price: price,
            quantity: crate::order::Quantity::from(100_000_000u64),
        };
        let base_scale = NonZeroU64::new(BASE_SCALE).unwrap();
        let settled = FillSettlement::new(fill, FeeRates::default(), base_scale).fill_event();
        let [(taker_id, _), (maker_id, _)] =
            settled.trade_legs(BOOK, Side::Buy, price, base_scale, TIMESTAMP);
        assert_eq!(taker_id.fill_id(), maker_id.fill_id());
        assert_ne!(taker_id.order_id(), maker_id.order_id());
    }
}

mod settlement_shape {
    use crate::order::{self, FeeRates, PairToken};
    use crate::settlement::{FillSettlement, RemovedOrderSettlement};
    use crate::state::event::BalanceOperation;
    use crate::test_fixtures::PRICE_SCALE;
    use proptest::prelude::*;

    proptest! {
        /// `FillSettlement::new` + `push_balance_operations` preserve structural invariants
        /// over any `MatchingOutput` the arbitrary strategy can produce:
        /// - never panics
        /// - emits exactly one Quote Transfer and one Base Transfer per fill
        /// - total op count is in `[2 * fills + expired, 3 * fills + expired]`
        ///   (the extra per-fill op is the buy-taker price-improvement
        ///   `Unreserve`; each killed order adds one refund `Unreserve`)
        /// This covers the fuzz shape the retired `settle_fill_ordering`
        /// proptest exercised, moved one layer up to the pure compute fn.
        #[test]
        fn settlement_balance_ops_match_fill_shape(
            output in crate::test_fixtures::arbitrary::arb_matching_output()
        ) {
            let base_scale = std::num::NonZeroU64::new(PRICE_SCALE as u64).unwrap();
            let fills_len = output.fills.len();
            let expired_len = output.expired_orders.len();
            let mut ops = Vec::new();
            for fill in &output.fills {
                let settlement = FillSettlement::new(fill.clone(), FeeRates::default(), base_scale);
                settlement.push_balance_operations(&mut ops);
            }
            for (seq, killed) in &output.expired_orders {
                RemovedOrderSettlement::new(*seq, killed, base_scale)
                    .push_balance_operations(&mut ops);
            }

            prop_assert!(
                ops.len() >= 2 * fills_len + expired_len
                    && ops.len() <= 3 * fills_len + expired_len,
                "ops.len() {} outside [{}, {}] for {} fills and {} expired",
                ops.len(),
                2 * fills_len + expired_len,
                3 * fills_len + expired_len,
                fills_len,
                expired_len,
            );

            let quote_transfers = ops.iter().filter(|o| matches!(
                o,
                BalanceOperation::Transfer { token: PairToken::Quote, .. }
            )).count();
            let base_transfers = ops.iter().filter(|o| matches!(
                o,
                BalanceOperation::Transfer { token: PairToken::Base, .. }
            )).count();
            prop_assert_eq!(quote_transfers, fills_len);
            prop_assert_eq!(base_transfers, fills_len);

            // Unreserves fire for buy-taker fills with strictly positive price
            // improvement, plus one refund per killed (expired) order.
            let expected_unreserves = output.fills.iter().filter(|f| {
                f.taker_side == order::Side::Buy && f.taker_price.get() > f.maker_price.get()
            }).count() + expired_len;
            let unreserves = ops.iter().filter(|o| matches!(
                o,
                BalanceOperation::Unreserve { .. }
            )).count();
            prop_assert_eq!(unreserves, expected_unreserves);
        }
    }
}
