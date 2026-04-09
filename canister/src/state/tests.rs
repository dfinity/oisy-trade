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
    use crate::test_fixtures::{
        LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata,
    };
    use assert_matches::assert_matches;
    use candid::Principal;

    #[test]
    fn should_not_insert_empty_balance_on_failed_reservation() {
        let mut state = test_fixtures::state();
        let pair = icp_ckbtc_trading_pair();
        state
            .add_trading_pair(
                pair.clone(),
                TICK_SIZE,
                LOT_SIZE,
                icp_metadata(),
                ckbtc_metadata(),
            )
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
