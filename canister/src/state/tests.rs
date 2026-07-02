mod assert_caller_is_allowed {
    use crate::state::State;
    use crate::test_fixtures::mocks::MockRuntime;
    use candid::Principal;
    use oisy_trade_types_internal::Mode;

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
            oisy_trade_types_internal::InitArg {
                mode,
                max_orders_per_chunk: oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
                instruction_budget: oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
            },
            crate::state::OrderHistory::new(
                ic_stable_structures::VectorMemory::default(),
                ic_stable_structures::VectorMemory::default(),
            ),
            crate::state::TradeHistory::new(
                ic_stable_structures::VectorMemory::default(),
                ic_stable_structures::VectorMemory::default(),
            ),
            crate::user::UserRegistry::new(ic_stable_structures::VectorMemory::default()),
            crate::balance::TokenBalance::new(ic_stable_structures::VectorMemory::default()),
        )
        .unwrap()
    }
}

mod record_trading_pair {
    use crate::order::{FeeRates, OrderBookId, TokenId, TokenMetadata, TradingPair};
    use crate::test_fixtures;
    use crate::test_fixtures::{
        LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, TICK_SIZE, ckbtc_metadata, ckbtc_token_id,
        icp_ckbtc_trading_pair, icp_metadata, icp_token_id,
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
    }

    #[test]
    fn should_dedup_shared_token_across_pairs() {
        let mut state = test_fixtures::state();
        let token_c = TokenId::new(Principal::from_slice(&[0x03]));
        let token_c_metadata = TokenMetadata {
            symbol: "ckETH".to_string(),
            decimals: 18,
        };

        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );

        assert_eq!(state.tokens().len(), 3);
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );

        let book_ids: Vec<_> = state.trading_pairs().iter().map(|(_, id)| id).collect();
        assert_eq!(book_ids.len(), 2);
        assert_ne!(book_ids[0], book_ids[1]);
    }
}

mod add_limit_order {
    use crate::order::{FeeRates, OrderBookId, PendingOrder, Price, Quantity, Side, TimeInForce};
    use crate::state::AddLimitOrderError;
    use crate::test_fixtures;
    use crate::test_fixtures::{
        LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, PRICE_SCALE, TICK_SIZE, ckbtc_metadata,
        icp_ckbtc_trading_pair, icp_metadata,
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
        let user = Principal::from_slice(&[0x01]);
        let pending = PendingOrder {
            side: Side::Buy,
            price: Price::new(100 * PRICE_SCALE),
            quantity: Quantity::from(LOT_SIZE.get()),
            time_in_force: TimeInForce::GoodTilCanceled,
        };
        let result = state.validate_limit_order(user, pair, pending);

        assert_matches!(result, Err(AddLimitOrderError::InsufficientBalance { .. }));
    }
}

mod cancel_limit_order {
    use crate::EXECUTOR;
    use crate::balance::Balance;
    use crate::order::{FeeRates, OrderBookId, OrderId, OrderStatus, PairToken, Quantity, Side};
    use crate::state::State;
    use crate::test_fixtures::mocks::{MockRuntime, mock_runtime_for};
    use crate::test_fixtures::{
        self, LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, PRICE_SCALE, TICK_SIZE, balances_pair,
        ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata, order,
    };
    use candid::Principal;
    use ic_stable_structures::VectorMemory;

    const OWNER: Principal = Principal::from_slice(&[0x01]);
    const STRANGER: Principal = Principal::from_slice(&[0x02]);

    /// Status of `order_id` as `OWNER` would see it via `get_user_order`, or
    /// `None` if absent / not owned by `OWNER`.
    fn owner_status(
        state: &State<VectorMemory, VectorMemory>,
        order_id: OrderId,
    ) -> Option<OrderStatus> {
        state
            .get_user_order(&OWNER, order_id)
            .map(|(_, _, record)| record.status)
    }

    #[test]
    fn should_refund_full_reserved_quote_for_pending_buy() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        let buy_id = order(OWNER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);

        assert_cancel_refunds(&mut state, OWNER, buy_id, PairToken::Quote, 100 * lot, lot);
    }

    #[test]
    fn should_refund_base_for_pending_sell() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        let sell_id = order(OWNER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);

        assert_cancel_refunds(&mut state, OWNER, sell_id, PairToken::Base, lot, lot);
    }

    #[test]
    fn should_refund_resting_buy_after_matching_runs() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        let buy_id = order(OWNER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
        assert_eq!(owner_status(&state, buy_id), Some(OrderStatus::Open));

        assert_cancel_refunds(&mut state, OWNER, buy_id, PairToken::Quote, 100 * lot, lot);
    }

    #[test]
    fn should_refund_resting_sell_after_matching_runs() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        let sell_id = order(OWNER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
        assert_eq!(owner_status(&state, sell_id), Some(OrderStatus::Open));

        assert_cancel_refunds(&mut state, OWNER, sell_id, PairToken::Base, lot, lot);
    }

    #[test]
    fn should_refund_residual_of_partially_filled_buy() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        // Maker sells 1 lot; taker buys 3 lots — taker partially fills and rests with 2 lots.
        order(STRANGER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);
        let buy_id = order(OWNER, &pair, Side::Buy, 100 * PRICE_SCALE, 3 * lot).place(&mut state);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
        assert_eq!(owner_status(&state, buy_id), Some(OrderStatus::Open));

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
        let lot = u128::from(LOT_SIZE.get());
        // Maker buys 1 lot; taker sells 3 lots — taker partially fills and rests with 2 lots.
        order(STRANGER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        let sell_id = order(OWNER, &pair, Side::Sell, 100 * PRICE_SCALE, 3 * lot).place(&mut state);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
        assert_eq!(owner_status(&state, sell_id), Some(OrderStatus::Open));

        assert_cancel_refunds(
            &mut state,
            OWNER,
            sell_id,
            PairToken::Base,
            2 * lot,
            2 * lot,
        );
    }

    #[test]
    fn should_not_panic_canceling_order_matched_but_not_yet_settled() {
        use crate::state::CancelLimitOrderError;

        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        // Crossing pair: both fully fill when matched.
        let buy_id = order(OWNER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        let _sell_id = order(STRANGER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);

        // Record only the matching half: the book pops both orders into
        // `filled_orders` and the paired `SettlingEvent` lands on the queue
        // without being drained — exactly the state left behind by a chunk
        // whose inline drain was budget-interrupted.
        let orders: Vec<_> = state
            .order_book(&OrderBookId::ZERO)
            .unwrap()
            .pending_order_seqs()
            .collect();
        state.record_matching_event(
            &crate::state::event::MatchingEvent {
                book_id: OrderBookId::ZERO,
                orders,
            },
            crate::Timestamp::EPOCH,
            crate::state::StableMemoryOptions::Write,
        );
        assert!(state.has_pending_settling_events());

        let result = state.cancel_limit_order(&OWNER, buy_id, &mock_runtime_for(OWNER));

        assert_eq!(result, Err(CancelLimitOrderError::OrderAlreadyTerminal));
    }

    #[test]
    fn should_cancel_pending_fok_into_canceled_and_refund() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        let buy_id = order(OWNER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
            .fill_or_kill()
            .place(&mut state);
        assert_eq!(owner_status(&state, buy_id), Some(OrderStatus::Pending));

        assert_cancel_refunds(&mut state, OWNER, buy_id, PairToken::Quote, 100 * lot, lot);
    }

    #[test]
    fn should_reject_canceling_expired_fok() {
        use crate::state::CancelLimitOrderError;

        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        // FOK against an empty book: matching kills it, ending Expired.
        let buy_id = order(OWNER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
            .fill_or_kill()
            .place(&mut state);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
        assert_eq!(owner_status(&state, buy_id), Some(OrderStatus::Expired));

        let result = state.cancel_limit_order(&OWNER, buy_id, &mock_runtime_for(OWNER));

        assert_eq!(result, Err(CancelLimitOrderError::OrderAlreadyTerminal));
    }

    /// Cancels `order_id` owned by `user` and asserts that exactly
    /// `expected_amount` units of `refund_token` move from reserved to free;
    /// the other token's balance is unchanged and the order status becomes the
    /// unit `Canceled`, with remaining (`quantity − filled_quantity`) equal to
    /// `expected_remaining`.
    fn assert_cancel_refunds(
        state: &mut State<VectorMemory, VectorMemory>,
        user: Principal,
        order_id: OrderId,
        refund_token: PairToken,
        expected_amount: impl Into<Quantity>,
        expected_remaining: impl Into<Quantity>,
    ) {
        let mut runtime = MockRuntime::new();
        runtime.expect_time().return_const(crate::Timestamp::EPOCH);
        let expected_amount = expected_amount.into();
        let expected_remaining = expected_remaining.into();
        let pair = icp_ckbtc_trading_pair();
        let (base_before, quote_before) = balances_pair(state, &user, &pair);

        let order = state.cancel_limit_order(&user, order_id, &runtime).unwrap();
        assert_eq!(order.status, OrderStatus::Canceled);
        assert_eq!(
            order.quantity.checked_sub(order.filled_quantity),
            Some(expected_remaining),
            "remaining (quantity − filled_quantity) differed from expected",
        );

        let (base_after, quote_after) = balances_pair(state, &user, &pair);
        let persisted = state
            .get_user_order(&user, order_id)
            .map(|(_, _, record)| record.status);
        assert_eq!(persisted, Some(OrderStatus::Canceled));
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
                    .checked_sub(expected_amount)
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
        state
    }
}

mod record_limit_order {
    use crate::order::{FeeRates, OrderBookId, PendingOrder, Price, Side, TimeInForce};
    use crate::state::{StableMemoryOptions, State};
    use crate::test_fixtures::{
        self, LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, PRICE_SCALE, TICK_SIZE, ckbtc_metadata,
        icp_ckbtc_trading_pair, icp_metadata, order,
    };
    use candid::Principal;
    use ic_stable_structures::VectorMemory;

    const OWNER: Principal = Principal::from_slice(&[0x01]);

    fn setup() -> State<VectorMemory, VectorMemory> {
        let mut state = test_fixtures::state();
        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
        state
    }

    #[test]
    fn stores_the_submission_timestamp_on_the_record() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        state.deposit(OWNER, pair.base, lot.into(), StableMemoryOptions::Write);
        let (order_id, order) = state
            .validate_limit_order(
                OWNER,
                pair.clone(),
                PendingOrder {
                    side: Side::Sell,
                    price: Price::new(100 * PRICE_SCALE),
                    quantity: lot.into(),
                    time_in_force: TimeInForce::GoodTilCanceled,
                },
            )
            .unwrap();
        let timestamp = crate::Timestamp::new(1_700_000_000_000_000_000);

        state.record_limit_order(
            OWNER,
            order_id.book_id(),
            order,
            timestamp,
            StableMemoryOptions::Write,
        );

        assert_eq!(
            state.order_history.get(&order_id).unwrap().created_at,
            timestamp
        );
    }

    #[test]
    fn populates_the_per_user_index_newest_first() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        let first = order(OWNER, &pair, Side::Sell, 100, lot).place(&mut state);
        let second = order(OWNER, &pair, Side::Buy, 100, lot).place(&mut state);

        let owner_id = state.user_registry.lookup(OWNER).unwrap();
        assert_eq!(
            state.order_history.orders_after(owner_id, None, 10),
            Ok(vec![second, first])
        );
    }
}

mod get_user_orders {
    use crate::order::{CursorNotFound, FeeRates, OrderBookId, Side};
    use crate::state::State;
    use crate::test_fixtures::{
        self, LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, TICK_SIZE, ckbtc_metadata,
        icp_ckbtc_trading_pair, icp_metadata, order,
    };
    use candid::Principal;
    use ic_stable_structures::VectorMemory;

    const OWNER: Principal = Principal::from_slice(&[0x01]);

    fn setup() -> State<VectorMemory, VectorMemory> {
        let mut state = test_fixtures::state();
        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
        state
    }

    #[test]
    fn joins_pair_and_record_newest_first() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        let stranger = Principal::from_slice(&[0x02]);

        let first = order(OWNER, &pair, Side::Sell, 100, lot).place(&mut state);
        let second = order(OWNER, &pair, Side::Buy, 100, lot).place(&mut state);

        let orders = state.get_user_orders(&OWNER, None, 10).unwrap();
        let ids: Vec<_> = orders.iter().map(|(id, _, _)| *id).collect();
        assert_eq!(ids, vec![second, first], "newest first");
        for (_, joined_pair, record) in &orders {
            assert_eq!(*joined_pair, pair, "each entry carries its trading pair");
            assert_eq!(record.owner, OWNER, "each record is owned by the caller");
        }

        // Cursor pagination: resume after the newest → the older order.
        assert_eq!(
            state
                .get_user_orders(&OWNER, Some(second), 10)
                .unwrap()
                .into_iter()
                .map(|(id, _, _)| id)
                .collect::<Vec<_>>(),
            vec![first]
        );
        // A caller with no orders and no cursor sees an empty page.
        assert!(
            state
                .get_user_orders(&stranger, None, 10)
                .unwrap()
                .is_empty()
        );
        // A cursor that names an order the caller does not own is not found —
        // whether the caller has no orders at all...
        assert_eq!(
            state.get_user_orders(&stranger, Some(first), 10),
            Err(CursorNotFound)
        );
        // ...or the cursor is simply foreign to the (registered) caller.
        let owned_by_stranger = order(stranger, &pair, Side::Buy, 100, lot).place(&mut state);
        assert_eq!(
            state.get_user_orders(&OWNER, Some(owned_by_stranger), 10),
            Err(CursorNotFound)
        );
        // The oldest order is a valid cursor with no older orders: Ok([]).
        assert!(
            state
                .get_user_orders(&OWNER, Some(first), 10)
                .unwrap()
                .is_empty()
        );
    }
}

