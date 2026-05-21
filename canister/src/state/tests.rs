mod assert_caller_is_allowed {
    use crate::state::State;
    use crate::test_fixtures::mocks::MockRuntime;
    use candid::Principal;
    use dex_types_internal::Mode;

    #[test]
    fn should_allow_any_caller_in_general_availability_mode() {
        let state = state(Mode::GeneralAvailability);
        let mock = MockRuntime::new();
        state.assert_caller_is_allowed(&mock);
    }

    #[test]
    fn should_allow_caller_in_restricted_set() {
        let allowed_principal = Principal::from_slice(&[0x01]);
        let state = state(Mode::restricted_to(vec![allowed_principal]));

        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(allowed_principal);
        mock.expect_is_controller().return_const(false);

        state.assert_caller_is_allowed(&mock);
    }

    #[test]
    fn should_allow_controller_even_if_not_in_restricted_set() {
        let state = state(Mode::restricted_to(vec![]));
        let controller = Principal::from_slice(&[0xAA]);

        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(controller);
        mock.expect_is_controller().return_const(true);

        state.assert_caller_is_allowed(&mock);
    }

    #[test]
    #[should_panic(expected = "is not allowed to call this endpoint in restricted mode")]
    fn should_reject_caller_not_in_restricted_set() {
        let unauthorized = Principal::from_slice(&[0xFF]);
        let state = state(Mode::restricted_to(vec![]));

        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(unauthorized);
        mock.expect_is_controller().return_const(false);

        state.assert_caller_is_allowed(&mock);
    }

    fn state(
        mode: Mode,
    ) -> State<ic_stable_structures::VectorMemory, ic_stable_structures::VectorMemory> {
        State::new(
            dex_types_internal::InitArg {
                mode,
                max_orders_per_chunk: 1_000,
                instruction_budget: 1_000_000_000,
            },
            crate::state::OrderHistory::new(ic_stable_structures::VectorMemory::default()),
            crate::balance::TokenBalance::new(ic_stable_structures::VectorMemory::default()),
        )
        .unwrap()
    }
}

mod record_trading_pair {
    use crate::order::{OrderBookId, TokenId, TokenMetadata, TradingPair};
    use crate::test_fixtures;
    use crate::test_fixtures::{
        LOT_SIZE, TICK_SIZE, ckbtc_metadata, ckbtc_token_id, icp_ckbtc_trading_pair, icp_metadata,
        icp_token_id,
    };
    use candid::Principal;

    #[test]
    fn should_store_token_metadata() {
        let mut state = test_fixtures::state();
        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
        );

        assert_eq!(state.token_metadata(&icp_token_id()), Some(&icp_metadata()));
        assert_eq!(
            state.token_metadata(&ckbtc_token_id()),
            Some(&ckbtc_metadata())
        );
    }

    #[test]
    fn should_accept_same_metadata_for_existing_token() {
        let mut state = test_fixtures::state();
        let token_c = TokenId::new(Principal::from_slice(&[0x03]));
        let token_c_metadata = TokenMetadata {
            symbol: "ckETH".to_string(),
            decimals: 18,
        };

        // First pair: ICP/ckBTC
        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
        );

        // Second pair: ICP/ckETH — ICP already registered with same metadata
        state.record_trading_pair(
            OrderBookId::ONE,
            TradingPair {
                base: icp_token_id(),
                quote: token_c,
            },
            icp_metadata(),
            token_c_metadata,
            TICK_SIZE,
            LOT_SIZE,
        );
    }

    #[test]
    fn should_assign_distinct_order_book_ids() {
        let mut state = test_fixtures::state();
        let token_c = TokenId::new(Principal::from_slice(&[0x03]));

        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
        );

        state.record_trading_pair(
            OrderBookId::ONE,
            TradingPair {
                base: icp_token_id(),
                quote: token_c,
            },
            icp_metadata(),
            TokenMetadata {
                symbol: "ckETH".to_string(),
                decimals: 18,
            },
            TICK_SIZE,
            LOT_SIZE,
        );

        let book_ids: Vec<_> = state.trading_pairs().iter().map(|(_, id)| id).collect();
        assert_eq!(book_ids.len(), 2);
        assert_ne!(book_ids[0], book_ids[1]);
    }
}

