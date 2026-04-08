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

mod add_limit_order {
    use crate::order::{PendingOrder, Price, Quantity, Side};
    use crate::state::AddLimitOrderError;
    use crate::test_fixtures;
    use crate::test_fixtures::{LOT_SIZE, TICK_SIZE, icp_ckbtc_trading_pair};
    use assert_matches::assert_matches;
    use candid::Principal;

    #[test]
    fn should_not_insert_empty_balance_on_failed_reservation() {
        let mut state = test_fixtures::state();
        let pair = icp_ckbtc_trading_pair();
        state
            .add_trading_pair(pair.clone(), TICK_SIZE, LOT_SIZE)
            .unwrap();
        let user = Principal::from_slice(&[0x01]);
        let pending = PendingOrder {
            side: Side::Buy,
            price: Price::new(100),
            quantity: Quantity::new(LOT_SIZE.get()),
        };
        let state_before_reservation = state.clone();

        let result = state.add_limit_order(user, pair, pending);

        assert_matches!(result, Err(AddLimitOrderError::InsufficientBalance { .. }));
        assert_eq!(state_before_reservation, state);
    }
}

mod settle_fills {
    use crate::order::{PendingOrder, Price, Quantity, Side, TokenId};
    use crate::state::State;
    use crate::test_fixtures::{LOT_SIZE, TICK_SIZE, icp_ckbtc_trading_pair};
    use candid::{Nat, Principal};
    use dex_types_internal::{InitArg, Mode};

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
        let totals_before = snapshot_totals(&state);
        state.process_pending_orders();