mod validate_overflow_invariant {
    use crate::order::{FeeRates, OrderBookId, PendingOrder, Price, Quantity, TimeInForce};
    use crate::state::AddLimitOrderError;
    use crate::test_fixtures;
    use crate::test_fixtures::arbitrary::arb_side;
    use crate::test_fixtures::{
        LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair,
        icp_metadata,
    };
    use candid::Principal;
    use proptest::prelude::{Strategy, any};
    use proptest::{prop_assert_eq, proptest};

    fn arb_tick_aligned_price() -> impl Strategy<Value = Price> {
        let tick = TICK_SIZE.get();
        (1u128..=u128::MAX / tick).prop_map(move |ticks| Price::new(ticks * tick))
    }

    fn arb_lot_aligned_quantity() -> impl Strategy<Value = Quantity> {
        let lot = LOT_SIZE.get();
        (any::<u128>(), any::<u128>()).prop_map(move |(high, low)| {
            let raw = Quantity::new(high, low);
            let (_, remainder) = raw.checked_div_rem_u64(lot).unwrap();
            raw.checked_sub(Quantity::from_u128(remainder as u128))
                .unwrap()
        })
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
            price in arb_tick_aligned_price(),
            quantity in arb_lot_aligned_quantity(),
            side in arb_side(),
        ) {
            let mut state = test_fixtures::state();
            let pair = icp_ckbtc_trading_pair();
            state.record_trading_pair(
                OrderBookId::ZERO,
                pair.clone(),
                icp_metadata(),
                ckbtc_metadata(),
                TICK_SIZE,
                LOT_SIZE,
                MIN_NOTIONAL,
                Some(MAX_NOTIONAL),
                FeeRates::default(),
            );

            let fits = price
                .checked_mul_quantity_scaled(&quantity, state.base_scale(&pair.base))
                .is_some();

            let result = state.validate_limit_order(
                Principal::from_slice(&[0x01]),
                pair,
                PendingOrder {
                    side,
                    price,
                    quantity,
                    time_in_force: TimeInForce::GoodTilCanceled,
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

mod validate_limit_order {
    use crate::order::{FeeRates, OrderBookId, PendingOrder, Price, Quantity, Side, TimeInForce};
    use crate::state::AddLimitOrderError;
    use crate::state::State;
    use crate::test_fixtures;
    use crate::test_fixtures::{
        LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata,
    };
    use assert_matches::assert_matches;
    use candid::Principal;
    use ic_stable_structures::VectorMemory;

    const USER: Principal = Principal::from_slice(&[0x01]);

    /// Registers ICP/ckBTC with the given notional bounds. With `TICK_SIZE`,
    /// `LOT_SIZE` and 8 base decimals, an order of `t` ticks and `q` lots has
    /// notional `price × quantity / 10^8 = t × q` quote units, so notionals
    /// are easy to dial in.
    fn state_with_notional_bounds(
        min_notional: Quantity,
        max_notional: Option<Quantity>,
    ) -> State<VectorMemory, VectorMemory> {
        let mut state = test_fixtures::state();
        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
            min_notional,
            max_notional,
            FeeRates::default(),
        );
        state
    }

    fn validate(
        state: &State<VectorMemory, VectorMemory>,
        ticks: u128,
        lots: u64,
    ) -> Result<(), AddLimitOrderError> {
        let pending = PendingOrder {
            side: Side::Buy,
            price: Price::new(ticks * TICK_SIZE.get()),
            quantity: Quantity::from(lots * LOT_SIZE.get()),
            time_in_force: TimeInForce::GoodTilCanceled,
        };
        state
            .validate_limit_order(USER, icp_ckbtc_trading_pair(), pending)
            .map(|_| ())
    }

    #[test]
    fn should_reject_notional_below_min() {
        let state =
            state_with_notional_bounds(Quantity::from_u128(5), Some(Quantity::from_u128(8)));
        assert_eq!(
            validate(&state, 2, 2),
            Err(AddLimitOrderError::InvalidNotional {
                notional: Quantity::from_u128(4),
                min: Quantity::from_u128(5),
                max: Some(Quantity::from_u128(8)),
            })
        );
    }

    #[test]
    fn should_reject_notional_above_max() {
        let state =
            state_with_notional_bounds(Quantity::from_u128(5), Some(Quantity::from_u128(8)));
        assert_eq!(
            validate(&state, 9, 1),
            Err(AddLimitOrderError::InvalidNotional {
                notional: Quantity::from_u128(9),
                min: Quantity::from_u128(5),
                max: Some(Quantity::from_u128(8)),
            })
        );
    }

    #[test]
    fn should_accept_notional_equal_to_min() {
        let state =
            state_with_notional_bounds(Quantity::from_u128(5), Some(Quantity::from_u128(8)));
        // notional == 5 == min: not an `InvalidNotional` rejection.
        assert_matches!(
            validate(&state, 5, 1),
            Err(AddLimitOrderError::InsufficientBalance { .. })
        );
    }

    #[test]
    fn should_accept_notional_equal_to_max() {
        let state =
            state_with_notional_bounds(Quantity::from_u128(5), Some(Quantity::from_u128(8)));
        assert_matches!(
            validate(&state, 8, 1),
            Err(AddLimitOrderError::InsufficientBalance { .. })
        );
    }

    #[test]
    fn should_not_enforce_upper_bound_when_max_unset() {
        let state = state_with_notional_bounds(Quantity::from_u128(5), None);
        assert_matches!(
            validate(&state, 1_000, 1_000),
            Err(AddLimitOrderError::InsufficientBalance { .. })
        );
    }

    #[test]
    fn should_enforce_tick_lot_and_notional_independently() {
        let state =
            state_with_notional_bounds(Quantity::from_u128(5), Some(Quantity::from_u128(8)));

        assert_eq!(
            validate(&state, 2, 2),
            Err(AddLimitOrderError::InvalidNotional {
                notional: Quantity::from_u128(4),
                min: Quantity::from_u128(5),
                max: Some(Quantity::from_u128(8)),
            })
        );

        // Off tick (price not a multiple of TICK_SIZE) but notional within
        // bounds: rejected for tick, not notional.
        let off_tick = PendingOrder {
            side: Side::Buy,
            price: Price::new(TICK_SIZE.get() + 1),
            quantity: Quantity::from(6 * LOT_SIZE.get()),
            time_in_force: TimeInForce::GoodTilCanceled,
        };
        assert_matches!(
            state.validate_limit_order(USER, icp_ckbtc_trading_pair(), off_tick),
            Err(AddLimitOrderError::InvalidOrder(_))
        );
    }
}

mod settle_fills {
    use crate::EXECUTOR;
    use crate::balance::Balance;
    use crate::order::{FeeRates, OrderBookId, Price, Quantity, Side};
    use crate::state::State;
    use crate::test_fixtures;
    use crate::test_fixtures::mocks::{self, mock_runtime_for};
    use crate::test_fixtures::{
        LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, PRICE_SCALE, TICK_SIZE, ckbtc_metadata,
        icp_ckbtc_trading_pair, icp_metadata,
    };
    use candid::Principal;
    use ic_stable_structures::VectorMemory;
    use std::collections::BTreeMap;

    type TestState = State<VectorMemory, VectorMemory>;

    const BUYER: Principal = Principal::from_slice(&[0x01]);
    const SELLER: Principal = Principal::from_slice(&[0x02]);

    #[test]
    fn should_settle_exact_match_at_same_price() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        let price = 100u128;

        test_fixtures::order(BUYER, &pair, Side::Buy, price * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, price * PRICE_SCALE, lot).place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        assert_eq!(buyer_base, balance(lot, 0u64));
        assert_eq!(buyer_quote, balance(0u64, 0u64));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0u64, 0u64));
        assert_eq!(seller_quote, balance(price * lot, 0u64));

        assert_token_conservation(&state, &totals_before);
    }