mod add_limit_order {
    use crate::order::{OrderBookId, PendingOrder, Price, Quantity, Side};
    use crate::state::AddLimitOrderError;
    use crate::test_fixtures;
    use crate::test_fixtures::{
        LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata,
    };
    use assert_matches::assert_matches;
    use candid::Principal;

    #[test]
    fn should_not_insert_empty_balance_on_failed_reservation() {
        let mut state = test_fixtures::state();
        let pair = icp_ckbtc_trading_pair();
        state.record_trading_pair(
            OrderBookId::ZERO,
            pair.clone(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
        );
        let user = Principal::from_slice(&[0x01]);
        let pending = PendingOrder {
            side: Side::Buy,
            price: Price::new(100),
            quantity: Quantity::from(LOT_SIZE.get()),
        };
        let result = state.validate_limit_order(user, pair, pending);

        assert_matches!(result, Err(AddLimitOrderError::InsufficientBalance { .. }));
    }
}

mod cancel_limit_order {
    use crate::balance::Balance;
    use crate::order::{
        CanceledOrderInfo, OrderBookId, OrderId, OrderStatus, PairToken, Quantity, Side,
    };
    use crate::state::State;
    use crate::test_fixtures::mocks::{MockRuntime, mock_runtime_for};
    use crate::test_fixtures::{
        self, LOT_SIZE, TICK_SIZE, balances_pair, ckbtc_metadata, icp_ckbtc_trading_pair,
        icp_metadata, place_order,
    };
    use candid::Principal;
    use ic_stable_structures::VectorMemory;

    const OWNER: Principal = Principal::from_slice(&[0x01]);
    const STRANGER: Principal = Principal::from_slice(&[0x02]);

    #[test]
    fn should_refund_full_reserved_quote_for_pending_buy() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        let buy_id = place_order(&mut state, OWNER, &pair, Side::Buy, 100, lot);

        assert_cancel_refunds(&mut state, OWNER, buy_id, PairToken::Quote, 100 * lot, lot);
    }

    #[test]
    fn should_refund_base_for_pending_sell() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        let sell_id = place_order(&mut state, OWNER, &pair, Side::Sell, 100, lot);

        assert_cancel_refunds(&mut state, OWNER, sell_id, PairToken::Base, lot, lot);
    }

    #[test]
    fn should_refund_resting_buy_after_matching_runs() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        let buy_id = place_order(&mut state, OWNER, &pair, Side::Buy, 100, lot);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));
        assert_eq!(state.get_order_status(buy_id), Some(OrderStatus::Open));

        assert_cancel_refunds(&mut state, OWNER, buy_id, PairToken::Quote, 100 * lot, lot);
    }

    #[test]
    fn should_refund_resting_sell_after_matching_runs() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        let sell_id = place_order(&mut state, OWNER, &pair, Side::Sell, 100, lot);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));
        assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Open));

        assert_cancel_refunds(&mut state, OWNER, sell_id, PairToken::Base, lot, lot);
    }

    #[test]
    fn should_refund_residual_of_partially_filled_buy() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        // Maker sells 1 lot; taker buys 3 lots — taker partially fills and rests with 2 lots.
        place_order(&mut state, STRANGER, &pair, Side::Sell, 100, lot);
        let buy_id = place_order(&mut state, OWNER, &pair, Side::Buy, 100, 3 * lot);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));
        assert_eq!(state.get_order_status(buy_id), Some(OrderStatus::Open));

        assert_cancel_refunds(
            &mut state,
            OWNER,
            buy_id,
            PairToken::Quote,
            2 * 100 * lot,
            2 * lot,
        );
    }

    #[test]
    fn should_refund_residual_of_partially_filled_sell() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        // Maker buys 1 lot; taker sells 3 lots — taker partially fills and rests with 2 lots.
        place_order(&mut state, STRANGER, &pair, Side::Buy, 100, lot);
        let sell_id = place_order(&mut state, OWNER, &pair, Side::Sell, 100, 3 * lot);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));
        assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Open));

        assert_cancel_refunds(
            &mut state,
            OWNER,
            sell_id,
            PairToken::Base,
            2 * lot,
            2 * lot,
        );
    }

    /// Cancels `order_id` owned by `user` and asserts that exactly
    /// `expected_amount` units of `refund_token` move from reserved to free;
    /// the other token's balance is unchanged and the order status becomes
    /// `Canceled(CanceledOrderInfo { remaining_quantity: expected_remaining })`.
    fn assert_cancel_refunds(
        state: &mut State<VectorMemory, VectorMemory>,
        user: Principal,
        order_id: OrderId,
        refund_token: PairToken,
        expected_amount: impl Into<Quantity>,
        expected_remaining: impl Into<Quantity>,
    ) {
        let mut runtime = MockRuntime::new();
        runtime.expect_time().return_const(0u64);
        let expected_amount = expected_amount.into();
        let expected_remaining = expected_remaining.into();
        let pair = icp_ckbtc_trading_pair();
        let (base_before, quote_before) = balances_pair(&state.balances, &user, &pair);

        let order = state.cancel_limit_order(&user, order_id, &runtime).unwrap();
        assert!(
            matches!(order.status, OrderStatus::Canceled( info ) if info.remaining_quantity == expected_remaining )
        );

        let (base_after, quote_after) = balances_pair(&state.balances, &user, &pair);
        assert_eq!(
            state.get_order_status(order_id),
            Some(OrderStatus::Canceled(CanceledOrderInfo {
                remaining_quantity: expected_remaining,
            })),
        );
        let (refunded_before, refunded_after, untouched_before, untouched_after) =
            match refund_token {
                PairToken::Base => (base_before, base_after, quote_before, quote_after),
                PairToken::Quote => (quote_before, quote_after, base_before, base_after),
            };
        assert_eq!(
            refunded_after,
            Balance::new(
                refunded_before.free().checked_add(expected_amount).unwrap(),
                refunded_before
                    .reserved()
                    .checked_sub(&expected_amount)
                    .unwrap(),
            ),
            "refund on {refund_token:?} differed from expected {expected_amount:?}",
        );
        assert_eq!(
            untouched_before, untouched_after,
            "the non-refund token balance should not change",
        );
    }

    fn setup() -> State<VectorMemory, VectorMemory> {
        let mut state = test_fixtures::state();
        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
        );
        state
    }
}