        let buyer_base = state.get_balance(BUYER, pair.base);
        let buyer_quote = state.get_balance(BUYER, pair.quote);
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(0, 0));

        let seller_base = state.get_balance(SELLER, pair.base);
        let seller_quote = state.get_balance(SELLER, pair.quote);
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
        let totals_before = snapshot_totals(&state);
        state.process_pending_orders();

        let buyer_base = state.get_balance(BUYER, pair.base);
        let buyer_quote = state.get_balance(BUYER, pair.quote);
        // Buyer deposited 100*lot quote, paid 90*lot, surplus 10*lot returned to free
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(10 * lot, 0));

        let seller_base = state.get_balance(SELLER, pair.base);
        let seller_quote = state.get_balance(SELLER, pair.quote);
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
        let totals_before = snapshot_totals(&state);
        state.process_pending_orders();

        let buyer_base = state.get_balance(BUYER, pair.base);
        let buyer_quote = state.get_balance(BUYER, pair.quote);
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(0, 0));

        let seller_base = state.get_balance(SELLER, pair.base);
        let seller_quote = state.get_balance(SELLER, pair.quote);
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
        let totals_before = snapshot_totals(&state);
        state.process_pending_orders();

        let buyer_base = state.get_balance(BUYER, pair.base);
        let buyer_quote = state.get_balance(BUYER, pair.quote);
        // Buyer filled 1 lot, 2 lots remain reserved
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(0, 200 * lot));

        let seller_base = state.get_balance(SELLER, pair.base);
        let seller_quote = state.get_balance(SELLER, pair.quote);
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
        let totals_before = snapshot_totals(&state);
        state.process_pending_orders();

        let buyer_base = state.get_balance(BUYER, pair.base);
        let buyer_quote = state.get_balance(BUYER, pair.quote);
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
        let totals_before = snapshot_totals(&state);
        state.process_pending_orders();

        let buyer_base = state.get_balance(BUYER, pair.base);
        let buyer_quote = state.get_balance(BUYER, pair.quote);
        // Surplus: (100-90)*lot = 10*lot returned to free
        // Remaining reserved: 100*2*lot = 200*lot
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(10 * lot, 200 * lot));

        let seller_base = state.get_balance(SELLER, pair.base);
        let seller_quote = state.get_balance(SELLER, pair.quote);
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
        let totals_before = snapshot_totals(&state);
        state.process_pending_orders();

        let buyer_base = state.get_balance(BUYER, pair.base);
        let buyer_quote = state.get_balance(BUYER, pair.quote);
        assert_eq!(buyer_base, balance(lot, 0));
        assert_eq!(buyer_quote, balance(0, 0));

        let seller_base = state.get_balance(SELLER, pair.base);
        let seller_quote = state.get_balance(SELLER, pair.quote);
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
        let totals_before = snapshot_totals(&state);
        state.process_pending_orders();

        let buyer_base = state.get_balance(BUYER, pair.base);
        let buyer_quote = state.get_balance(BUYER, pair.quote);
        // Buyer deposited 100*lot + 110*lot = 210*lot quote, all consumed
        assert_eq!(buyer_base, balance(2 * lot, 0));
        assert_eq!(buyer_quote, balance(0, 0));

        let seller_base = state.get_balance(SELLER, pair.base);
        let seller_quote = state.get_balance(SELLER, pair.quote);
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
        state.deposit(user, pair.quote, (100 * lot).into());
        state
            .add_limit_order(
                user,
                pair.clone(),
                PendingOrder {
                    side: Side::Buy,
                    price: Price::new(100),
                    quantity: Quantity::new(lot),
                },
            )
            .unwrap();
        state.deposit(user, pair.base, lot.into());
        state
            .add_limit_order(
                user,
                pair.clone(),
                PendingOrder {
                    side: Side::Sell,
                    price: Price::new(100),
                    quantity: Quantity::new(lot),
                },
            )
            .unwrap();

        let base_before = state.get_balance(user, pair.base);
        let quote_before = state.get_balance(user, pair.quote);
        state.process_pending_orders();
        let base_after = state.get_balance(user, pair.base);
        let quote_after = state.get_balance(user, pair.quote);

        // Total tokens unchanged: base and quote just move between free/reserved
        assert_eq!(
            base_before.free.clone() + base_before.reserved.clone(),
            base_after.free.clone() + base_after.reserved.clone(),
            "base token total changed"
        );
        assert_eq!(
            quote_before.free.clone() + quote_before.reserved.clone(),
            quote_after.free.clone() + quote_after.reserved.clone(),
            "quote token total changed"
        );
        // After self-trade: all reserved released, net balances same as deposited
        assert_eq!(base_after, balance(lot, 0));
        assert_eq!(quote_after, balance(100 * lot, 0));
    }

    fn setup() -> State {
        let mut state = State::try_from(InitArg {
            mode: Mode::GeneralAvailability,
        })
        .unwrap();
        let pair = icp_ckbtc_trading_pair();
        state.add_trading_pair(pair, TICK_SIZE, LOT_SIZE).unwrap();
        state
    }

    fn place_buy_order(state: &mut State, price: u64, quantity: u64) {
        let pair = icp_ckbtc_trading_pair();
        state.deposit(BUYER, pair.quote, (price * quantity).into());
        state
            .add_limit_order(
                BUYER,
                pair,
                PendingOrder {
                    side: Side::Buy,
                    price: Price::new(price),
                    quantity: Quantity::new(quantity),
                },
            )
            .unwrap();
    }

    fn place_sell_order(state: &mut State, price: u64, quantity: u64) {
        let pair = icp_ckbtc_trading_pair();
        state.deposit(SELLER, pair.base, quantity.into());
        state
            .add_limit_order(
                SELLER,
                pair,
                PendingOrder {
                    side: Side::Sell,
                    price: Price::new(price),
                    quantity: Quantity::new(quantity),
                },
            )
            .unwrap();
    }

    fn balance(free: u64, reserved: u64) -> dex_types::Balance {
        dex_types::Balance {
            free: free.into(),
            reserved: reserved.into(),
        }
    }

    /// Sum free + reserved across both users for a given token.
    fn total_token(state: &State, token: TokenId) -> Nat {
        [BUYER, SELLER].iter().fold(Nat::from(0u64), |acc, user| {
            let b = state.get_balance(*user, token);
            acc + b.free + b.reserved
        })
    }

    /// Snapshot the total supply of base and quote tokens across both users.
    fn snapshot_totals(state: &State) -> (Nat, Nat) {
        let pair = icp_ckbtc_trading_pair();
        (
            total_token(state, pair.base),
            total_token(state, pair.quote),
        )
    }

    /// Assert that the total base and quote tokens across both users are unchanged.
    fn assert_token_conservation(state: &State, before: &(Nat, Nat)) {
        let after = snapshot_totals(state);
        assert_eq!(before.0, after.0, "base token total changed");
        assert_eq!(before.1, after.1, "quote token total changed");
    }
}