    /// Realistic asymmetric-decimal pair ckETH(18)/ckUSDC(6): buying 0.5 ETH at
    /// 3000.50 USDC/ETH must settle to exactly 1500.25 USDC — the rate that
    /// rounds to zero under the old per-base-unit price representation.
    #[test]
    fn should_settle_realistic_cketh_ckusdc_fill() {
        use crate::order::{LotSize, TickSize, TokenId, TokenMetadata, TradingPair};
        use std::num::{NonZeroU64, NonZeroU128};

        let mut state = test_fixtures::state();
        let pair = TradingPair {
            base: TokenId::new(Principal::from_slice(&[0xee])),
            quote: TokenId::new(Principal::from_slice(&[0xdc])),
        };
        // tick = $0.01/ETH = 0.01 × 10^6 = 10_000; lot = 0.0001 ETH = 10^14.
        // tick × lot = 10^18 = 10^base_decimals → every fill settles exactly.
        state.record_trading_pair(
            OrderBookId::ZERO,
            pair.clone(),
            TokenMetadata {
                symbol: "ckETH".to_string(),
                decimals: 18,
            },
            TokenMetadata {
                symbol: "ckUSDC".to_string(),
                decimals: 6,
            },
            TickSize::new(NonZeroU128::new(10_000).unwrap()),
            LotSize::new(NonZeroU64::new(100_000_000_000_000).unwrap()),
            MIN_NOTIONAL,
            None,
            FeeRates::default(),
        );

        // 3000.50 USDC/ETH × 10^6 = 3_000_500_000 quote units per whole ETH.
        let price = 3_000_500_000u128;
        // 0.5 ETH = 5 × 10^17 wei.
        let quantity = 500_000_000_000_000_000u128;
        test_fixtures::order(BUYER, &pair, Side::Buy, price, quantity).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, price, quantity).place(&mut state);

        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        // Buyer receives 0.5 ETH; seller receives exactly 1500.25 USDC.
        assert_eq!(
            state.get_balance(&BUYER, &pair.base),
            balance(quantity, 0u64)
        );
        assert_eq!(state.get_balance(&BUYER, &pair.quote), balance(0u64, 0u64));
        assert_eq!(state.get_balance(&SELLER, &pair.base), balance(0u64, 0u64));
        assert_eq!(
            state.get_balance(&SELLER, &pair.quote),
            balance(1_500_250_000u64, 0u64)
        );
        assert_token_conservation(&state, &totals_before);
    }

    /// ckBTC(8)/18-decimal-stablecoin pair priced at 6_000_000 whole quote per
    /// whole BTC: the per-whole-base price is 6 × 10^24, far above `u64::MAX`.
    /// The fill must settle to exactly `price × quantity / 10^8`, a quote amount
    /// that the old `u64` price representation could not even hold.
    #[test]
    fn should_settle_fill_with_price_exceeding_u64() {
        use crate::Timestamp;
        use crate::order::{
            LotSize, PendingOrder, TickSize, TimeInForce, TokenId, TokenMetadata, TradingPair,
        };
        use crate::state::StableMemoryOptions;
        use std::num::{NonZeroU64, NonZeroU128};

        let mut state = test_fixtures::state();
        let pair = TradingPair {
            base: TokenId::new(Principal::from_slice(&[0xbc])),
            quote: TokenId::new(Principal::from_slice(&[0x5b])),
        };
        // tick = 10^18, lot = 1; tick × lot = 10^18 is a multiple of
        // 10^base_decimals = 10^8, so every fill settles exactly.
        let tick = 1_000_000_000_000_000_000u128;
        state.record_trading_pair(
            OrderBookId::ZERO,
            pair.clone(),
            TokenMetadata {
                symbol: "ckBTC".to_string(),
                decimals: 8,
            },
            TokenMetadata {
                symbol: "ckUSD18".to_string(),
                decimals: 18,
            },
            TickSize::new(NonZeroU128::new(tick).unwrap()),
            LotSize::new(NonZeroU64::new(1).unwrap()),
            MIN_NOTIONAL,
            None,
            FeeRates::default(),
        );

        // 6_000_000 whole quote per whole BTC = 6_000_000 × 10^18 = 6 × 10^24,
        // a multiple of `tick` and well beyond u64::MAX (~1.8 × 10^19).
        let price = Price::new(6_000_000_000_000_000_000_000_000u128);
        assert!(price.get() > u64::MAX as u128);
        // 1 whole BTC = 10^8 base units.
        let quantity = Quantity::from(100_000_000u64);
        // Expected settled quote = price × quantity / 10^8 = 6 × 10^24.
        let expected_quote = Quantity::from_u128(6_000_000_000_000_000_000_000_000u128);

        let mut place = |user: Principal, side: Side, deposit_token, deposit_amount| {
            state.deposit(
                user,
                deposit_token,
                deposit_amount,
                StableMemoryOptions::Write,
            );
            let (order_id, order) = state
                .validate_limit_order(
                    user,
                    pair.clone(),
                    PendingOrder {
                        side,
                        price,
                        quantity,
                        time_in_force: TimeInForce::GoodTilCanceled,
                    },
                )
                .expect("validate_limit_order failed");
            state.record_limit_order(
                user,
                order_id.book_id(),
                order,
                Timestamp::EPOCH,
                StableMemoryOptions::Write,
            );
        };
        place(BUYER, Side::Buy, pair.quote, expected_quote);
        place(SELLER, Side::Sell, pair.base, quantity);

        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        // Buyer receives the full 1 BTC; seller receives exactly 6 × 10^24 quote.
        assert_eq!(
            state.get_balance(&BUYER, &pair.base),
            Balance::new(quantity, Quantity::ZERO)
        );
        assert_eq!(
            state.get_balance(&BUYER, &pair.quote),
            Balance::new(Quantity::ZERO, Quantity::ZERO)
        );
        assert_eq!(
            state.get_balance(&SELLER, &pair.base),
            Balance::new(Quantity::ZERO, Quantity::ZERO)
        );
        assert_eq!(
            state.get_balance(&SELLER, &pair.quote),
            Balance::new(expected_quote, Quantity::ZERO)
        );
    }

    #[test]
    fn should_unreserve_surplus_when_buy_taker_fills_at_lower_price() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        // Sell rests at 90, buy taker at 100 → fills at maker's 90
        test_fixtures::order(SELLER, &pair, Side::Sell, 90 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Buyer deposited 100*lot quote, paid 90*lot, surplus 10*lot returned to free
        assert_eq!(buyer_base, balance(lot, 0u64));
        assert_eq!(buyer_quote, balance(10 * lot, 0u64));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0u64, 0u64));
        assert_eq!(seller_quote, balance(90 * lot, 0u64));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_sell_taker_at_higher_maker_price() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        // Buy rests at 110, sell taker at 100 → fills at maker's 110
        test_fixtures::order(BUYER, &pair, Side::Buy, 110 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        assert_eq!(buyer_base, balance(lot, 0u64));
        assert_eq!(buyer_quote, balance(0u64, 0u64));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0u64, 0u64));
        // Seller gets 110*lot quote (better than their limit of 100)
        assert_eq!(seller_quote, balance(110 * lot, 0u64));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_partial_fill() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        // Buy 3 lots at 100, only 1 lot of sell available
        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, 3 * lot).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Buyer filled 1 lot, 2 lots remain reserved
        assert_eq!(buyer_base, balance(lot, 0u64));
        assert_eq!(buyer_quote, balance(0u64, 200 * lot));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0u64, 0u64));
        assert_eq!(seller_quote, balance(100 * lot, 0u64));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_multiple_fills_across_price_levels() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        // Two sells at different prices, buy taker sweeps both
        test_fixtures::order(SELLER, &pair, Side::Sell, 90 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, 2 * lot).place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Buyer deposited 100*2*lot = 200*lot quote
        // Paid 90*lot + 100*lot = 190*lot, surplus = 10*lot
        assert_eq!(buyer_base, balance(2 * lot, 0u64));
        assert_eq!(buyer_quote, balance(10 * lot, 0u64));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_buy_taker_partial_fill_with_price_improvement() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        // Sell rests at 90 for 1 lot, buy taker at 100 for 3 lots
        // Fills 1 lot at 90, rests 2 lots
        test_fixtures::order(SELLER, &pair, Side::Sell, 90 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, 3 * lot).place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Surplus: (100-90)*lot = 10*lot returned to free
        // Remaining reserved: 100*2*lot = 200*lot
        assert_eq!(buyer_base, balance(lot, 0u64));
        assert_eq!(buyer_quote, balance(10 * lot, 200 * lot));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0u64, 0u64));
        assert_eq!(seller_quote, balance(90 * lot, 0u64));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_sell_taker_partial_fill() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        // Buy rests 1 lot at 100, sell taker 3 lots at 100
        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, 3 * lot)
            .place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        assert_eq!(buyer_base, balance(lot, 0u64));
        assert_eq!(buyer_quote, balance(0u64, 0u64));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        // 1 lot filled, 2 lots remain reserved
        assert_eq!(seller_base, balance(0u64, 2 * lot));
        assert_eq!(seller_quote, balance(100 * lot, 0u64));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_sell_taker_multi_level_sweep() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        // Two buys at different prices, sell taker sweeps both
        // Sell at 100 matches buy at 110 first, then buy at 100
        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(BUYER, &pair, Side::Buy, 110 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, 2 * lot)
            .place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let buyer_base = state.get_balance(&BUYER, &pair.base);
        let buyer_quote = state.get_balance(&BUYER, &pair.quote);
        // Buyer deposited 100*lot + 110*lot = 210*lot quote, all consumed
        assert_eq!(buyer_base, balance(2 * lot, 0u64));
        assert_eq!(buyer_quote, balance(0u64, 0u64));

        let seller_base = state.get_balance(&SELLER, &pair.base);
        let seller_quote = state.get_balance(&SELLER, &pair.quote);
        assert_eq!(seller_base, balance(0u64, 0u64));
        // Seller receives 110*lot + 100*lot = 210*lot quote
        assert_eq!(seller_quote, balance(210 * lot, 0u64));

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_self_trade() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());
        let user = Principal::from_slice(&[0x42]);

        // Same user places both buy and sell
        test_fixtures::order(user, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(user, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);

        let base_before = state.get_balance(&user, &pair.base);
        let quote_before = state.get_balance(&user, &pair.quote);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
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
        assert_eq!(base_after, balance(lot, 0u64));
        assert_eq!(quote_after, balance(100 * lot, 0u64));
    }

    #[test]
    fn should_settle_taker_against_multiple_different_makers() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        let seller_a = Principal::from_slice(&[0x0A]);
        let seller_b = Principal::from_slice(&[0x0B]);

        // Two sellers place 1 lot each at different prices
        test_fixtures::order(seller_a, &pair, Side::Sell, 90 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(seller_b, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);

        // Buy taker sweeps both
        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, 2 * lot).place(&mut state);
        let participants = [BUYER, seller_a, seller_b];
        let totals_before = snapshot_balances(&state, &participants);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        // Buyer: received 2 lots, paid 90*lot + 100*lot, surplus 10*lot
        assert_eq!(
            state.get_balance(&BUYER, &pair.base),
            balance(2 * lot, 0u64)
        );
        assert_eq!(
            state.get_balance(&BUYER, &pair.quote),
            balance(10 * lot, 0u64)
        );

        // Seller A: sold 1 lot at 90
        assert_eq!(
            state.get_balance(&seller_a, &pair.base),
            balance(0u64, 0u64)
        );
        assert_eq!(
            state.get_balance(&seller_a, &pair.quote),
            balance(90 * lot, 0u64)
        );

        // Seller B: sold 1 lot at 100
        assert_eq!(
            state.get_balance(&seller_b, &pair.base),
            balance(0u64, 0u64)
        );
        assert_eq!(
            state.get_balance(&seller_b, &pair.quote),
            balance(100 * lot, 0u64)
        );

        assert_token_conservation(&state, &totals_before);
    }

    #[test]
    fn should_settle_trade_with_quantity_exceeding_u64_max() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let price = 100 * PRICE_SCALE;
        // quantity = LOT_SIZE * u64::MAX, guaranteed to be a valid lot multiple and > u64::MAX
        let quantity = Quantity::from(u64::from(LOT_SIZE))
            .checked_mul_u64(u64::MAX)
            .unwrap();

        test_fixtures::order(BUYER, &pair, Side::Buy, price, quantity).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, price, quantity).place(&mut state);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        let quote_total = Price::new(price)
            .checked_mul_quantity_scaled(&quantity, state.base_scale(&pair.base))
            .unwrap();

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
        use crate::order::{FeeRates, TokenId, TokenMetadata, TradingPair};

        let mut state = test_fixtures::state();
        let lot = u128::from(LOT_SIZE.get());
        let price = 100u128;

        // Pair A: ICP/ckBTC (book 0).
        let pair_a = icp_ckbtc_trading_pair();
        state.record_trading_pair(
            OrderBookId::ZERO,
            pair_a.clone(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );

        let buyer_a = Principal::from_slice(&[0x0A, 0x01]);
        let seller_a = Principal::from_slice(&[0x0A, 0x02]);
        let buyer_b = Principal::from_slice(&[0x0B, 0x01]);
        let seller_b = Principal::from_slice(&[0x0B, 0x02]);
        test_fixtures::order(buyer_a, &pair_a, Side::Buy, price * PRICE_SCALE, lot)
            .place(&mut state);
        test_fixtures::order(seller_a, &pair_a, Side::Sell, price * PRICE_SCALE, lot)
            .place(&mut state);
        test_fixtures::order(buyer_b, &pair_b, Side::Buy, price * PRICE_SCALE, lot)
            .place(&mut state);
        test_fixtures::order(seller_b, &pair_b, Side::Sell, price * PRICE_SCALE, lot)
            .place(&mut state);

        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        // Both books settled: both buyers hold their base free, both
        // sellers hold their quote free, no reserves left. If the second
        // book's SettlingEvent were silently dropped, buyer_b would
        // still have `price * lot` reserved and seller_b would still hold
        // `lot` reserved.
        assert_eq!(
            state.get_balance(&buyer_a, &pair_a.base),
            balance(lot, 0u64)
        );
        assert_eq!(
            state.get_balance(&seller_a, &pair_a.quote),
            balance(price * lot, 0u64),
        );
        assert_eq!(
            state.get_balance(&buyer_b, &pair_b.base),
            balance(lot, 0u64)
        );
        assert_eq!(
            state.get_balance(&seller_b, &pair_b.quote),
            balance(price * lot, 0u64),
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
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
        state
    }

    mod order_status {
        use super::*;
        use crate::order::{OrderRecord, OrderStatus, TimeInForce};

        fn status_of(
            state: &State<VectorMemory, VectorMemory>,
            owner: Principal,
            order_id: crate::order::OrderId,
        ) -> Option<OrderStatus> {
            state
                .get_user_order(&owner, order_id)
                .map(|(_, _, record)| record.status)
        }

        #[test]
        fn should_return_pending_before_matching() {
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            let buy_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
                .place(&mut state);

            assert_eq!(status_of(&state, BUYER, buy_id), Some(OrderStatus::Pending));
        }

        #[test]
        fn should_return_open_for_resting_order() {
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            let buy_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            assert_eq!(status_of(&state, BUYER, buy_id), Some(OrderStatus::Open));
        }

        #[test]
        fn should_return_filled_after_exact_match() {
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            let buy_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            let sell_id = test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            assert_eq!(status_of(&state, BUYER, buy_id), Some(OrderStatus::Filled));
            assert_eq!(
                status_of(&state, SELLER, sell_id),
                Some(OrderStatus::Filled)
            );
        }

        #[test]
        fn should_return_open_for_partially_filled_maker() {
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            // Sell 3 lots, buy only 1 → sell partially filled, remainder rests
            let sell_id =
                test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, 3 * lot)
                    .place(&mut state);
            let buy_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            // The maker rests `Open` with `0 < filled_quantity < quantity`.
            let sell = test_fixtures::record_of(&state, SELLER, sell_id);
            test_fixtures::assert_eq_ignoring_timestamp(
                &sell,
                &OrderRecord {
                    owner: SELLER,
                    side: Side::Sell,
                    price: Price::new(100 * PRICE_SCALE),
                    quantity: Quantity::from(3 * lot),
                    filled_quantity: Quantity::from(lot),
                    status: OrderStatus::Open,
                    created_at: sell.created_at,
                    last_updated_at: sell.last_updated_at,
                    time_in_force: TimeInForce::GoodTilCanceled,
                    filled_quote: Quantity::from(100 * lot),
                    filled_fee: Quantity::ZERO,
                },
            );
            assert_eq!(status_of(&state, BUYER, buy_id), Some(OrderStatus::Filled));
        }

        #[test]
        fn should_return_open_for_partially_filled_taker() {
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            // Sell 1 lot, buy 3 lots → buy partially fills and rests with 2 remaining
            let sell_id = test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            let buy_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, 3 * lot)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            // The fully consumed maker reports `filled_quantity == quantity`.
            let sell = test_fixtures::record_of(&state, SELLER, sell_id);
            test_fixtures::assert_eq_ignoring_timestamp(
                &sell,
                &OrderRecord {
                    owner: SELLER,
                    side: Side::Sell,
                    price: Price::new(100 * PRICE_SCALE),
                    quantity: Quantity::from(lot),
                    filled_quantity: Quantity::from(lot),
                    status: OrderStatus::Filled,
                    created_at: sell.created_at,
                    last_updated_at: sell.last_updated_at,
                    time_in_force: TimeInForce::GoodTilCanceled,
                    filled_quote: Quantity::from(100 * lot),
                    filled_fee: Quantity::ZERO,
                },
            );
            // The taker rests `Open` with one of three lots filled.
            let buy = test_fixtures::record_of(&state, BUYER, buy_id);
            test_fixtures::assert_eq_ignoring_timestamp(
                &buy,
                &OrderRecord {
                    owner: BUYER,
                    side: Side::Buy,
                    price: Price::new(100 * PRICE_SCALE),
                    quantity: Quantity::from(3 * lot),
                    filled_quantity: Quantity::from(lot),
                    status: OrderStatus::Open,
                    created_at: buy.created_at,
                    last_updated_at: buy.last_updated_at,
                    time_in_force: TimeInForce::GoodTilCanceled,
                    filled_quote: Quantity::from(100 * lot),
                    filled_fee: Quantity::ZERO,
                },
            );
        }

        #[test]
        fn should_return_filled_after_multi_fill_maker_depletion() {
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            // Sell rests with 2 lots; two successive buys deplete it
            let sell_id =
                test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, 2 * lot)
                    .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
            assert_eq!(status_of(&state, SELLER, sell_id), Some(OrderStatus::Open));

            let buy1_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
            // Across two batches the maker accrued both fills, one write
            // per batch, and now sits at one of two lots filled, still `Open`.
            let sell = test_fixtures::record_of(&state, SELLER, sell_id);
            assert_eq!(sell.status, OrderStatus::Open);
            assert_eq!(sell.filled_quantity, Quantity::from(lot));
            assert_eq!(status_of(&state, BUYER, buy1_id), Some(OrderStatus::Filled));

            let buy2_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
            let sell = test_fixtures::record_of(&state, SELLER, sell_id);
            assert_eq!(sell.status, OrderStatus::Filled);
            assert_eq!(sell.filled_quantity, sell.quantity);
            assert_eq!(status_of(&state, BUYER, buy2_id), Some(OrderStatus::Filled));
        }

        /// A taker that sweeps several makers in one batch accrues all its
        /// per-fill deltas into a single record write — its `filled_quantity`
        /// equals the sum of the fills and the order reaches `Filled`.
        #[test]
        fn should_write_once_for_taker_spanning_multiple_fills() {
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            // Two resting makers at different prices; one crossing taker sweeps
            // both in a single matching batch (two `Fill`s for the taker seq).
            test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            test_fixtures::order(SELLER, &pair, Side::Sell, 101 * PRICE_SCALE, lot)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            let buy_id = test_fixtures::order(BUYER, &pair, Side::Buy, 101 * PRICE_SCALE, 2 * lot)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            let buy = test_fixtures::record_of(&state, BUYER, buy_id);
            test_fixtures::assert_eq_ignoring_timestamp(
                &buy,
                &OrderRecord {
                    owner: BUYER,
                    side: Side::Buy,
                    price: Price::new(101 * PRICE_SCALE),
                    quantity: Quantity::from(2 * lot),
                    filled_quantity: Quantity::from(2 * lot),
                    status: OrderStatus::Filled,
                    created_at: buy.created_at,
                    last_updated_at: buy.last_updated_at,
                    time_in_force: TimeInForce::GoodTilCanceled,
                    filled_quote: Quantity::from(100 * lot + 101 * lot),
                    filled_fee: Quantity::ZERO,
                },
            );
        }

        /// `created_at` is stamped once at placement and never moves, while
        /// `last_updated_at` advances to the timestamp of the most recent
        /// modifying event. Distinct timestamps pin which event's time is kept.
        #[test]
        fn created_at_is_stable_while_last_updated_at_advances() {
            use crate::Timestamp;
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            // order(...).place() stamps created_at at Timestamp::EPOCH (0).
            let sell_id =
                test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, 3 * lot)
                    .place(&mut state);
            let placed = test_fixtures::record_of(&state, SELLER, sell_id);
            assert_eq!(placed.created_at, Timestamp::EPOCH);
            assert_eq!(placed.last_updated_at, None);

            // First fill at t = 100: partial, maker stays Open.
            test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
            EXECUTOR.run_once(
                &mut state,
                &mocks::mock_runtime_at(BUYER, Timestamp::new(100)),
            );
            let after_first = test_fixtures::record_of(&state, SELLER, sell_id);
            assert_eq!(after_first.created_at, Timestamp::EPOCH);
            assert_eq!(after_first.last_updated_at, Some(Timestamp::new(100)));
            assert_eq!(after_first.filled_quantity, Quantity::from(lot));

            // Second fill at t = 200: last_updated_at advances, created_at holds.
            test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, 2 * lot)
                .place(&mut state);
            EXECUTOR.run_once(
                &mut state,
                &mocks::mock_runtime_at(BUYER, Timestamp::new(200)),
            );
            let after_second = test_fixtures::record_of(&state, SELLER, sell_id);
            assert_eq!(after_second.created_at, Timestamp::EPOCH);
            assert_eq!(after_second.last_updated_at, Some(Timestamp::new(200)));
            assert_eq!(after_second.status, OrderStatus::Filled);
            assert_eq!(after_second.filled_quantity, after_second.quantity);
        }

        /// Durability: a matching event applied under `Skip` (the
        /// post-upgrade replay mode, since stable memory already holds the
        /// post-fill record) must not touch order history — `filled_quantity`
        /// and `last_updated_at` stay at their pre-event values, so replay
        /// never double-counts a fill.
        #[test]
        fn replay_under_skip_does_not_write_history() {
            use crate::state::StableMemoryOptions;
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            // Two crossing orders left pending on the book; history holds them
            // at `Pending` with `filled_quantity == 0`.
            test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            let buy_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
                .place(&mut state);
            let orders: Vec<_> = state
                .order_book(&OrderBookId::ZERO)
                .unwrap()
                .pending_order_seqs()
                .collect();

            // Matching under Skip consumes the book's pending orders (producing
            // fills) but must leave order history untouched.
            state.record_matching_event(
                &crate::state::event::MatchingEvent {
                    book_id: OrderBookId::ZERO,
                    orders,
                },
                crate::Timestamp::new(999),
                StableMemoryOptions::Skip,
            );

            let buy = test_fixtures::record_of(&state, BUYER, buy_id);
            test_fixtures::assert_eq_ignoring_timestamp(
                &buy,
                &OrderRecord {
                    owner: BUYER,
                    side: Side::Buy,
                    price: Price::new(100 * PRICE_SCALE),
                    quantity: Quantity::from(lot),
                    filled_quantity: Quantity::ZERO,
                    status: OrderStatus::Pending,
                    created_at: buy.created_at,
                    last_updated_at: buy.last_updated_at,
                    time_in_force: TimeInForce::GoodTilCanceled,
                    filled_quote: Quantity::ZERO,
                    filled_fee: Quantity::ZERO,
                },
            );
            assert_eq!(buy.last_updated_at, None);
        }
    }

    mod fees {
        use super::*;
        use crate::Timestamp;
        use crate::order::{
            BasisPoint, OrderRecord, OrderStatus, PairToken, TimeInForce, TradeRecord,
        };
        use crate::state::StableMemoryOptions;

        /// Fill deducts fees on both sides at the role-specific rates.
        /// Parameterized over which side crosses (taker):
        /// - buyer's base fee uses the buyer's role rate
        /// - seller's quote fee uses the seller's role rate
        ///
        /// where "role rate" is `taker_bps` for the crossing side and
        /// `maker_bps` for the resting side.
        #[test]
        fn buy_taker_fill_deducts_fees_on_both_sides() {
            fill_deducts_fees_on_both_sides(Side::Buy);
        }

        #[test]
        fn sell_taker_fill_deducts_fees_on_both_sides() {
            fill_deducts_fees_on_both_sides(Side::Sell);
        }

        fn fill_deducts_fees_on_both_sides(taker_side: Side) {
            let maker_bps = 10; // 0.1 %
            let taker_bps = 25; // 0.25 %
            let mut state = setup_with_fees(maker_bps, taker_bps);
            let pair = icp_ckbtc_trading_pair();
            let price = 100u128;
            // qty chosen so the two fees are exact (no ceiling rounding).
            let qty = u128::from(LOT_SIZE.get()) * 1_000_000;

            // Maker rests first, taker crosses. SELLER always sells, BUYER
            // always buys.
            let (first_side, second_side) = match taker_side {
                Side::Buy => (Side::Sell, Side::Buy),
                Side::Sell => (Side::Buy, Side::Sell),
            };
            let (first_user, second_user) = match first_side {
                Side::Sell => (SELLER, BUYER),
                Side::Buy => (BUYER, SELLER),
            };
            let first_id =
                test_fixtures::order(first_user, &pair, first_side, price * PRICE_SCALE, qty)
                    .place(&mut state);
            let second_id =
                test_fixtures::order(second_user, &pair, second_side, price * PRICE_SCALE, qty)
                    .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            let notional = price * qty;
            // Buyer pays the role rate of whoever crossed on the buy side;
            // same for seller on the sell side.
            let (buyer_role_bps, seller_role_bps) = match taker_side {
                Side::Buy => (taker_bps, maker_bps),
                Side::Sell => (maker_bps, taker_bps),
            };
            let base_fee_num = qty * buyer_role_bps as u128;
            let quote_fee_num = notional * seller_role_bps as u128;
            let base_fee = base_fee_num / 10_000;
            let quote_fee = quote_fee_num / 10_000;
            // Workload picks qty/price so the fees are exact (no ceiling
            // rounding) and strictly positive — keeps the equality
            // assertions below tight.
            assert_eq!(base_fee_num % 10_000, 0, "base fee should be exact");
            assert_eq!(quote_fee_num % 10_000, 0, "quote fee should be exact");
            assert!(base_fee > 0, "base fee should be > 0");
            assert!(quote_fee > 0, "quote fee should be > 0");

            assert_eq!(
                state.get_balance(&BUYER, &pair.base),
                balance(qty - base_fee, 0u64),
            );
            assert_eq!(
                state.get_balance(&SELLER, &pair.quote),
                balance(notional - quote_fee, 0u64),
            );
            assert_eq!(
                state.balances.fee_balance(&pair.base),
                Some(Quantity::from(base_fee)),
            );
            assert_eq!(
                state.balances.fee_balance(&pair.quote),
                Some(Quantity::from(quote_fee)),
            );

            let (buyer_id, seller_id) = match taker_side {
                Side::Buy => (second_id, first_id),
                Side::Sell => (first_id, second_id),
            };
            let buy = test_fixtures::record_of(&state, BUYER, buyer_id);
            test_fixtures::assert_eq_ignoring_timestamp(
                &buy,
                &OrderRecord {
                    owner: BUYER,
                    side: Side::Buy,
                    price: Price::new(price * PRICE_SCALE),
                    quantity: Quantity::from(qty),
                    filled_quantity: Quantity::from(qty),
                    status: OrderStatus::Filled,
                    created_at: buy.created_at,
                    last_updated_at: buy.last_updated_at,
                    time_in_force: TimeInForce::GoodTilCanceled,
                    filled_quote: Quantity::from(notional),
                    filled_fee: Quantity::from(base_fee),
                },
            );
            let sell = test_fixtures::record_of(&state, SELLER, seller_id);
            test_fixtures::assert_eq_ignoring_timestamp(
                &sell,
                &OrderRecord {
                    owner: SELLER,
                    side: Side::Sell,
                    price: Price::new(price * PRICE_SCALE),
                    quantity: Quantity::from(qty),
                    filled_quantity: Quantity::from(qty),
                    status: OrderStatus::Filled,
                    created_at: sell.created_at,
                    last_updated_at: sell.last_updated_at,
                    time_in_force: TimeInForce::GoodTilCanceled,
                    filled_quote: Quantity::from(notional),
                    filled_fee: Quantity::from(quote_fee),
                },
            );
        }

        /// Zero rates is a regression guard: the fill path with
        /// `FeeRates::default()` must produce no fee-pool entries on
        /// either side.
        #[test]
        fn zero_rates_create_no_fee_pool_entries() {
            let mut state = setup_with_fees(0, 0);
            let pair = icp_ckbtc_trading_pair();
            let price = 100u128;
            let qty = u128::from(LOT_SIZE.get());

            test_fixtures::order(SELLER, &pair, Side::Sell, price * PRICE_SCALE, qty)
                .place(&mut state);
            test_fixtures::order(BUYER, &pair, Side::Buy, price * PRICE_SCALE, qty)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            assert_eq!(state.get_balance(&BUYER, &pair.base), balance(qty, 0u64));
            assert_eq!(
                state.get_balance(&SELLER, &pair.quote),
                balance(price * qty, 0u64),
            );
            assert_eq!(state.balances.fee_balance(&pair.base), None);
            assert_eq!(state.balances.fee_balance(&pair.quote), None);
        }

        /// Buy-taker price improvement and fees co-exist: the maker-price
        /// quote_fee comes off the seller's credit, the `price_diff × qty`
        /// surplus is still refunded to the buyer's free balance, and the
        /// base_fee comes off the buyer's credit at the taker rate.
        #[test]
        fn price_improvement_refund_coexists_with_fees() {
            let maker_bps = 10;
            let taker_bps = 25;
            let mut state = setup_with_fees(maker_bps, taker_bps);
            let pair = icp_ckbtc_trading_pair();
            let qty = u64::from(LOT_SIZE) * 1_000_000;

            // Sell rests at 90, buy taker at 100 → fills at maker's 90.
            test_fixtures::order(SELLER, &pair, Side::Sell, 90 * PRICE_SCALE, qty)
                .place(&mut state);
            test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, qty).place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            let notional = 90u64 * qty;
            let base_fee = (qty as u128 * taker_bps as u128 / 10_000) as u64;
            let quote_fee = (notional as u128 * maker_bps as u128 / 10_000) as u64;

            // Buyer reserved 100*qty, paid 90*qty (notional) minus 0 (fee is
            // on base side), surplus of 10*qty returns to free.
            assert_eq!(
                state.get_balance(&BUYER, &pair.base),
                balance(qty - base_fee, 0u64),
            );
            assert_eq!(
                state.get_balance(&BUYER, &pair.quote),
                balance(10 * qty, 0u64),
            );
            assert_eq!(
                state.get_balance(&SELLER, &pair.quote),
                balance(notional - quote_fee, 0u64),
            );
        }

        /// Successive fills against the same pair accumulate deterministically
        /// into the per-token fee pool.
        #[test]
        fn multiple_fills_accumulate_into_fee_pool() {
            let taker_bps = 100; // 1 %
            let mut state = setup_with_fees(0, taker_bps);
            let pair = icp_ckbtc_trading_pair();
            let price = 100u128;
            let qty = u64::from(LOT_SIZE) * 1_000_000;

            // Two fills, each at qty.
            test_fixtures::order(SELLER, &pair, Side::Sell, price * PRICE_SCALE, qty)
                .place(&mut state);
            test_fixtures::order(BUYER, &pair, Side::Buy, price * PRICE_SCALE, qty)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
            test_fixtures::order(SELLER, &pair, Side::Sell, price * PRICE_SCALE, qty)
                .place(&mut state);
            test_fixtures::order(BUYER, &pair, Side::Buy, price * PRICE_SCALE, qty)
                .place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            let base_fee_per_fill = (qty as u128 * taker_bps as u128 / 10_000) as u64;
            // Buyer was taker on both fills (maker rate = 0, so quote pool is empty).
            assert_eq!(
                state.balances.fee_balance(&pair.base),
                Some(Quantity::from(2 * base_fee_per_fill)),
            );
            assert_eq!(state.balances.fee_balance(&pair.quote), None);
        }

        fn setup_with_fees(maker_bps: u16, taker_bps: u16) -> TestState {
            let fee_rates = FeeRates {
                maker: BasisPoint::new(maker_bps).unwrap(),
                taker: BasisPoint::new(taker_bps).unwrap(),
            };
            let mut state = test_fixtures::state();
            let pair = icp_ckbtc_trading_pair();
            state.record_trading_pair(
                OrderBookId::ZERO,
                pair,
                icp_metadata(),
                ckbtc_metadata(),
                TICK_SIZE,
                LOT_SIZE,
                MIN_NOTIONAL,
                Some(MAX_NOTIONAL),
                fee_rates,
            );
            state
        }

        // The worked-example test below registers ICP/ckUSDT (base ICP
        // 8 decimals / quote ckUSDT 6 decimals, `base_scale = 10^8`) so the
        // smallest-unit figures match the DEFI-2901 spec's example literally:
        // `PRICE_10 = 10_000_000` is 10 ckUSDT/ICP and `notional 20_000_000`
        // is 20 ckUSDT.
        use crate::test_fixtures::icp_ckusdt_trading_pair;
        use crate::test_fixtures::tokens::SupportedTokens;

        // Maker B and the two distinct maker levels need a third principal.
        const MAKER_B: Principal = Principal::from_slice(&[0x03]);
        const PRICE_10: u128 = 10_000_000;
        const PRICE_11: u128 = 11_000_000;
        const PRICE_12: u128 = 12_000_000;
        const QTY_2: u128 = 200_000_000;
        const QTY_3: u128 = 300_000_000;
        const QTY_5: u128 = 500_000_000;

        fn setup_ckusdt_with_fees(maker_bps: u16, taker_bps: u16) -> TestState {
            let mut state = test_fixtures::state();
            state.record_trading_pair(
                OrderBookId::ZERO,
                icp_ckusdt_trading_pair(),
                icp_metadata(),
                SupportedTokens::CKUSDT.token_metadata().into(),
                TICK_SIZE,
                LOT_SIZE,
                MIN_NOTIONAL,
                Some(MAX_NOTIONAL),
                FeeRates {
                    maker: BasisPoint::new(maker_bps).unwrap(),
                    taker: BasisPoint::new(taker_bps).unwrap(),
                },
            );
            state
        }

        #[test]
        fn rolls_up_realized_quote_and_fee() {
            let test_cases = vec![
                TestCase {
                    desc: "buy taker sweeps two maker levels, surplus excluded".to_string(),
                    orders: vec![
                        PlacedOrder::new(SELLER, Side::Sell, PRICE_10, QTY_2).expect(Expect {
                            status: OrderStatus::Filled,
                            filled_quantity: QTY_2,
                            filled_quote: 20_000_000,
                            filled_fee: 10_000,
                            vwap: None,
                        }),
                        PlacedOrder::new(MAKER_B, Side::Sell, PRICE_11, QTY_3).expect(Expect {
                            status: OrderStatus::Filled,
                            filled_quantity: QTY_3,
                            filled_quote: 33_000_000,
                            filled_fee: 16_500,
                            vwap: None,
                        }),
                        PlacedOrder::new(BUYER, Side::Buy, PRICE_12, QTY_5).expect(Expect {
                            status: OrderStatus::Filled,
                            filled_quantity: QTY_5,
                            filled_quote: 53_000_000,
                            filled_fee: 500_000,
                            vwap: Some(10_600_000),
                        }),
                    ],
                },
                TestCase {
                    desc: "order is taker on entry then maker within one batch".to_string(),
                    orders: vec![
                        PlacedOrder::new(SELLER, Side::Sell, PRICE_10, QTY_2).expect(Expect {
                            status: OrderStatus::Filled,
                            filled_quantity: QTY_2,
                            filled_quote: 20_000_000,
                            filled_fee: 10_000,
                            vwap: None,
                        }),
                        PlacedOrder::new(BUYER, Side::Buy, PRICE_10, QTY_5).expect(Expect {
                            status: OrderStatus::Filled,
                            filled_quantity: QTY_5,
                            filled_quote: 50_000_000,
                            filled_fee: 200_000 + 150_000,
                            vwap: None,
                        }),
                        PlacedOrder::new(MAKER_B, Side::Sell, PRICE_10, QTY_3).expect(Expect {
                            status: OrderStatus::Filled,
                            filled_quantity: QTY_3,
                            filled_quote: 30_000_000,
                            filled_fee: 30_000,
                            vwap: None,
                        }),
                    ],
                },
            ];

            for case in test_cases {
                let mut state = setup_ckusdt_with_fees(5, 10);
                let pair = icp_ckusdt_trading_pair();
                let placed: Vec<_> = case
                    .orders
                    .iter()
                    .map(|order| {
                        let id = test_fixtures::order(
                            order.owner,
                            &pair,
                            order.side,
                            order.price,
                            order.quantity,
                        )
                        .place(&mut state);
                        (order, id)
                    })
                    .collect();
                EXECUTOR.run_once(&mut state, &mocks::mock_runtime_for_timer());

                let base_scale = 100_000_000u128;
                for (order, id) in placed {
                    let Some(expect) = &order.expect else {
                        continue;
                    };
                    let record = test_fixtures::record_of(&state, order.owner, id);
                    assert_eq!(record.status, expect.status, "BUG ({}): status", case.desc);
                    assert_eq!(
                        record.filled_quantity,
                        Quantity::from(expect.filled_quantity),
                        "BUG ({}): filled_quantity",
                        case.desc
                    );
                    assert_eq!(
                        record.filled_quote,
                        Quantity::from(expect.filled_quote),
                        "BUG ({}): filled_quote",
                        case.desc
                    );
                    assert_eq!(
                        record.filled_fee,
                        Quantity::from(expect.filled_fee),
                        "BUG ({}): filled_fee",
                        case.desc
                    );
                    if let Some(vwap) = expect.vwap {
                        let actual = record.filled_quote.as_u128().unwrap() * base_scale
                            / record.filled_quantity.as_u128().unwrap();
                        assert_eq!(actual, vwap, "BUG ({}): vwap", case.desc);
                    }
                }
            }
        }

        struct TestCase {
            desc: String,
            orders: Vec<PlacedOrder>,
        }

        struct PlacedOrder {
            owner: Principal,
            side: Side,
            price: u128,
            quantity: u128,
            expect: Option<Expect>,
        }

        impl PlacedOrder {
            fn new(owner: Principal, side: Side, price: u128, quantity: u128) -> Self {
                Self {
                    owner,
                    side,
                    price,
                    quantity,
                    expect: None,
                }
            }

            fn expect(mut self, expect: Expect) -> Self {
                self.expect = Some(expect);
                self
            }
        }

        struct Expect {
            status: OrderStatus,
            filled_quantity: u128,
            filled_quote: u128,
            filled_fee: u128,
            vwap: Option<u128>,
        }

        /// The two swept levels each persist a taker-leg and a maker-leg fill
        /// record at the maker's own execution price, with per-fill notional and
        /// the side-specific realized fee — the granular feed behind the
        /// order-level rollups asserted above.
        #[test]
        fn buy_taker_sweeping_two_levels_persists_per_fill_records() {
            let mut state = setup_ckusdt_with_fees(5, 10);
            let pair = icp_ckusdt_trading_pair();

            let maker_a =
                test_fixtures::order(SELLER, &pair, Side::Sell, PRICE_10, QTY_2).place(&mut state);
            let maker_b =
                test_fixtures::order(MAKER_B, &pair, Side::Sell, PRICE_11, QTY_3).place(&mut state);
            let taker =
                test_fixtures::order(BUYER, &pair, Side::Buy, PRICE_12, QTY_5).place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            let taker_fills = order_trades_of(&state, taker);
            assert_eq!(
                taker_fills,
                vec![
                    TradeRecord {
                        side: Side::Buy,
                        price: Price::new(PRICE_11),
                        quantity: Quantity::from(QTY_3),
                        notional: Quantity::from(33_000_000u128),
                        fee: Quantity::from(300_000u128),
                        fee_token: PairToken::Base,
                        is_maker: false,
                        timestamp: Timestamp::EPOCH,
                    },
                    TradeRecord {
                        side: Side::Buy,
                        price: Price::new(PRICE_10),
                        quantity: Quantity::from(QTY_2),
                        notional: Quantity::from(20_000_000u128),
                        fee: Quantity::from(200_000u128),
                        fee_token: PairToken::Base,
                        is_maker: false,
                        timestamp: Timestamp::EPOCH,
                    },
                ],
                "taker fills newest-first at the maker prices, never the taker's 12",
            );

            assert_eq!(
                order_trades_of(&state, maker_a),
                vec![TradeRecord {
                    side: Side::Sell,
                    price: Price::new(PRICE_10),
                    quantity: Quantity::from(QTY_2),
                    notional: Quantity::from(20_000_000u128),
                    fee: Quantity::from(10_000u128),
                    fee_token: PairToken::Quote,
                    is_maker: true,
                    timestamp: Timestamp::EPOCH,
                }],
                "maker A leg",
            );

            assert_eq!(
                order_trades_of(&state, maker_b),
                vec![TradeRecord {
                    side: Side::Sell,
                    price: Price::new(PRICE_11),
                    quantity: Quantity::from(QTY_3),
                    notional: Quantity::from(33_000_000u128),
                    fee: Quantity::from(16_500u128),
                    fee_token: PairToken::Quote,
                    is_maker: true,
                    timestamp: Timestamp::EPOCH,
                }],
                "maker B leg",
            );
        }

        /// A single order that crosses (taker leg) then rests and is hit (maker
        /// leg) records two fills with their own per-fill role — `is_maker`
        /// false then true — and side-specific rate.
        #[test]
        fn order_filling_both_ways_records_a_taker_and_a_maker_fill() {
            let mut state = setup_ckusdt_with_fees(5, 10);
            let pair = icp_ckusdt_trading_pair();

            test_fixtures::order(SELLER, &pair, Side::Sell, PRICE_10, QTY_2).place(&mut state);
            let pivot =
                test_fixtures::order(BUYER, &pair, Side::Buy, PRICE_10, QTY_5).place(&mut state);
            test_fixtures::order(MAKER_B, &pair, Side::Sell, PRICE_10, QTY_3).place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            assert_eq!(
                order_trades_of(&state, pivot),
                vec![
                    TradeRecord {
                        side: Side::Buy,
                        price: Price::new(PRICE_10),
                        quantity: Quantity::from(QTY_3),
                        notional: Quantity::from(30_000_000u128),
                        fee: Quantity::from(150_000u128),
                        fee_token: PairToken::Base,
                        is_maker: true,
                        timestamp: Timestamp::EPOCH,
                    },
                    TradeRecord {
                        side: Side::Buy,
                        price: Price::new(PRICE_10),
                        quantity: Quantity::from(QTY_2),
                        notional: Quantity::from(20_000_000u128),
                        fee: Quantity::from(200_000u128),
                        fee_token: PairToken::Base,
                        is_maker: false,
                        timestamp: Timestamp::EPOCH,
                    },
                ],
                "maker leg (rested, then hit) newest-first, then the taker leg (crossed on entry)",
            );
        }

        /// Settlement populates the account-wide `by_user` index: a user's fills
        /// span all their orders, newest-first, and stay scoped to their owner.
        #[test]
        fn settlement_indexes_account_wide_fills_newest_first() {
            let mut state = setup_ckusdt_with_fees(5, 10);
            let pair = icp_ckusdt_trading_pair();
            let maker_a =
                test_fixtures::order(SELLER, &pair, Side::Sell, PRICE_10, QTY_2).place(&mut state);
            let maker_b =
                test_fixtures::order(SELLER, &pair, Side::Sell, PRICE_10, QTY_2).place(&mut state);
            test_fixtures::order(BUYER, &pair, Side::Buy, PRICE_10, QTY_2).place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));
            let taker_2 =
                test_fixtures::order(BUYER, &pair, Side::Buy, PRICE_10, QTY_2).place(&mut state);
            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            let buyer_orders = account_trade_orders(&state, BUYER);
            assert_eq!(
                buyer_orders.len(),
                2,
                "buyer has one fill per taker order, newest-first",
            );
            assert_eq!(buyer_orders[0], taker_2, "newest taker fill first");

            let seller_orders = account_trade_orders(&state, SELLER);
            assert_eq!(
                seller_orders,
                vec![maker_b, maker_a],
                "seller sees only their own maker fills",
            );
        }

        /// Replay under `Skip` (post-upgrade) writes no fill records, so durable
        /// fills are not double-written.
        #[test]
        fn replay_under_skip_writes_no_fills() {
            let mut state = setup_ckusdt_with_fees(5, 10);
            let pair = icp_ckusdt_trading_pair();
            let maker_a =
                test_fixtures::order(SELLER, &pair, Side::Sell, PRICE_10, QTY_2).place(&mut state);
            let taker =
                test_fixtures::order(BUYER, &pair, Side::Buy, PRICE_10, QTY_2).place(&mut state);

            let event = crate::state::event::MatchingEvent {
                book_id: OrderBookId::ZERO,
                orders: vec![maker_a.seq(), taker.seq()],
            };
            state.record_matching_event(&event, Timestamp::EPOCH, StableMemoryOptions::Skip);

            assert_eq!(
                state
                    .trade_history
                    .trades_for_order(taker, None, 10)
                    .unwrap(),
                Vec::new(),
            );
            assert_eq!(
                state
                    .trade_history
                    .trades_for_order(maker_a, None, 10)
                    .unwrap(),
                Vec::new(),
            );
        }

        #[test]
        fn settling_event_under_skip_writes_no_fills_and_no_balances() {
            let pair = icp_ckusdt_trading_pair();

            let settling_event = {
                let mut state = setup_ckusdt_with_fees(5, 10);
                let maker = test_fixtures::order(SELLER, &pair, Side::Sell, PRICE_10, QTY_2)
                    .place(&mut state);
                let taker = test_fixtures::order(BUYER, &pair, Side::Buy, PRICE_10, QTY_2)
                    .place(&mut state);
                state.record_matching_event(
                    &crate::state::event::MatchingEvent {
                        book_id: OrderBookId::ZERO,
                        orders: vec![maker.seq(), taker.seq()],
                    },
                    Timestamp::EPOCH,
                    StableMemoryOptions::Write,
                );
                state
                    .take_next_pending_settling_event()
                    .expect("matching a full cross must produce a settling event")
            };
            assert!(
                !settling_event.fills.is_empty(),
                "the settling event must carry fills for the gate to be exercised",
            );
            assert!(
                !settling_event.balance_operations.is_empty(),
                "the settling event must carry balance operations for the gate to be exercised",
            );

            let prepare = || {
                let mut state = setup_ckusdt_with_fees(5, 10);
                let maker = test_fixtures::order(SELLER, &pair, Side::Sell, PRICE_10, QTY_2)
                    .place(&mut state);
                let taker = test_fixtures::order(BUYER, &pair, Side::Buy, PRICE_10, QTY_2)
                    .place(&mut state);
                state.record_matching_event(
                    &crate::state::event::MatchingEvent {
                        book_id: OrderBookId::ZERO,
                        orders: vec![maker.seq(), taker.seq()],
                    },
                    Timestamp::EPOCH,
                    StableMemoryOptions::Write,
                );
                let _ = state.take_next_pending_settling_event();
                (state, maker, taker)
            };

            let balances_of = |state: &TestState| {
                [
                    state.get_balance(&BUYER, &pair.base),
                    state.get_balance(&BUYER, &pair.quote),
                    state.get_balance(&SELLER, &pair.base),
                    state.get_balance(&SELLER, &pair.quote),
                ]
            };

            let (mut skip_state, skip_maker, skip_taker) = prepare();
            let balances_before = balances_of(&skip_state);
            skip_state.record_settling_event(
                &settling_event,
                Timestamp::EPOCH,
                StableMemoryOptions::Skip,
            );
            assert_eq!(
                order_trades_of(&skip_state, skip_taker),
                Vec::new(),
                "Skip replay must not write the taker's fill",
            );
            assert_eq!(
                order_trades_of(&skip_state, skip_maker),
                Vec::new(),
                "Skip replay must not write the maker's fill",
            );
            assert_eq!(
                balances_of(&skip_state),
                balances_before,
                "Skip replay must not move any balances",
            );

            let (mut write_state, write_maker, write_taker) = prepare();
            write_state.record_settling_event(
                &settling_event,
                Timestamp::EPOCH,
                StableMemoryOptions::Write,
            );
            assert_eq!(
                order_trades_of(&write_state, write_taker).len(),
                1,
                "Write must write the taker's fill exactly once",
            );
            assert_eq!(
                order_trades_of(&write_state, write_maker).len(),
                1,
                "Write must write the maker's fill exactly once",
            );
            assert_ne!(
                balances_of(&write_state),
                balances_before,
                "Write must move balances, proving the Skip case is a real no-op",
            );
        }

        use crate::order::{FeeRates, Quantity};
        use crate::state::event::{BalanceOperation, SettlingEvent};
        use crate::test_fixtures::arbitrary::{arb_fee_rates, arb_side};
        use proptest::prelude::Strategy;
        use proptest::{prop_assert_eq, proptest};

        fn arb_tick_aligned_price_in_notional_bounds() -> impl Strategy<Value = u128> {
            let tick = TICK_SIZE.get();
            (1u128..=1_000_000u128).prop_map(move |ticks| ticks * tick)
        }

        proptest! {
            /// Over arbitrary fills, each persisted `TradeRecord.fee`/`notional`
            /// equals the fee/amount of the `BalanceOperation` produced from the
            /// same settlement — the quote transfer's `amount` is the notional of
            /// both legs, its `fee` is the quote-side leg's fee, and the base
            /// transfer's `fee` is the base-side leg's fee. A future edit that
            /// lets the settle-time recompute drift from the match-time balance
            /// ops fails here.
            #[test]
            fn persisted_fee_and_notional_match_the_balance_operations(
                taker_side in arb_side(),
                price in arb_tick_aligned_price_in_notional_bounds(),
                quantity in arb_lot_aligned_quantity_in_bounds(),
                fee_rates in arb_fee_rates(),
            ) {
                let mut state = setup_ckusdt_with_fee_rates(fee_rates);
                let pair = icp_ckusdt_trading_pair();
                let (maker_side, maker_owner, taker_owner) = match taker_side {
                    Side::Buy => (Side::Sell, SELLER, BUYER),
                    Side::Sell => (Side::Buy, BUYER, SELLER),
                };

                let maker = test_fixtures::order(maker_owner, &pair, maker_side, price, quantity)
                    .place(&mut state);
                let taker = test_fixtures::order(taker_owner, &pair, taker_side, price, quantity)
                    .place(&mut state);

                let event = crate::state::event::MatchingEvent {
                    book_id: OrderBookId::ZERO,
                    orders: vec![maker.seq(), taker.seq()],
                };
                state.record_matching_event(&event, Timestamp::EPOCH, StableMemoryOptions::Write);
                let settling = state
                    .take_next_pending_settling_event()
                    .expect("a full cross must produce a settling event");
                state.record_settling_event(&settling, Timestamp::EPOCH, StableMemoryOptions::Write);

                let (quote_transfer_amount, quote_fee, base_fee) =
                    quote_and_base_transfer_fees(&settling);

                for order_id in [taker, maker] {
                    for leg in order_trades_of(&state, order_id) {
                        prop_assert_eq!(
                            leg.notional,
                            quote_transfer_amount,
                            "leg notional must equal the quote transfer amount",
                        );
                        let expected_fee = match leg.fee_token {
                            PairToken::Quote => quote_fee,
                            PairToken::Base => base_fee,
                        };
                        prop_assert_eq!(
                            leg.fee,
                            expected_fee,
                            "leg fee must equal the matching balance-op fee",
                        );
                    }
                }
            }
        }

        /// A fill snapshots the book's `fee_rates` onto its `FillEvent` at match
        /// time, so a fee-rate change between matching and settling must not move
        /// the persisted trade legs' fee — this is the whole reason `fee_rates`
        /// rides on the `FillEvent`.
        #[test]
        fn persisted_fee_reflects_match_time_rate_not_a_later_change() {
            let match_time = FeeRates {
                maker: BasisPoint::new(5).unwrap(),
                taker: BasisPoint::new(10).unwrap(),
            };
            let mut state = setup_ckusdt_with_fee_rates(match_time);
            let pair = icp_ckusdt_trading_pair();

            let maker =
                test_fixtures::order(SELLER, &pair, Side::Sell, PRICE_10, QTY_2).place(&mut state);
            let taker =
                test_fixtures::order(BUYER, &pair, Side::Buy, PRICE_10, QTY_2).place(&mut state);

            state.record_matching_event(
                &crate::state::event::MatchingEvent {
                    book_id: OrderBookId::ZERO,
                    orders: vec![maker.seq(), taker.seq()],
                },
                Timestamp::EPOCH,
                StableMemoryOptions::Write,
            );
            let settling = state
                .take_next_pending_settling_event()
                .expect("a full cross must produce a settling event");

            let settle_time = FeeRates {
                maker: BasisPoint::new(200).unwrap(),
                taker: BasisPoint::new(300).unwrap(),
            };
            assert_ne!(settle_time, match_time, "the rate must actually change");
            state.set_book_fee_rates(OrderBookId::ZERO, settle_time);

            state.record_settling_event(&settling, Timestamp::EPOCH, StableMemoryOptions::Write);

            assert_eq!(
                order_trades_of(&state, taker),
                vec![TradeRecord {
                    side: Side::Buy,
                    price: Price::new(PRICE_10),
                    quantity: Quantity::from(QTY_2),
                    notional: Quantity::from(20_000_000u128),
                    fee: Quantity::from(200_000u128),
                    fee_token: PairToken::Base,
                    is_maker: false,
                    timestamp: Timestamp::EPOCH,
                }],
                "taker fee at the 10 bps match-time taker rate, not the 300 bps settle-time rate",
            );
            assert_eq!(
                order_trades_of(&state, maker),
                vec![TradeRecord {
                    side: Side::Sell,
                    price: Price::new(PRICE_10),
                    quantity: Quantity::from(QTY_2),
                    notional: Quantity::from(20_000_000u128),
                    fee: Quantity::from(10_000u128),
                    fee_token: PairToken::Quote,
                    is_maker: true,
                    timestamp: Timestamp::EPOCH,
                }],
                "maker fee at the 5 bps match-time maker rate, not the 200 bps settle-time rate",
            );
        }

        fn setup_ckusdt_with_fee_rates(fee_rates: FeeRates) -> TestState {
            let mut state = test_fixtures::state();
            state.record_trading_pair(
                OrderBookId::ZERO,
                icp_ckusdt_trading_pair(),
                icp_metadata(),
                SupportedTokens::CKUSDT.token_metadata().into(),
                TICK_SIZE,
                LOT_SIZE,
                MIN_NOTIONAL,
                Some(MAX_NOTIONAL),
                fee_rates,
            );
            state
        }

        fn arb_lot_aligned_quantity_in_bounds() -> impl Strategy<Value = u128> {
            let lot = u128::from(LOT_SIZE.get());
            (1u128..=1_000u128).prop_map(move |lots| lots * lot)
        }

        /// The `(quote_transfer_amount, quote_fee, base_fee)` of the single fill's
        /// two `Transfer` operations: the quote-token transfer carries the
        /// notional as its `amount` and the quote-side fee, the base-token
        /// transfer carries the base-side fee.
        fn quote_and_base_transfer_fees(event: &SettlingEvent) -> (Quantity, Quantity, Quantity) {
            let mut quote = None;
            let mut base = None;
            for op in &event.balance_operations {
                if let BalanceOperation::Transfer {
                    token, amount, fee, ..
                } = op
                {
                    let fee = fee.unwrap_or(Quantity::ZERO);
                    match token {
                        PairToken::Quote => assert!(
                            quote.replace((*amount, fee)).is_none(),
                            "a fill must produce exactly one quote transfer",
                        ),
                        PairToken::Base => assert!(
                            base.replace(fee).is_none(),
                            "a fill must produce exactly one base transfer",
                        ),
                    }
                }
            }
            let (amount, quote_fee) = quote.expect("a fill must produce a quote transfer");
            let base_fee = base.expect("a fill must produce a base transfer");
            (amount, quote_fee, base_fee)
        }

        fn order_trades_of(
            state: &TestState,
            order_id: crate::order::OrderId,
        ) -> Vec<crate::order::TradeRecord> {
            state
                .trade_history
                .trades_for_order(order_id, None, 100)
                .expect("per-order fill read should not error")
                .into_iter()
                .map(|(_, record)| record)
                .collect()
        }

        fn account_trade_orders(state: &TestState, owner: Principal) -> Vec<crate::order::OrderId> {
            let user = state
                .user_registry
                .lookup(owner)
                .expect("owner should be registered after settlement");
            state
                .trade_history
                .trades_after(user, None, 100)
                .expect("account-wide fill read should not error")
                .into_iter()
                .map(|(id, _)| id.order_id())
                .collect()
        }
    }

    fn balance(free: impl Into<Quantity>, reserved: impl Into<Quantity>) -> Balance {
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

    mod fill_or_kill {
        use super::*;
        use crate::order::{BasisPoint, OrderBookSnapshot, OrderId, OrderSeq, OrderStatus};
        use crate::test_fixtures::tokens::SupportedTokens;
        use std::collections::BTreeSet;

        const MAKER: Principal = Principal::from_slice(&[42_u8]);
        const TAKER: Principal = Principal::from_slice(&[43_u8]);

        const MAKER_BPS: u16 = 10; // 0.1 %
        const TAKER_BPS: u16 = 25; // 0.25 %
        const ONE_ICP: u64 = SupportedTokens::ICP.one();

        #[test]
        fn should_fill() {
            let tests_cases = vec![
                TestCase {
                    desc: "buy fully crosses a single resting ask".to_string(),
                    bids: vec![],
                    asks: vec![(Price::from(4_000), vec![Quantity::from(ONE_ICP)])],
                    fok: (Side::Buy, Price::from(4_000), Quantity::from(ONE_ICP)),
                    expected_balances_taker: (
                        Balance::new_free(ONE_ICP - 250_000),
                        Balance::zero(),
                    ),
                    expected_balances_maker: (Balance::zero(), Balance::new_free(3_996_u64)),
                    expected_fee_balances: (
                        Some(Quantity::from(250_000_u64)),
                        Some(Quantity::from(4_u64)),
                    ),
                },
                TestCase {
                    desc: "sell fully crosses a single resting bid".to_string(),
                    bids: vec![(Price::from(4_000), vec![Quantity::from(ONE_ICP)])],
                    asks: vec![],
                    fok: (Side::Sell, Price::from(4_000), Quantity::from(ONE_ICP)),
                    expected_balances_taker: (Balance::zero(), Balance::new_free(3_990_u64)),
                    expected_balances_maker: (
                        Balance::new_free(ONE_ICP - 100_000),
                        Balance::zero(),
                    ),
                    expected_fee_balances: (
                        Some(Quantity::from(100_000_u64)),
                        Some(Quantity::from(10_u64)),
                    ),
                },
                TestCase {
                    desc: "buy above resting ask fills at maker price and refunds surplus"
                        .to_string(),
                    bids: vec![],
                    asks: vec![(Price::from(4_000), vec![Quantity::from(ONE_ICP)])],
                    fok: (Side::Buy, Price::from(5_000), Quantity::from(ONE_ICP)),
                    expected_balances_taker: (
                        Balance::new_free(ONE_ICP - 250_000),
                        Balance::new_free(1_000_u64),
                    ),
                    expected_balances_maker: (Balance::zero(), Balance::new_free(3_996_u64)),
                    expected_fee_balances: (
                        Some(Quantity::from(250_000_u64)),
                        Some(Quantity::from(4_u64)),
                    ),
                },
                TestCase {
                    desc: "buy sweeps several ascending ask levels".to_string(),
                    bids: vec![],
                    asks: vec![
                        (Price::from(4_000), vec![Quantity::from(ONE_ICP)]),
                        (Price::from(5_000), vec![Quantity::from(ONE_ICP)]),
                        (Price::from(6_000), vec![Quantity::from(ONE_ICP)]),
                    ],
                    fok: (Side::Buy, Price::from(6_000), Quantity::from(3 * ONE_ICP)),
                    expected_balances_taker: (
                        Balance::new_free(3 * ONE_ICP - 750_000),
                        Balance::new_free(3_000_u64),
                    ),
                    expected_balances_maker: (Balance::zero(), Balance::new_free(14_985_u64)),
                    expected_fee_balances: (
                        Some(Quantity::from(750_000_u64)),
                        Some(Quantity::from(15_u64)),
                    ),
                },
            ];
            for case in tests_cases {
                let mut state = state();
                let pair = icp_ckbtc_trading_pair();
                case.populate_book(&mut state);

                let fok_id = case.execute_fok(&mut state);

                let (_, _, fok_order) = state.get_user_order(&TAKER, fok_id).unwrap();
                assert_eq!(
                    fok_order.status,
                    OrderStatus::Filled,
                    "BUG ({}): a crossing FOK should end Filled",
                    case.desc
                );
                assert_eq!(
                    fok_order.filled_quantity, fok_order.quantity,
                    "BUG ({}): a filled FOK should be fully filled",
                    case.desc
                );

                let (resting_bids, resting_asks) = resting_levels(&state, &pair);
                assert!(
                    resting_bids.is_empty() && resting_asks.is_empty(),
                    "BUG ({}): the FOK is sized to consume every resting maker, so the book should end empty",
                    case.desc
                );

                let balances_taker = test_fixtures::balances_pair(&state, &TAKER, &pair);
                assert_eq!(
                    balances_taker, case.expected_balances_taker,
                    "BUG ({}): taker balances differ from expected after fill",
                    case.desc
                );
                let balances_maker = test_fixtures::balances_pair(&state, &MAKER, &pair);
                assert_eq!(
                    balances_maker, case.expected_balances_maker,
                    "BUG ({}): maker balances differ from expected after fill",
                    case.desc
                );

                let fee_balances = (
                    state.balances.fee_balance(&pair.base),
                    state.balances.fee_balance(&pair.quote),
                );
                assert_eq!(
                    fee_balances, case.expected_fee_balances,
                    "BUG ({}): fee balances differ from expected after fill",
                    case.desc
                );
            }
        }

        #[test]
        fn should_expire() {
            let tests_cases = vec![
                TestCase {
                    desc: "buy against empty book".to_string(),
                    bids: vec![],
                    asks: vec![],
                    fok: (Side::Buy, Price::from(4_000), Quantity::from(ONE_ICP)),
                    expected_balances_taker: (Balance::zero(), Balance::new_free(4_000_u64)),
                    expected_balances_maker: (Balance::zero(), Balance::zero()),
                    expected_fee_balances: (None, None),
                },
                TestCase {
                    desc: "sell against empty book".to_string(),
                    bids: vec![],
                    asks: vec![],
                    fok: (Side::Sell, Price::from(4_000), Quantity::from(ONE_ICP)),
                    expected_balances_taker: (Balance::new_free(ONE_ICP), Balance::zero()),
                    expected_balances_maker: (Balance::zero(), Balance::zero()),
                    expected_fee_balances: (None, None),
                },
                TestCase {
                    desc: "buy below resting ask (no cross)".to_string(),
                    bids: vec![],
                    asks: vec![(Price::from(5_000), vec![Quantity::from(ONE_ICP)])],
                    fok: (Side::Buy, Price::from(4_000), Quantity::from(ONE_ICP)),
                    expected_balances_taker: (Balance::zero(), Balance::new_free(4_000_u64)),
                    expected_balances_maker: (Balance::new_reserved(ONE_ICP), Balance::zero()),
                    expected_fee_balances: (None, None),
                },
                TestCase {
                    desc: "sell above resting bid (no cross)".to_string(),
                    bids: vec![(Price::from(3_000), vec![Quantity::from(ONE_ICP)])],
                    asks: vec![],
                    fok: (Side::Sell, Price::from(4_000), Quantity::from(ONE_ICP)),
                    expected_balances_taker: (Balance::new_free(ONE_ICP), Balance::zero()),
                    expected_balances_maker: (Balance::zero(), Balance::new_reserved(3_000_u64)),
                    expected_fee_balances: (None, None),
                },
                TestCase {
                    desc: "buy crosses but exceeds total resting ask".to_string(),
                    bids: vec![],
                    asks: vec![(Price::from(4_000), vec![Quantity::from(ONE_ICP)])],
                    fok: (Side::Buy, Price::from(4_000), Quantity::from(2 * ONE_ICP)),
                    expected_balances_taker: (Balance::zero(), Balance::new_free(8_000_u64)),
                    expected_balances_maker: (Balance::new_reserved(ONE_ICP), Balance::zero()),
                    expected_fee_balances: (None, None),
                },
                TestCase {
                    desc: "sell crosses but exceeds total resting bid".to_string(),
                    bids: vec![(Price::from(4_000), vec![Quantity::from(ONE_ICP)])],
                    asks: vec![],
                    fok: (Side::Sell, Price::from(4_000), Quantity::from(2 * ONE_ICP)),
                    expected_balances_taker: (Balance::new_free(2 * ONE_ICP), Balance::zero()),
                    expected_balances_maker: (Balance::zero(), Balance::new_reserved(4_000_u64)),
                    expected_fee_balances: (None, None),
                },
                TestCase {
                    desc: "buy crosses several resting orders but exceeds their total".to_string(),
                    bids: vec![],
                    asks: vec![(
                        Price::from(4_000),
                        vec![Quantity::from(ONE_ICP), Quantity::from(ONE_ICP)],
                    )],
                    fok: (Side::Buy, Price::from(4_000), Quantity::from(3 * ONE_ICP)),
                    expected_balances_taker: (Balance::zero(), Balance::new_free(12_000_u64)),
                    expected_balances_maker: (Balance::new_reserved(2 * ONE_ICP), Balance::zero()),
                    expected_fee_balances: (None, None),
                },
            ];
            for case in tests_cases {
                let mut state = state();
                let pair = icp_ckbtc_trading_pair();
                let book_before = case.populate_book(&mut state);

                let fok_id = case.execute_fok(&mut state);

                let mut book_after = OrderBookSnapshot::from(state.get_order_book(&pair).unwrap());
                assert_eq!(
                    book_after.next_seq,
                    OrderSeq::new(book_before.next_seq.get() + 1),
                    "BUG ({}): should add only one order between book snapshots",
                    case.desc
                );
                book_after.next_seq = book_before.next_seq;
                assert_eq!(
                    book_before, book_after,
                    "BUG ({}): book should be the same as before when a FOK order is killed (except for the next_seq increment).",
                    case.desc
                );

                let (_, _, fok_order) = state.get_user_order(&TAKER, fok_id).unwrap();
                assert_eq!(
                    fok_order.status,
                    OrderStatus::Expired,
                    "BUG ({}): killed FOK should end Expired",
                    case.desc
                );

                let balances_taker = test_fixtures::balances_pair(&state, &TAKER, &pair);
                assert_eq!(
                    balances_taker, case.expected_balances_taker,
                    "BUG ({}): taker balances differ from expected after kill",
                    case.desc
                );
                let balances_maker = test_fixtures::balances_pair(&state, &MAKER, &pair);
                assert_eq!(
                    balances_maker, case.expected_balances_maker,
                    "BUG ({}): maker balances differ from expected after kill",
                    case.desc
                );

                let fee_balances = (
                    state.balances.fee_balance(&pair.base),
                    state.balances.fee_balance(&pair.quote),
                );
                assert_eq!(
                    fee_balances, case.expected_fee_balances,
                    "BUG ({}): a killed FOK order should not change fee balances",
                    case.desc
                );
            }
        }

        struct TestCase {
            desc: String,
            bids: Vec<(Price, Vec<Quantity>)>,
            asks: Vec<(Price, Vec<Quantity>)>,
            fok: (Side, Price, Quantity),
            expected_balances_taker: (Balance, Balance),
            expected_balances_maker: (Balance, Balance),
            expected_fee_balances: (Option<Quantity>, Option<Quantity>),
        }

        impl TestCase {
            fn populate_book(&self, state: &mut TestState) -> OrderBookSnapshot {
                let pair = icp_ckbtc_trading_pair();
                let mut maker_orders = BTreeSet::default();

                for (price, quantities) in &self.bids {
                    for quantity in quantities {
                        let order_id =
                            test_fixtures::order(MAKER, &pair, Side::Buy, price.get(), *quantity)
                                .place(state);
                        assert!(
                            maker_orders.insert(order_id),
                            "BUG ({}): duplicate order ID",
                            self.desc
                        );
                    }
                }

                for (price, quantities) in &self.asks {
                    for quantity in quantities {
                        let order_id =
                            test_fixtures::order(MAKER, &pair, Side::Sell, price.get(), *quantity)
                                .place(state);
                        assert!(
                            maker_orders.insert(order_id),
                            "BUG ({}): duplicate order ID",
                            self.desc
                        );
                    }
                }

                EXECUTOR.run_once(state, &mocks::mock_runtime_for_timer());

                for maker_order in maker_orders {
                    let (_, _, order) = state.get_user_order(&MAKER, maker_order).unwrap();
                    assert_eq!(
                        order.status,
                        OrderStatus::Open,
                        "BUG (test setup, {}): maker order is not resting in the book",
                        self.desc
                    );
                }

                OrderBookSnapshot::from(state.get_order_book(&pair).unwrap())
            }

            fn execute_fok(&self, state: &mut TestState) -> OrderId {
                let (fok_side, fok_price, fok_quantity) = self.fok;

                let fok_id = test_fixtures::order(
                    TAKER,
                    &icp_ckbtc_trading_pair(),
                    fok_side,
                    fok_price.get(),
                    fok_quantity,
                )
                .fill_or_kill()
                .place(state);
                EXECUTOR.run_once(state, &mocks::mock_runtime_for_timer());
                fok_id
            }
        }

        fn state() -> TestState {
            let mut state = test_fixtures::state();
            let pair = icp_ckbtc_trading_pair();
            state.record_trading_pair(
                OrderBookId::ZERO,
                pair,
                icp_metadata(),
                ckbtc_metadata(),
                TICK_SIZE,
                LOT_SIZE,
                MIN_NOTIONAL,
                Some(MAX_NOTIONAL),
                FeeRates {
                    maker: BasisPoint::new(MAKER_BPS).unwrap(),
                    taker: BasisPoint::new(TAKER_BPS).unwrap(),
                },
            );
            state
        }

        /// A single `process_pending_orders` round carrying both a killed FOK
        /// and a GTC order: the GTC behaves normally (rests Open) and the FOK
        /// kill does not disturb it.
        #[test]
        fn should_process_killed_fok_and_gtc_in_same_round() {
            let mut state = setup();
            let lot = u128::from(LOT_SIZE.get());
            let pair = icp_ckbtc_trading_pair();
            // FOK Buy against an empty book — it will be killed. A GTC Sell that
            // does not cross it (priced above the FOK) — it will rest Open. Both
            // are pending when the single round runs.
            let fok_id = test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot)
                .fill_or_kill()
                .place(&mut state);
            let gtc_id = test_fixtures::order(SELLER, &pair, Side::Sell, 200 * PRICE_SCALE, lot)
                .place(&mut state);
            assert_eq!(state.get_order_book(&pair).unwrap().pending_orders_len(), 2);

            EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

            // The FOK was killed; its quote reservation is released.
            let fok = test_fixtures::record_of(&state, BUYER, fok_id);
            assert_eq!(fok.status, OrderStatus::Expired);
            assert_eq!(fok.filled_quantity, Quantity::ZERO);
            assert_eq!(
                state.get_balance(&BUYER, &pair.quote),
                balance(100 * lot, 0u64)
            );

            // The GTC rested Open, fully reserved, untouched by the FOK kill.
            let gtc = test_fixtures::record_of(&state, SELLER, gtc_id);
            assert_eq!(gtc.status, OrderStatus::Open);
            assert_eq!(gtc.filled_quantity, Quantity::ZERO);
            assert_eq!(state.get_balance(&SELLER, &pair.base), balance(0u64, lot));
        }

        /// The resting orders of `pair`'s book — its bid and ask levels — with
        /// the next-sequence counter and (drained) pending queue excluded, so
        /// two snapshots compare equal iff the resting book is byte-identical.
        fn resting_levels(
            state: &State<VectorMemory, VectorMemory>,
            pair: &crate::order::TradingPair,
        ) -> (Vec<crate::order::PriceLevel>, Vec<crate::order::PriceLevel>) {
            let snapshot =
                crate::order::OrderBookSnapshot::from(state.get_order_book(pair).unwrap());
            (snapshot.bids, snapshot.asks)
        }
    }
}

