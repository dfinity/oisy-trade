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

    fn state(mode: Mode) -> State {
        State::try_from(dex_types_internal::InitArg { mode }).unwrap()
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

mod settle_fills {
    use crate::balance::Balance;
    use crate::order::{OrderBookId, PendingOrder, Price, Quantity, Side};
    use crate::state::State;
    use crate::test_fixtures::{
        LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata,
    };
    use candid::Principal;
    use dex_types_internal::{InitArg, Mode};
    use std::collections::BTreeMap;

    const BUYER: Principal = Principal::from_slice(&[0x01]);
    const SELLER: Principal = Principal::from_slice(&[0x02]);

    #[test]
    fn should_settle_exact_match_at_same_price() {
        let mut state = setup();
        let pair = icp_ckbtc_trading_pair();
        let lot = u64::from(LOT_SIZE);
        let price = 100u64;

        place_buy_order(&mut state, price, lot);
        place_sell_order(&mut state, price, lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

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
        place_sell_order(&mut state, 90, lot);
        place_buy_order(&mut state, 100, lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

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
        place_buy_order(&mut state, 110, lot);
        place_sell_order(&mut state, 100, lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

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
        place_buy_order(&mut state, 100, 3 * lot);
        place_sell_order(&mut state, 100, lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

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
        place_sell_order(&mut state, 90, lot);
        place_sell_order(&mut state, 100, lot);
        place_buy_order(&mut state, 100, 2 * lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

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
        place_sell_order(&mut state, 90, lot);
        place_buy_order(&mut state, 100, 3 * lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

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
        place_buy_order(&mut state, 100, lot);
        place_sell_order(&mut state, 100, 3 * lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

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
        place_buy_order(&mut state, 100, lot);
        place_buy_order(&mut state, 110, lot);
        place_sell_order(&mut state, 100, 2 * lot);
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

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
        place_buy_order_for(&mut state, user, 100, lot);
        place_sell_order_for(&mut state, user, 100, lot);

        let base_before = state.get_balance(&user, &pair.base);
        let quote_before = state.get_balance(&user, &pair.quote);
        state.process_pending_orders();
        let base_after = state.get_balance(&user, &pair.base);
        let quote_after = state.get_balance(&user, &pair.quote);

        // Total tokens unchanged: base and quote just move between free/reserved
        assert_eq!(
            base_before.free().clone() + base_before.reserved().clone(),
            base_after.free().clone() + base_after.reserved().clone(),
            "base token total changed"
        );
        assert_eq!(
            quote_before.free().clone() + quote_before.reserved().clone(),
            quote_after.free().clone() + quote_after.reserved().clone(),
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
        place_sell_order_for(&mut state, seller_a, 90, lot);
        place_sell_order_for(&mut state, seller_b, 100, lot);

        // Buy taker sweeps both
        place_buy_order(&mut state, 100, 2 * lot);
        let participants = [BUYER, seller_a, seller_b];
        let totals_before = snapshot_balances(&state, &participants);
        state.process_pending_orders();

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
        let quantity = Quantity::from(u64::from(LOT_SIZE)) * Quantity::from(u64::MAX);

        place_buy_order(&mut state, price, quantity.clone());
        place_sell_order(&mut state, price, quantity.clone());
        let totals_before = snapshot_balances(&state, &[BUYER, SELLER]);
        state.process_pending_orders();

        let quote_total = Price::new(price).mul_quantity(&quantity);

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

    fn setup() -> State {
        let mut state = State::try_from(InitArg {
            mode: Mode::GeneralAvailability,
        })
        .unwrap();
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

    fn place_buy_order(
        state: &mut State,
        price: u64,
        quantity: impl Into<Quantity>,
    ) -> crate::order::OrderId {
        place_buy_order_for(state, BUYER, price, quantity)
    }

    fn place_sell_order(
        state: &mut State,
        price: u64,
        quantity: impl Into<Quantity>,
    ) -> crate::order::OrderId {
        place_sell_order_for(state, SELLER, price, quantity)
    }

    fn place_buy_order_for(
        state: &mut State,
        user: Principal,
        price: u64,
        quantity: impl Into<Quantity>,
    ) -> crate::order::OrderId {
        place_order(state, user, Side::Buy, price, quantity)
    }

    fn place_sell_order_for(
        state: &mut State,
        user: Principal,
        price: u64,
        quantity: impl Into<Quantity>,
    ) -> crate::order::OrderId {
        place_order(state, user, Side::Sell, price, quantity)
    }

    fn place_order(
        state: &mut State,
        user: Principal,
        side: Side,
        price: u64,
        quantity: impl Into<Quantity>,
    ) -> crate::order::OrderId {
        let pair = icp_ckbtc_trading_pair();
        let pending = PendingOrder {
            side,
            price: Price::new(price),
            quantity: quantity.into(),
        };
        let deposit = match pending.side {
            Side::Buy => (pair.quote, pending.price.mul_quantity(&pending.quantity)),
            Side::Sell => (pair.base, pending.quantity.clone()),
        };
        state.deposit(user, deposit.0, deposit.1);
        let (order_id, order) = state.validate_limit_order(user, pair, pending).unwrap();
        state.record_limit_order(user, order_id.book_id(), order);
        order_id
    }

    mod order_status {
        use super::*;
        use dex_types::OrderStatus;

        #[test]
        fn should_return_pending_before_matching() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let buy_id = place_buy_order(&mut state, 100, lot);

            assert_eq!(state.get_order_status(buy_id), OrderStatus::Pending);
        }

        #[test]
        fn should_return_open_for_resting_order() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let buy_id = place_buy_order(&mut state, 100, lot);
            state.process_pending_orders();

            assert_eq!(state.get_order_status(buy_id), OrderStatus::Open);
        }

        #[test]
        fn should_return_filled_after_exact_match() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            let buy_id = place_buy_order(&mut state, 100, lot);
            let sell_id = place_sell_order(&mut state, 100, lot);
            state.process_pending_orders();

            assert_eq!(state.get_order_status(buy_id), OrderStatus::Filled);
            assert_eq!(state.get_order_status(sell_id), OrderStatus::Filled);
        }

        #[test]
        fn should_return_open_for_partially_filled_maker() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            // Sell 3 lots, buy only 1 → sell partially filled, remainder rests
            let sell_id = place_sell_order(&mut state, 100, 3 * lot);
            let buy_id = place_buy_order(&mut state, 100, lot);
            state.process_pending_orders();

            assert_eq!(state.get_order_status(sell_id), OrderStatus::Open);
            assert_eq!(state.get_order_status(buy_id), OrderStatus::Filled);
        }

        #[test]
        fn should_return_open_for_partially_filled_taker() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            // Sell 1 lot, buy 3 lots → buy partially fills and rests with 2 remaining
            let sell_id = place_sell_order(&mut state, 100, lot);
            let buy_id = place_buy_order(&mut state, 100, 3 * lot);
            state.process_pending_orders();

            assert_eq!(state.get_order_status(sell_id), OrderStatus::Filled);
            assert_eq!(state.get_order_status(buy_id), OrderStatus::Open);
        }

        #[test]
        fn should_return_filled_after_multi_fill_maker_depletion() {
            let mut state = setup();
            let lot = u64::from(LOT_SIZE);
            // Sell rests with 2 lots; two successive buys deplete it
            let sell_id = place_sell_order(&mut state, 100, 2 * lot);
            state.process_pending_orders();
            assert_eq!(state.get_order_status(sell_id), OrderStatus::Open);

            let buy1_id = place_buy_order(&mut state, 100, lot);
            state.process_pending_orders();
            assert_eq!(state.get_order_status(sell_id), OrderStatus::Open);
            assert_eq!(state.get_order_status(buy1_id), OrderStatus::Filled);

            let buy2_id = place_buy_order(&mut state, 100, lot);
            state.process_pending_orders();
            assert_eq!(state.get_order_status(sell_id), OrderStatus::Filled);
            assert_eq!(state.get_order_status(buy2_id), OrderStatus::Filled);
        }
    }

    mod settle_fill_ordering {
        use super::*;
        use crate::order::{OrderBookId, OrderId, OrderRecord};
        use crate::test_fixtures::arbitrary::arb_fill;
        use dex_types::OrderStatus;
        use proptest::prelude::*;

        const BOOK_ID: OrderBookId = OrderBookId::ZERO;

        proptest! {
            #[test]
            fn should_produce_same_state_regardless_of_fill_order(
                fill1 in arb_fill(0),
                fill2 in arb_fill(1),
                // Small range so principals can collide (self-trade, shared maker/taker)
                buyer1_id in 1..=4u8,
                seller1_id in 1..=4u8,
                buyer2_id in 1..=4u8,
                seller2_id in 1..=4u8,
            ) {
                let mut state = setup();
                let pair = icp_ckbtc_trading_pair();
                let principals: [(Principal, Principal); 2] = [
                    (Principal::from_slice(&[buyer1_id]), Principal::from_slice(&[seller1_id])),
                    (Principal::from_slice(&[buyer2_id]), Principal::from_slice(&[seller2_id])),
                ];

                // Register orders and fund balances for each fill
                for (i, fill) in [&fill1, &fill2].iter().enumerate() {
                    let (buyer, seller) = principals[i];
                    let (taker_owner, maker_owner) = match fill.taker_side {
                        Side::Buy => (buyer, seller),
                        Side::Sell => (seller, buyer),
                    };

                    state.order_history.insert_once(
                        OrderId::new(BOOK_ID, fill.taker_order_seq),
                        OrderRecord {
                            owner: taker_owner,
                            pair: pair.clone(),
                            side: fill.taker_side,
                            price: fill.taker_price,
                            quantity: fill.quantity.clone(),
                            status: OrderStatus::Open,
                        },
                    );
                    let maker_side = match fill.taker_side {
                        Side::Buy => Side::Sell,
                        Side::Sell => Side::Buy,
                    };
                    state.order_history.insert_once(
                        OrderId::new(BOOK_ID, fill.maker_order_seq),
                        OrderRecord {
                            owner: maker_owner,
                            pair: pair.clone(),
                            side: maker_side,
                            price: fill.maker_price,
                            quantity: fill.quantity.clone(),
                            status: OrderStatus::Open,
                        },
                    );

                    // Buyer reserved at their order price (taker_price for buy
                    // takers, maker_price for sell takers where maker is buyer).
                    let buyer_price = match fill.taker_side {
                        Side::Buy => fill.taker_price,
                        Side::Sell => fill.maker_price,
                    };
                    let buy_reserve = buyer_price.mul_quantity(&fill.quantity);
                    state.deposit(buyer, pair.quote, buy_reserve.clone());
                    state.balance_mut(buyer, pair.quote).reserve(buy_reserve).unwrap();
                    state.deposit(seller, pair.base, fill.quantity.clone());
                    state.balance_mut(seller, pair.base).reserve(fill.quantity.clone()).unwrap();
                }

                let mut state1 = state.clone();
                state1.settle_fill(BOOK_ID, &pair, &fill1);
                state1.settle_fill(BOOK_ID, &pair, &fill2);

                let mut state2 = state;
                state2.settle_fill(BOOK_ID, &pair, &fill2);
                state2.settle_fill(BOOK_ID, &pair, &fill1);

                prop_assert_eq!(state1, state2);
            }
        }
    }

    fn balance(free: u64, reserved: u64) -> Balance {
        Balance::new(free, reserved)
    }

    type BalanceSnapshot = BTreeMap<Principal, (Balance, Balance)>;

    /// Snapshot base and quote balances for each principal.
    fn snapshot_balances(state: &State, principals: &[Principal]) -> BalanceSnapshot {
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
    fn assert_token_conservation(state: &State, before: &BalanceSnapshot) {
        let principals: Vec<Principal> = before.keys().copied().collect();
        let after = snapshot_balances(state, &principals);

        let sum = |snap: &BalanceSnapshot| -> (Quantity, Quantity) {
            snap.values().fold(
                (Quantity::ZERO, Quantity::ZERO),
                |(base_acc, quote_acc), (base, quote)| {
                    (
                        base_acc + base.free().clone() + base.reserved().clone(),
                        quote_acc + quote.free().clone() + quote.reserved().clone(),
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
