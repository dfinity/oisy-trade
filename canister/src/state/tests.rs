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

mod add_trading_pair {
    use crate::order::{LotSize, TickSize, TokenId, TokenMetadata, TradingPair};
    use crate::test_fixtures;
    use crate::test_fixtures::{
        LOT_SIZE, TICK_SIZE, ckbtc_metadata, ckbtc_token_id, icp_ckbtc_trading_pair, icp_metadata,
        icp_token_id,
    };
    use candid::Principal;

    #[test]
    fn should_add_trading_pair_and_store_token_metadata() {
        let mut state = test_fixtures::state();
        state
            .add_trading_pair(
                icp_ckbtc_trading_pair(),
                TICK_SIZE,
                LOT_SIZE,
                icp_metadata(),
                ckbtc_metadata(),
            )
            .unwrap();

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
        state
            .add_trading_pair(
                icp_ckbtc_trading_pair(),
                TICK_SIZE,
                LOT_SIZE,
                icp_metadata(),
                ckbtc_metadata(),
            )
            .unwrap();

        // Second pair: ICP/ckETH — ICP already registered with same metadata
        state
            .add_trading_pair(
                TradingPair {
                    base: icp_token_id(),
                    quote: token_c,
                },
                TICK_SIZE,
                LOT_SIZE,
                icp_metadata(),
                token_c_metadata,
            )
            .unwrap();
    }

    #[test]
    fn should_reject_inconsistent_metadata_for_base_token() {
        let mut state = test_fixtures::state();
        let token_c = TokenId::new(Principal::from_slice(&[0x03]));

        state
            .add_trading_pair(
                icp_ckbtc_trading_pair(),
                TICK_SIZE,
                LOT_SIZE,
                icp_metadata(),
                ckbtc_metadata(),
            )
            .unwrap();

        let wrong_metadata = TokenMetadata {
            symbol: "WRONG".to_string(),
            decimals: 99,
        };
        let result = state.add_trading_pair(
            TradingPair {
                base: icp_token_id(),
                quote: token_c,
            },
            TICK_SIZE,
            LOT_SIZE,
            wrong_metadata.clone(),
            TokenMetadata {
                symbol: "ckETH".to_string(),
                decimals: 18,
            },
        );

        assert_eq!(
            result,
            Err(dex_types::AddTradingPairError::InconsistentTokenMetadata {
                token: icp_token_id().into(),
                expected: icp_metadata().into(),
                submitted: wrong_metadata.into(),
            })
        );
    }

    #[test]
    fn should_reject_inconsistent_metadata_for_quote_token() {
        let mut state = test_fixtures::state();
        let token_c = TokenId::new(Principal::from_slice(&[0x03]));

        state
            .add_trading_pair(
                icp_ckbtc_trading_pair(),
                TICK_SIZE,
                LOT_SIZE,
                icp_metadata(),
                ckbtc_metadata(),
            )
            .unwrap();

        let wrong_metadata = TokenMetadata {
            symbol: "WRONG".to_string(),
            decimals: 99,
        };
        let result = state.add_trading_pair(
            TradingPair {
                base: token_c,
                quote: ckbtc_token_id(),
            },
            TICK_SIZE,
            LOT_SIZE,
            TokenMetadata {
                symbol: "ckETH".to_string(),
                decimals: 18,
            },
            wrong_metadata.clone(),
        );

        assert_eq!(
            result,
            Err(dex_types::AddTradingPairError::InconsistentTokenMetadata {
                token: ckbtc_token_id().into(),
                expected: ckbtc_metadata().into(),
                submitted: wrong_metadata.into(),
            })
        );
    }

    #[test]
    fn should_not_mutate_state_on_inconsistent_metadata_error() {
        let mut state = test_fixtures::state();
        state
            .add_trading_pair(
                icp_ckbtc_trading_pair(),
                TICK_SIZE,
                LOT_SIZE,
                icp_metadata(),
                ckbtc_metadata(),
            )
            .unwrap();
        let state_before = state.clone();

        let token_c = TokenId::new(Principal::from_slice(&[0x03]));
        let result = state.add_trading_pair(
            TradingPair {
                base: icp_token_id(),
                quote: token_c,
            },
            TICK_SIZE,
            LOT_SIZE,
            TokenMetadata {
                symbol: "WRONG".to_string(),
                decimals: 99,
            },
            TokenMetadata {
                symbol: "ckETH".to_string(),
                decimals: 18,
            },
        );

        assert!(result.is_err());
        assert_eq!(state_before, state);
    }

    #[test]
    fn should_reject_duplicate_trading_pair() {
        let mut state = test_fixtures::state();
        state
            .add_trading_pair(
                icp_ckbtc_trading_pair(),
                TICK_SIZE,
                LOT_SIZE,
                icp_metadata(),
                ckbtc_metadata(),
            )
            .unwrap();

        let result = state.add_trading_pair(
            icp_ckbtc_trading_pair(),
            TickSize::new(std::num::NonZeroU64::new(20).unwrap()),
            LotSize::new(std::num::NonZeroU64::new(2_000_000).unwrap()),
            icp_metadata(),
            ckbtc_metadata(),
        );

        assert_eq!(
            result,
            Err(dex_types::AddTradingPairError::TradingPairAlreadyExists)
        );
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