mod execution_policy {
    use crate::balance::TokenBalance;
    use crate::order::{OrderHistory, TradeHistory};
    use crate::state::{ExecutionPolicy, State};
    use ic_stable_structures::VectorMemory;
    use oisy_trade_types_internal::{InitArg, Mode};

    #[test]
    fn should_thread_init_arg_fields_through_to_execution_policy() {
        let state = State::new(
            InitArg {
                mode: Mode::GeneralAvailability,
                max_orders_per_chunk: 17,
                instruction_budget: 12_345,
            },
            OrderHistory::new(VectorMemory::default(), VectorMemory::default()),
            TradeHistory::new(VectorMemory::default(), VectorMemory::default()),
            crate::user::UserRegistry::new(VectorMemory::default()),
            TokenBalance::new(VectorMemory::default()),
        )
        .unwrap();

        assert_eq!(
            state.execution_policy(),
            &ExecutionPolicy::try_new(17, 12_345).unwrap()
        );
    }
}

mod get_balances {
    use crate::order::{Quantity, TokenId};
    use crate::state::StableMemoryOptions;
    use crate::test_fixtures;
    use candid::{Nat, Principal};
    use oisy_trade_types::{Balance, FilterToken, GetBalancesError, UserTokenBalance};

    const USER: Principal = Principal::from_slice(&[0xAA]);