mod validate_overflow_invariant {
    use crate::order::{LotSize, OrderBookId, PendingOrder, Price, Quantity, Side, TickSize};
    use crate::state::AddLimitOrderError;
    use crate::test_fixtures;
    use crate::test_fixtures::{ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata};
    use candid::Principal;
    use proptest::prelude::*;
    use std::num::NonZeroU64;

    fn arb_quantity() -> impl Strategy<Value = Quantity> {
        (any::<u128>(), any::<u128>()).prop_map(|(high, low)| Quantity::new(high, low))
    }

    fn arb_side() -> impl Strategy<Value = Side> {
        prop_oneof![Just(Side::Buy), Just(Side::Sell)]
    }

    proptest! {
        // `record_limit_order` and `settle_fill` rely on `price * quantity`
        // not overflowing once an order has passed `validate_limit_order`.
        // Settlement computes `maker_price × fill.quantity` regardless of
        // the maker's side, so the invariant must hold for Buy and Sell alike.
        // This biconditional pins that guarantee: validation rejects with
        // `AmountExceedsMaximum` exactly when the multiplication would overflow.
        #[test]
        fn validate_rejects_iff_price_times_quantity_overflows(
            price_raw in 1u64..=u64::MAX,
            quantity in arb_quantity(),
            side in arb_side(),
        ) {
            // tick=lot=1 so tick/lot checks accept any non-zero price/quantity,
            // leaving `AmountExceedsMaximum` as the only overflow-driven rejection.
            let tick = TickSize::new(NonZeroU64::new(1).unwrap());
            let lot = LotSize::new(NonZeroU64::new(1).unwrap());

            let mut state = test_fixtures::state();
            let pair = icp_ckbtc_trading_pair();
            state.record_trading_pair(
                OrderBookId::ZERO,
                pair.clone(),
                icp_metadata(),
                ckbtc_metadata(),
                tick,
                lot,
            );

            let price = Price::new(price_raw);
            let fits = price.checked_mul_quantity(&quantity).is_some();

            let result = state.validate_limit_order(
                Principal::from_slice(&[0x01]),
                pair,
                PendingOrder {
                    side,
                    price,
                    quantity,
                },
            );

            let rejected_for_overflow =
                matches!(result, Err(AddLimitOrderError::AmountExceedsMaximum));
            prop_assert_eq!(
                rejected_for_overflow,
                !fits,
                "result was {:?}, fits={}",
                result,
                fits
            );
        }
    }
}