    #[test]
    fn should_return_empty_for_user_without_balances_and_no_filter() {
        let (state, _, _) = test_fixtures::two_token_state();
        assert_eq!(state.get_balances(&USER, None), Ok(vec![]));
    }

    /// Read paths resolve identities with `lookup`, never `get_or_register`, so
    /// querying a never-seen principal must not create a registry entry.
    #[test]
    fn reads_do_not_register_unseen_principals() {
        let (state, a_id, _) = test_fixtures::two_token_state();
        let registry_before = state.user_registry.clone();
        let stranger = Principal::from_slice(&[0xEE]);

        assert_eq!(state.get_balances(&stranger, None), Ok(vec![]));
        let filter = vec![FilterToken::ById(a_id.into())];
        assert_eq!(
            state.get_balances(&stranger, Some(&filter)),
            Ok(vec![balance(a_id, test_fixtures::ckbtc_metadata(), 0, 0)]),
        );
        assert_eq!(
            state.get_balance(&stranger, &a_id),
            crate::balance::Balance::zero()
        );
        assert!(
            state
                .get_user_orders(&stranger, None, 10)
                .unwrap()
                .is_empty()
        );

        assert_eq!(
            state.user_registry, registry_before,
            "read paths must not register an unseen principal"
        );
    }

    #[test]
    fn should_return_zero_entry_for_registered_token_in_filter() {
        let (state, a_id, _) = test_fixtures::two_token_state();
        let filter = vec![FilterToken::ById(a_id.into())];

        assert_eq!(
            state.get_balances(&USER, Some(&filter)),
            Ok(vec![balance(a_id, test_fixtures::ckbtc_metadata(), 0, 0)]),
        );
    }

    #[test]
    fn should_return_non_zero_entries_without_filter() {
        let (mut state, a_id, b_id) = test_fixtures::two_token_state();
        state.deposit(
            USER,
            a_id,
            Quantity::from(10u64),
            StableMemoryOptions::Write,
        );
        state.deposit(USER, b_id, Quantity::from(5u64), StableMemoryOptions::Write);

        // BTreeMap iteration follows TokenId ordering; assert as a set.
        let mut got = state
            .get_balances(&USER, None)
            .expect("no filter cannot fail");
        got.sort_by_key(|b| b.token.id.ledger_id);
        let mut want = vec![
            balance(a_id, test_fixtures::ckbtc_metadata(), 10, 0),
            balance(b_id, test_fixtures::icp_metadata(), 5, 0),
        ];
        want.sort_by_key(|b| b.token.id.ledger_id);
        assert_eq!(got, want);
    }

    #[test]
    fn should_include_zero_entries_for_filtered_tokens_user_does_not_hold() {
        let (mut state, a_id, b_id) = test_fixtures::two_token_state();
        state.deposit(
            USER,
            a_id,
            Quantity::from(10u64),
            StableMemoryOptions::Write,
        );
        let filter = vec![
            FilterToken::ById(a_id.into()),
            FilterToken::ById(b_id.into()),
        ];

        assert_eq!(
            state.get_balances(&USER, Some(&filter)),
            Ok(vec![
                balance(a_id, test_fixtures::ckbtc_metadata(), 10, 0),
                balance(b_id, test_fixtures::icp_metadata(), 0, 0),
            ]),
        );
    }