mod settle_fills {
    use crate::balance::Balance;
    use crate::order::{OrderBookId, Price, Quantity, Side};
    use crate::state::State;
    use crate::test_fixtures;
    use crate::test_fixtures::mocks::mock_runtime_for;
    use crate::test_fixtures::{
        LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata,
    };
    use candid::Principal;
    use ic_stable_structures::VectorMemory;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    type TestState = State<VectorMemory, VectorMemory>;

    const BUYER: Principal = Principal::from_slice(&[0x01]);
    const SELLER: Principal = Principal::from_slice(&[0x02]);

    #[test]
    fn should_settle_exact_match_at_same_price() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        let price = 100u64;

        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, price, lot);
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, price, lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(0, 0));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0, 0));
        assert_eq!(seller_quote, balance(price * lot, 0));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_unreserve_surplus_when_buy_taker_fills_at_lower_price() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);

        // Sell rests at 90, buy taker at 100 → fills at maker's 90
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 90, lot);
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Buyer deposited 100*lot quote, paid 90*lot, surplus 10*lot returned to free
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(10 * lot, 0));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0, 0));
        assert_eq!(seller_quote, balance(90 * lot, 0));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_sell_taker_at_higher_maker_price() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);

        // Buy rests at 110, sell taker at 100 → fills at maker's 110
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 110, lot);
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(0, 0));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0, 0));
        // Seller gets 110*lot quote (better than their limit of 100)
        assert_eq!(seller_quote, balance(110 * lot, 0));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_partial_fill() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);

        // Buy 3 lots at 100, only 1 lot of sell available
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, 3 * lot);
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Buyer filled 1 lot, 2 lots remain reserved
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(0, 200 * lot));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0, 0));
        assert_eq!(seller_quote, balance(100 * lot, 0));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_multiple_fills_across_price_levels() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);

        // Two sells at different prices, buy taker sweeps both
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 90, lot);
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, lot);
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, 2 * lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Buyer deposited 100*2*lot = 200*lot quote
        // Paid 90*lot + 100*lot = 190*lot, surplus = 10*lot
        assert_eq!(buyer_base, balance(2 * lot, 0));
        assert_eq!(buyer_quote, balance(10 * lot, 0));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_buy_taker_partial_fill_with_price_improvement() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);

        // Sell rests at 90 for 1 lot, buy taker at 100 for 3 lots
        // Fills 1 lot at 90, rests 2 lots
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 90, lot);
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, 3 * lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Surplus: (100-90)*lot = 10*lot returned to free
        // Remaining reserved: 100*2*lot = 200*lot
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(10 * lot, 200 * lot));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0, 0));
        assert_eq!(seller_quote, balance(90 * lot, 0));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_sell_taker_partial_fill() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);

        // Buy rests 1 lot at 100, sell taker 3 lots at 100
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, 3 * lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(0, 0));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        // 1 lot filled, 2 lots remain reserved
        assert_eq!(seller_base, balance(0, 2 * lot));
        assert_eq!(seller_quote, balance(100 * lot, 0));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_sell_taker_multi_level_sweep() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);

        // Two buys at different prices, sell taker sweeps both
        // Sell at 100 matches buy at 110 first, then buy at 100
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 110, lot);
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, 2 * lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Buyer deposited 100*lot + 110*lot = 210*lot quote, all consumed
        assert_eq!(buyer_base, balance(2 * lot, 0));
        assert_eq!(buyer_quote, balance(0, 0));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0, 0));
        // Seller receives 110*lot + 100*lot = 210*lot quote
        assert_eq!(seller_quote, balance(210 * lot, 0));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_self_trade() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        let user = Principal::from_slice(&[0x42]);

        // Same user places both buy and sell
        test_fixtures::place_order(&mut state, user, &pair, Side::Buy, 100, lot);
        test_fixtures::place_order(&mut state, user, &pair, Side::Sell, 100, lot);

        let base_before = state.get_balance(&user, &pair.base);
        let quote_before = state.get_balance(&user, &pair.quote);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));
        let base_after = state.get_balance(&user, &pair.base);
        let quote_after = state.get_balance(&user, &pair.quote);

        // Total tokens unchanged: base and quote just move between free/reserved
        assert_eq!(
            base_before
                .free()
                .checked_add(*base_before.reserved())
                .unwrap(),
            base_after
                .free()
                .checked_add(*base_after.reserved())
                .unwrap(),
            "base token total changed"
        );
        assert_eq!(
            quote_before
                .free()
                .checked_add(*quote_before.reserved())
                .unwrap(),
            quote_after
                .free()
                .checked_add(*quote_after.reserved())
                .unwrap(),
            "quote token total changed"
        );
        // After self-trade: all reserved released, net balances same as deposited
        assert_eq!(base_after, balance(lot, 0));
        assert_eq!(quote_after, balance(100 * lot, 0));
    }

    #[test]
    fn should_settle_taker_against_multiple_different_makers() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);

        let seller_a = Principal::from_slice(&[0x0A]);
        let seller_b = Principal::from_slice(&[0x0B]);

        // Two sellers place 1 lot each at different prices
        test_fixtures::place_order(&mut state, seller_a, &pair, Side::Sell, 90, lot);
        test_fixtures::place_order(&mut state, seller_b, &pair, Side::Sell, 100, lot);

        // Buy taker sweeps both
        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, 2 * lot);
        let participants = [BUYER, seller_a, seller_b];
        let totals_before = snapshot_balances(&state, &participants);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        // Buyer: received 2 lots, paid 90*lot + 100*lot, surplus 10*lot
        assert_eq!(state.get_balance(&BUYER, &pair.base), balance(2 * lot, 0));
        assert_eq!(state.get_balance(&BUYER, &pair.quote), balance(10 * lot, 0));

        // Seller A: sold 1 lot at 90
        assert_eq!(state.get_balance(&seller_a, &pair.base), balance(0, 0));
        assert_eq!(
            state.get_balance(&seller_a, &pair.quote),
            balance(90 * lot, 0)
        );

        // Seller B: sold 1 lot at 100
        assert_eq!(state.get_balance(&seller_b, &pair.base), balance(0, 0));
        assert_eq!(
            state.get_balance(&seller_b, &pair.quote),
            balance(100 * lot, 0)
        );

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_trade_with_quantity_exceeding_u64_max() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let price = 100u64;
        // quantity = LOT_SIZE * u64::MAX, guaranteed to be a valid lot multiple and > u64::MAX
        let quantity = Quantity::from(u64::from(LOT_SIZE))
            .checked_mul_u64(u64::MAX)
            .unwrap();

        test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, price, quantity);
        test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, price, quantity);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let quote_total = Price::new(price).checked_mul_quantity(&quantity).unwrap();

        // Buyer received all base tokens
        let buyer_base = state.get_balance(&BUYER, &pair.base);
        assert_eq!(buyer_base.free(), &quantity);
        assert_eq!(buyer_base.reserved(), &Quantity::ZERO);

        // Seller received price * quantity quote tokens
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_quote.free(), &quote_total);
        assert_eq!(seller_quote.reserved(), &Quantity::ZERO);

        assert_token_conservation(&state, &totals_before);
    }

    /// Regression test for the multi-book drain path in
    /// `process_pending_orders`: two trading pairs both produce a
    /// `SettlingEvent` in the same call, so the drain loop has to process
    /// more than one queued event. A bug that pops the queue outside the
    /// drain loop would silently drop the second book's settlement.
    #[test]
    fn should_settle_matches_across_multiple_books() {
        use crate::order::{TokenId, TokenMetadata, TradingPair};

        let mut state = test_fixtures::state();
        let lot = u64::from(LOT_SIZE);
        let price = 100u64;

        // Pair A: ICP/ckBTC (book 0).
        let pair_a = icp_ckbtc_trading_pair();
        state.record_trading_pair(
            OrderBookId::ZERO,
            pair_a.clone(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
        );
        // Pair B: a distinct base/quote token pair on a second book.
        let base_b = TokenId::new(Principal::from_slice(&[0xB1]));
        let quote_b = TokenId::new(Principal::from_slice(&[0xB2]));
        let pair_b = TradingPair {
            base: base_b,
            quote: quote_b,
        };
        state.record_trading_pair(
            OrderBookId::ONE,
            pair_b.clone(),
            TokenMetadata {
                symbol: "B".to_string(),
                decimals: 8,
            },
            TokenMetadata {
                symbol: "Q".to_string(),
                decimals: 8,
            },
            TICK_SIZE,
            LOT_SIZE,
        );

        let buyer_a = Principal::from_slice(&[0x0A, 0x01]);
        let seller_a = Principal::from_slice(&[0x0A, 0x02]);
        let buyer_b = Principal::from_slice(&[0x0B, 0x01]);
        let seller_b = Principal::from_slice(&[0x0B, 0x02]);
        test_fixtures::place_order(&mut state, buyer_a, &pair_a, Side::Buy, price, lot);
        test_fixtures::place_order(&mut state, seller_a, &pair_a, Side::Sell, price, lot);
        test_fixtures::place_order(&mut state, buyer_b, &pair_b, Side::Buy, price, lot);
        test_fixtures::place_order(&mut state, seller_b, &pair_b, Side::Sell, price, lot);

        state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        // Both books settled: both buyers hold their base free, both
        // sellers hold their quote free, no reserves left. If the second
        // book's SettlingEvent were silently dropped, buyer_b would
        // still have `price * lot` reserved and seller_b would still hold
        // `lot` reserved.
        assert_eq!(state.get_balance(&buyer_a, &pair_a.base), balance(lot, 0));
        assert_eq!(
            state.get_balance(&seller_a, &pair_a.quote),
            balance(price * lot, 0),
        );
        assert_eq!(state.get_balance(&buyer_b, &pair_b.base), balance(lot, 0));
        assert_eq!(
            state.get_balance(&seller_b, &pair_b.quote),
            balance(price * lot, 0),
        );
    }

    fn setup() -> TestState {
        let mut state = test_fixtures::state();
        let pair = icp_ckbtc_trading_pair();
        state.record_trading_pair(
            OrderBookId::ZERO,
            pair,
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
        );
        state
    }

    mod order_status {
        use super::*;
        use crate::order::OrderStatus;

        #[test]
        fn should_return_pending_before_matching() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let pair = icp_ckbtc_trading_pair();
            let buy_id = test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);

            assert_eq!(state.get_order_status(buy_id), Some(OrderStatus::Pending));
        }

        #[test]
        fn should_return_open_for_resting_order() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let pair = icp_ckbtc_trading_pair();
            let buy_id = test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
            state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

            assert_eq!(state.get_order_status(buy_id), Some(OrderStatus::Open));
        }

        #[test]
        fn should_return_filled_after_exact_match() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let pair = icp_ckbtc_trading_pair();
            let buy_id = test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
            let sell_id =
                test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, lot);
            state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

            assert_eq!(state.get_order_status(buy_id), Some(OrderStatus::Filled));
            assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Filled));
        }

        #[test]
        fn should_return_open_for_partially_filled_maker() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let pair = icp_ckbtc_trading_pair();
            // Sell 3 lots, buy only 1 → sell partially filled, remainder rests
            let sell_id =
                test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, 3 * lot);
            let buy_id = test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
            state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

            assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Open));
            assert_eq!(state.get_order_status(buy_id), Some(OrderStatus::Filled));
        }

        #[test]
        fn should_return_open_for_partially_filled_taker() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let pair = icp_ckbtc_trading_pair();
            // Sell 1 lot, buy 3 lots → buy partially fills and rests with 2 remaining
            let sell_id =
                test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, lot);
            let buy_id =
                test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, 3 * lot);
            state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

            assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Filled));
            assert_eq!(state.get_order_status(buy_id), Some(OrderStatus::Open));
        }

        #[test]
        fn should_return_filled_after_multi_fill_maker_depletion() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let pair = icp_ckbtc_trading_pair();
            // Sell rests with 2 lots; two successive buys deplete it
            let sell_id =
                test_fixtures::place_order(&mut state, SELLER, &pair, Side::Sell, 100, 2 * lot);
            state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));
            assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Open));

            let buy1_id = test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
            state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));
            assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Open));
            assert_eq!(state.get_order_status(buy1_id), Some(OrderStatus::Filled));

            let buy2_id = test_fixtures::place_order(&mut state, BUYER, &pair, Side::Buy, 100, lot);
            state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));
            assert_eq!(state.get_order_status(sell_id), Some(OrderStatus::Filled));
            assert_eq!(state.get_order_status(buy2_id), Some(OrderStatus::Filled));
        }
    }

    // The old `settle_fill_ordering` proptest lived here, testing that two
    // `settle_fill` calls on independent fills commuted. `settle_fill` has
    // been retired — settlement is now a flat `Vec<BalanceOperation>` in
    // `SettlingEvent`. Commutativity isn't claimed for arbitrary op sequences
    // (two Transfers from the same debtor can fail depending on order), only
    // for op sequences produced by `compute_balance_operations` from a valid
    // `MatchingOutput`.

    proptest! {
        /// `compute_balance_operations` preserves structural invariants over
        /// any `MatchingOutput` the arbitrary strategy can produce:
        /// - never panics
        /// - emits exactly one Quote Transfer and one Base Transfer per fill
        /// - total op count is in `[2 * fills, 3 * fills]` (the extra op is
        ///   the buy-taker price-improvement `Unreserve`)
        /// This covers the fuzz shape the retired `settle_fill_ordering`
        /// proptest exercised, moved one layer up to the pure compute fn.
        #[test]
        fn compute_balance_operations_matches_fill_shape(
            output in crate::test_fixtures::arbitrary::arb_matching_output()
        ) {
            use crate::order::{self, PairToken};
            use crate::state::event::BalanceOperation;

            let ops = super::super::compute_balance_operations(&output);
            let fills_len = output.fills.len();

            prop_assert!(
                ops.len() >= 2 * fills_len && ops.len() <= 3 * fills_len,
                "ops.len() {} outside [{}, {}] for {} fills",
                ops.len(), 2 * fills_len, 3 * fills_len, fills_len,
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

            // Unreserves only fire for buy-taker fills with strictly positive
            // price improvement.
            let expected_unreserves = output.fills.iter().filter(|f| {
                f.taker_side == order::Side::Buy && f.taker_price.get() > f.maker_price.get()
            }).count();
            let unreserves = ops.iter().filter(|o| matches!(
                o,
                BalanceOperation::Unreserve { .. }
            )).count();
            prop_assert_eq!(unreserves, expected_unreserves);
        }
    }

    fn balance(free: u64, reserved: u64) -> Balance {
        Balance::new(free, reserved)
    }

    type BalanceSnapshot = BTreeMap<Principal, (Balance, Balance)>;

    /// Snapshot base and quote balances for each principal.
    fn snapshot_balances(state: &TestState, principals: &[Principal]) -> BalanceSnapshot {
        let pair = icp_ckbtc_trading_pair();
        principals
            .iter()
            .map(|p| {
                (
                    *p,
                    (
                        state.get_balance(p, &pair.base),
                        state.get_balance(p, &pair.quote),
                    ),
                )
            })
            .collect()
    }

    /// Assert that the total base and quote tokens across all principals are unchanged.
    fn assert_token_conservation(state: &TestState, before: &BalanceSnapshot) {
        let principals: Vec<Principal> = before.keys().copied().collect();
        let after = snapshot_balances(state, &principals);

        let sum = |snap: &BalanceSnapshot| -> (Quantity, Quantity) {
            snap.values().fold(
                (Quantity::ZERO, Quantity::ZERO),
                |(base_acc, quote_acc), (base, quote)| {
                    (
                        base_acc
                            .checked_add(*base.free())
                            .unwrap()
                            .checked_add(*base.reserved())
                            .unwrap(),
                        quote_acc
                            .checked_add(*quote.free())
                            .unwrap()
                            .checked_add(*quote.reserved())
                            .unwrap(),
                    )
                },
            )
        };

        let (base_before, quote_before) = sum(before);
        let (base_after, quote_after) = sum(&after);
        assert_eq!(base_before, base_after, "base token total changed");
        assert_eq!(quote_before, quote_after, "quote token total changed");
    }
}

mod execution_policy {
    use crate::balance::TokenBalance;
    use crate::order::OrderHistory;
    use crate::state::{ExecutionPolicy, State};
    use dex_types_internal::{InitArg, Mode};
    use ic_stable_structures::VectorMemory;

    #[test]
    fn should_thread_init_arg_fields_through_to_execution_policy() {
        let state = State::new(
            InitArg {
                mode: Mode::GeneralAvailability,
                max_orders_per_chunk: 17,
                instruction_budget: 12_345,
            },
            OrderHistory::new(VectorMemory::default()),
            TokenBalance::new(VectorMemory::default()),
        )
        .unwrap();

        assert_eq!(state.execution_policy(), &ExecutionPolicy::new(17, 12_345));
    }
}