    #[test]
    fn should_skip_existing_zero_entries_without_filter() {
        let (mut state, a_id, b_id) = test_fixtures::two_token_state();
        state.deposit(
            USER,
            a_id,
            Quantity::from(10u64),
            StableMemoryOptions::Write,
        );
        state.deposit(USER, b_id, Quantity::from(5u64), StableMemoryOptions::Write);
        state
            .withdraw(USER, b_id, Quantity::from(5u64))
            .expect("withdraw should succeed");

        assert_eq!(
            state.get_balances(&USER, None),
            Ok(vec![balance(a_id, test_fixtures::ckbtc_metadata(), 10, 0)])
        );
    }

    #[test]
    fn should_fail_whole_call_when_one_filter_entry_is_unknown() {
        let (mut state, a_id, _) = test_fixtures::two_token_state();
        state.deposit(
            USER,
            a_id,
            Quantity::from(10u64),
            StableMemoryOptions::Write,
        );
        let unknown = TokenId::new(Principal::from_slice(&[0xFF]));
        let filter = vec![
            FilterToken::ById(a_id.into()),
            FilterToken::ById(unknown.into()),
        ];

        assert_eq!(
            state.get_balances(&USER, Some(&filter)),
            Err(GetBalancesError::request(
                oisy_trade_types::GetBalancesRequestError::TokenNotSupported(FilterToken::ById(
                    unknown.into()
                ))
            )),
        );
    }

    #[test]
    fn should_dedup_filter_entries() {
        let (mut state, a_id, b_id) = test_fixtures::two_token_state();
        state.deposit(
            USER,
            a_id,
            Quantity::from(10u64),
            StableMemoryOptions::Write,
        );
        let filter = vec![
            FilterToken::ById(a_id.into()),
            FilterToken::ById(a_id.into()),
            FilterToken::ById(b_id.into()),
            FilterToken::ById(b_id.into()),
        ];

        assert_eq!(
            state.get_balances(&USER, Some(&filter)),
            Ok(vec![
                balance(a_id, test_fixtures::ckbtc_metadata(), 10, 0),
                balance(b_id, test_fixtures::icp_metadata(), 0, 0),
            ]),
        );
    }

    #[test]
    fn should_return_empty_for_empty_filter() {
        let (mut state, a_id, _) = test_fixtures::two_token_state();
        state.deposit(
            USER,
            a_id,
            Quantity::from(10u64),
            StableMemoryOptions::Write,
        );

        assert_eq!(state.get_balances(&USER, Some(&[])), Ok(vec![]));
    }

    fn balance(
        token_id: TokenId,
        metadata: crate::order::TokenMetadata,
        free: u64,
        reserved: u64,
    ) -> UserTokenBalance {
        UserTokenBalance {
            token: oisy_trade_types::Token {
                id: token_id.into(),
                metadata: metadata.into(),
            },
            balance: Balance {
                free: Nat::from(free),
                reserved: Nat::from(reserved),
            },
        }
    }
}

mod get_fee_balances {
    use crate::order::TokenId;
    use crate::test_fixtures;
    use candid::{Nat, Principal};
    use oisy_trade_types::{Balance, FilterToken, GetBalancesError, UserTokenBalance};

    #[test]
    fn should_return_empty_when_no_fees_accrued_and_no_filter() {
        let (state, _, _) = test_fixtures::two_token_state();
        assert_eq!(state.get_fee_balances(None), Ok(vec![]));
    }

    #[test]
    fn should_return_zero_entry_for_registered_token_in_filter() {
        let (state, a_id, _) = test_fixtures::two_token_state();
        let filter = vec![FilterToken::ById(a_id.into())];

        assert_eq!(
            state.get_fee_balances(Some(&filter)),
            Ok(vec![fee_balance(a_id, test_fixtures::ckbtc_metadata(), 0)]),
        );
    }

    #[test]
    fn should_return_non_zero_entries_without_filter() {
        let (mut state, a_id, b_id) = test_fixtures::two_token_state();
        test_fixtures::accrue_fee(&mut state.balances, a_id, 7);
        test_fixtures::accrue_fee(&mut state.balances, b_id, 3);

        let mut got = state.get_fee_balances(None).expect("no filter cannot fail");
        got.sort_by_key(|b| b.token.id.ledger_id);
        let mut want = vec![
            fee_balance(a_id, test_fixtures::ckbtc_metadata(), 7),
            fee_balance(b_id, test_fixtures::icp_metadata(), 3),
        ];
        want.sort_by_key(|b| b.token.id.ledger_id);
        assert_eq!(got, want);
    }

    #[test]
    fn should_include_zero_entries_for_filtered_tokens_with_no_accrual() {
        let (mut state, a_id, b_id) = test_fixtures::two_token_state();
        test_fixtures::accrue_fee(&mut state.balances, a_id, 7);
        let filter = vec![
            FilterToken::ById(a_id.into()),
            FilterToken::ById(b_id.into()),
        ];

        assert_eq!(
            state.get_fee_balances(Some(&filter)),
            Ok(vec![
                fee_balance(a_id, test_fixtures::ckbtc_metadata(), 7),
                fee_balance(b_id, test_fixtures::icp_metadata(), 0),
            ]),
        );
    }

    #[test]
    fn should_fail_whole_call_when_one_filter_entry_is_unknown() {
        let (mut state, a_id, _) = test_fixtures::two_token_state();
        test_fixtures::accrue_fee(&mut state.balances, a_id, 7);
        let unknown = TokenId::new(Principal::from_slice(&[0xFF]));
        let filter = vec![
            FilterToken::ById(a_id.into()),
            FilterToken::ById(unknown.into()),
        ];

        assert_eq!(
            state.get_fee_balances(Some(&filter)),
            Err(GetBalancesError::request(
                oisy_trade_types::GetBalancesRequestError::TokenNotSupported(FilterToken::ById(
                    unknown.into()
                ))
            )),
        );
    }

    #[test]
    fn should_collapse_duplicate_filter_entries() {
        let (mut state, a_id, _) = test_fixtures::two_token_state();
        test_fixtures::accrue_fee(&mut state.balances, a_id, 5);
        let filter = vec![
            FilterToken::ById(a_id.into()),
            FilterToken::ById(a_id.into()),
        ];

        assert_eq!(
            state.get_fee_balances(Some(&filter)),
            Ok(vec![fee_balance(a_id, test_fixtures::ckbtc_metadata(), 5)]),
        );
    }

    #[test]
    fn should_return_empty_for_empty_filter() {
        let (mut state, a_id, _) = test_fixtures::two_token_state();
        test_fixtures::accrue_fee(&mut state.balances, a_id, 5);

        assert_eq!(state.get_fee_balances(Some(&[])), Ok(vec![]));
    }

    fn fee_balance(
        token_id: TokenId,
        metadata: crate::order::TokenMetadata,
        amount: u64,
    ) -> UserTokenBalance {
        UserTokenBalance {
            token: oisy_trade_types::Token {
                id: token_id.into(),
                metadata: metadata.into(),
            },
            balance: Balance {
                free: Nat::from(amount),
                reserved: Nat::from(0u64),
            },
        }
    }
}

mod pending_state_predicates {
    use crate::EXECUTOR;
    use crate::order::{FeeRates, OrderBookId, Side};
    use crate::test_fixtures;
    use crate::test_fixtures::mocks::mock_runtime_for;
    use crate::test_fixtures::{
        LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, PRICE_SCALE, TICK_SIZE, ckbtc_metadata,
        icp_ckbtc_trading_pair, icp_metadata,
    };
    use candid::Principal;

    const BUYER: Principal = Principal::from_slice(&[0x01]);
    const SELLER: Principal = Principal::from_slice(&[0x02]);

    #[test]
    fn should_report_no_pending_state_on_fresh_state() {
        let state = setup_one_book();
        assert!(!state.has_pending_orders());
        assert!(!state.has_pending_settling_events());
    }

    #[test]
    fn should_clear_pending_predicates_after_matching_drains() {
        let mut state = setup_one_book();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);
        assert!(state.has_pending_orders());

        EXECUTOR.run_once(&mut state, &mock_runtime_for(Principal::anonymous()));

        assert!(!state.has_pending_orders());
        assert!(!state.has_pending_settling_events());
    }

    #[test]
    fn should_report_settling_events_present_between_match_and_drain() {
        let mut state = setup_one_book();
        let pair = icp_ckbtc_trading_pair();
        let lot = u128::from(LOT_SIZE.get());

        test_fixtures::order(BUYER, &pair, Side::Buy, 100 * PRICE_SCALE, lot).place(&mut state);
        test_fixtures::order(SELLER, &pair, Side::Sell, 100 * PRICE_SCALE, lot).place(&mut state);

        // Apply only the matching event; do not drain settling. The matching
        // produces a SettlingEvent on the queue that must be observable.
        let book = state.order_book(&OrderBookId::ZERO).unwrap();
        let matching_event = crate::state::event::MatchingEvent {
            book_id: OrderBookId::ZERO,
            orders: book.pending_order_seqs().collect(),
        };
        state.record_matching_event(
            &matching_event,
            crate::Timestamp::EPOCH,
            crate::state::StableMemoryOptions::Write,
        );

        assert!(state.has_pending_settling_events());
    }

    fn setup_one_book()
    -> crate::state::State<ic_stable_structures::VectorMemory, ic_stable_structures::VectorMemory>
    {
        let mut state = test_fixtures::state();
        state.record_trading_pair(
            OrderBookId::ZERO,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
        state
    }
}
