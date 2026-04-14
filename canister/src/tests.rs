mod add_trading_pair {
    use crate::test_fixtures::{
        ckbtc_token_id, icp_ckbtc_trading_pair, icp_token_id, init_state_with_order_book,
        mocks::MockRuntime, trading_pair_request,
    };
    use crate::{add_trading_pair, state};
    use candid::Principal;
    use dex_types::{AddTradingPairError, TokenId, TokenMetadata};

    #[test]
    fn should_reject_inconsistent_metadata_for_base_token() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let token_c = TokenId {
            ledger_id: Principal::from_slice(&[0x03]),
        };

        let wrong_metadata = TokenMetadata {
            symbol: "WRONG".to_string(),
            decimals: 99,
        };
        let result = add_trading_pair(
            trading_pair_request(
                icp_token_id(),
                wrong_metadata.clone(),
                token_c,
                TokenMetadata {
                    symbol: "ckETH".to_string(),
                    decimals: 18,
                },
            ),
            &runtime,
        );

        assert_eq!(
            result,
            Err(AddTradingPairError::InconsistentTokenMetadata {
                token: icp_token_id().into(),
                expected: TokenMetadata {
                    symbol: "ICP".to_string(),
                    decimals: 8,
                },
                submitted: wrong_metadata,
            })
        );
    }

    #[test]
    fn should_reject_inconsistent_metadata_for_quote_token() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let token_c = TokenId {
            ledger_id: Principal::from_slice(&[0x03]),
        };

        let wrong_metadata = TokenMetadata {
            symbol: "WRONG".to_string(),
            decimals: 99,
        };
        let result = add_trading_pair(
            trading_pair_request(
                token_c,
                TokenMetadata {
                    symbol: "ckETH".to_string(),
                    decimals: 18,
                },
                ckbtc_token_id(),
                wrong_metadata.clone(),
            ),
            &runtime,
        );

        assert_eq!(
            result,
            Err(AddTradingPairError::InconsistentTokenMetadata {
                token: ckbtc_token_id().into(),
                expected: TokenMetadata {
                    symbol: "ckBTC".to_string(),
                    decimals: 8,
                },
                submitted: wrong_metadata,
            })
        );
    }

    #[test]
    fn should_not_mutate_state_on_inconsistent_metadata_error() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let token_c = TokenId {
            ledger_id: Principal::from_slice(&[0x03]),
        };

        let trading_pairs_before = state::with_state(|s| s.trading_pairs().clone());

        let result = add_trading_pair(
            trading_pair_request(
                icp_token_id(),
                TokenMetadata {
                    symbol: "WRONG".to_string(),
                    decimals: 99,
                },
                token_c,
                TokenMetadata {
                    symbol: "ckETH".to_string(),
                    decimals: 18,
                },
            ),
            &runtime,
        );

        assert!(result.is_err());
        let trading_pairs_after = state::with_state(|s| s.trading_pairs().clone());
        assert_eq!(trading_pairs_before, trading_pairs_after);
    }

    #[test]
    fn should_reject_duplicate_trading_pair() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let pair = icp_ckbtc_trading_pair();

        let result = add_trading_pair(
            trading_pair_request(
                pair.base,
                TokenMetadata {
                    symbol: "ICP".to_string(),
                    decimals: 8,
                },
                pair.quote,
                TokenMetadata {
                    symbol: "ckBTC".to_string(),
                    decimals: 8,
                },
            ),
            &runtime,
        );

        assert_eq!(result, Err(AddTradingPairError::TradingPairAlreadyExists));
    }

    fn controller_runtime() -> MockRuntime {
        let mut mock = MockRuntime::new();
        mock.expect_msg_caller()
            .return_const(Principal::anonymous());
        mock.expect_is_controller().return_const(true);
        mock
    }
}

mod add_limit_order {
    use crate::test_fixtures::{
        fund_user, icp_ckbtc_trading_pair, init_state_with_order_book, limit_order_request,
        mocks::MockRuntime,
    };
    use crate::{add_limit_order, get_balance, state, test_fixtures};
    use candid::Principal;
    use dex_types::{Balance, LimitOrderRequest, Side};
    use std::collections::BTreeSet;

    const DEFAULT_USER: Principal = Principal::from_slice(&[0x042]);

    #[test]
    fn should_add_limit_orders_with_distinct_order_ids() {
        init_state_with_order_book();
        fund_user(DEFAULT_USER);
        let mut order_ids = BTreeSet::new();
        let num_orders = 100;

        for _ in 0..num_orders {
            let order_id = test_fixtures::add_limit_order(DEFAULT_USER, &limit_order_request());
            assert!(order_ids.insert(order_id));
        }
    }

    #[test]
    fn should_reject_order_for_unknown_trading_pair() {
        init_state_with_order_book();
        let runtime = mock_runtime_for(DEFAULT_USER);
        let mut request = limit_order_request();
        request.pair = dex_types::TradingPair {
            base: Principal::management_canister(),
            quote: Principal::management_canister(),
        };
        let result = add_limit_order(request, &runtime);
        assert_eq!(
            result,
            Err(dex_types::AddLimitOrderError::UnknownTradingPair)
        );
    }

    #[test]
    fn should_reject_order_with_invalid_price() {
        init_state_with_order_book();
        let runtime = mock_runtime_for(DEFAULT_USER);

        let cases = vec![(7, "not a multiple of tick size"), (0, "zero price")];
        for (price, name) in cases {
            let mut request = limit_order_request();
            request.price = price;
            let result = add_limit_order(request, &runtime);
            assert_eq!(
                result,
                Err(dex_types::AddLimitOrderError::InvalidPrice {
                    price,
                    tick_size: 10,
                }),
                "case: {name}"
            );
        }
    }

    #[test]
    fn should_reject_order_with_invalid_quantity() {
        init_state_with_order_book();
        let runtime = mock_runtime_for(DEFAULT_USER);

        let cases = vec![
            (500_000u64, "not a multiple of lot size"),
            (0, "zero quantity"),
        ];
        for (quantity, name) in cases {
            let mut request = limit_order_request();
            request.side = dex_types::Side::Sell;
            request.quantity = candid::Nat::from(quantity);
            let result = add_limit_order(request, &runtime);
            assert_eq!(
                result,
                Err(dex_types::AddLimitOrderError::InvalidQuantity {
                    quantity: candid::Nat::from(quantity),
                    lot_size: 1_000_000,
                }),
                "case: {name}"
            );
        }
    }

    #[test]
    fn should_reject_buy_order_with_insufficient_balance() {
        init_state_with_order_book();
        let user = Principal::from_slice(&[0x01]);
        let runtime = mock_runtime_for(user);
        // user has no balance at all
        let request = limit_order_request(); // Buy, price=100, quantity=1_000_000
        let result = add_limit_order(request, &runtime);
        assert_eq!(
            result,
            Err(dex_types::AddLimitOrderError::InsufficientBalance {
                token: dex_types::TokenId {
                    ledger_id: Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap(),
                },
                available: candid::Nat::from(0u64),
                // price * quantity = 100 * 1_000_000
                required: candid::Nat::from(100_000_000u64),
            })
        );
    }

    #[test]
    fn should_reject_sell_order_with_insufficient_balance() {
        init_state_with_order_book();
        let user = Principal::from_slice(&[0x01]);
        let runtime = mock_runtime_for(user);
        let mut request = limit_order_request();
        request.side = dex_types::Side::Sell;
        let result = add_limit_order(request, &runtime);
        assert_eq!(
            result,
            Err(dex_types::AddLimitOrderError::InsufficientBalance {
                token: dex_types::TokenId {
                    ledger_id: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
                },
                available: candid::Nat::from(0u64),
                required: candid::Nat::from(1_000_000u64),
            })
        );
    }

    #[test]
    fn should_reserve_balance_on_buy_order() {
        init_state_with_order_book();
        let pair = test_fixtures::icp_ckbtc_trading_pair();
        let runtime = mock_runtime_for(DEFAULT_USER);
        let price = 100u64;
        let quantity = 1_000_000u64;
        let required = price * quantity;
        let order = LimitOrderRequest {
            pair: icp_ckbtc_trading_pair().into(),
            side: Side::Buy,
            price,
            quantity: candid::Nat::from(quantity),
        };
        // Deposit exactly enough for a buy order: price=100, quantity=1_000_000 → 100_000_000
        state::with_state_mut(|s| {
            s.deposit(DEFAULT_USER, pair.quote, required.into());
        });

        test_fixtures::add_limit_order(DEFAULT_USER, &order);

        assert_eq!(get_balance(pair.base.into(), &runtime), Balance::default());
        assert_eq!(
            get_balance(pair.quote.into(), &runtime),
            Balance {
                free: 0u64.into(),
                reserved: required.into(),
            }
        );
    }

    #[test]
    fn should_reserve_balance_on_sell_order() {
        init_state_with_order_book();
        let pair = test_fixtures::icp_ckbtc_trading_pair();
        let runtime = mock_runtime_for(DEFAULT_USER);
        let quantity = 100_000_000u64;
        let order = LimitOrderRequest {
            pair: icp_ckbtc_trading_pair().into(),
            side: Side::Sell,
            price: 10,
            quantity: candid::Nat::from(quantity),
        };
        // Deposit exactly enough for a sell order: price=X, quantity=100_000_000→ 100_000_000
        state::with_state_mut(|s| {
            s.deposit(DEFAULT_USER, pair.base, quantity.into());
        });

        test_fixtures::add_limit_order(DEFAULT_USER, &order);

        assert_eq!(
            get_balance(pair.base.into(), &runtime),
            Balance {
                free: 0u64.into(),
                reserved: quantity.into(),
            }
        );
        assert_eq!(get_balance(pair.quote.into(), &runtime), Balance::default());
    }

    fn mock_runtime_for(caller: Principal) -> MockRuntime {
        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(caller);
        mock
    }
}

mod get_order_status {
    use crate::get_order_status;
    use crate::test_fixtures::{self, fund_user, init_state_with_order_book, limit_order_request};
    use candid::Principal;
    use dex_types::OrderStatus;

    #[test]
    fn should_return_pending_for_existing_order() {
        init_state_with_order_book();
        fund_user(Principal::anonymous());
        let order_id =
            test_fixtures::add_limit_order(Principal::anonymous(), &limit_order_request());
        let status = get_order_status(order_id.to_string());
        assert_eq!(status, OrderStatus::Pending);
    }

    #[test]
    fn should_return_filled_after_matching() {
        init_state_with_order_book();
        let buyer = Principal::from_slice(&[0x01]);
        let seller = Principal::from_slice(&[0x02]);
        fund_user(buyer);
        fund_user(seller);

        let buy_id = test_fixtures::add_limit_order(buyer, &limit_order_request());
        let mut sell_request = limit_order_request();
        sell_request.side = dex_types::Side::Sell;
        let sell_id = test_fixtures::add_limit_order(seller, &sell_request);

        crate::process_pending_orders();

        assert_eq!(get_order_status(buy_id.to_string()), OrderStatus::Filled);
        assert_eq!(get_order_status(sell_id.to_string()), OrderStatus::Filled);
    }

    #[test]
    fn should_return_not_found_for_nonexistent_order() {
        init_state_with_order_book();
        // Valid hex format but refers to a non-existent book/seq
        let status = get_order_status("ffffffffffffffffffffffffffffffff".to_string());
        assert_eq!(status, OrderStatus::NotFound);
    }

    #[test]
    #[should_panic(expected = "ERROR: invalid order id")]
    fn should_trap_on_syntactically_invalid_order_id() {
        init_state_with_order_book();
        get_order_status("not-a-valid-order-id".to_string());
    }
}

mod get_trading_pairs {
    use crate::get_trading_pairs;
    use crate::state::init_state;
    use crate::test_fixtures;
    use crate::test_fixtures::{
        LOT_SIZE, TICK_SIZE, ckbtc_token_id, icp_token_id, init_state_with_order_book,
    };
    use dex_types::TradingPairInfo;

    #[test]
    fn should_return_empty_when_no_trading_pairs() {
        init_state(test_fixtures::state());
        let pairs = get_trading_pairs();
        assert!(pairs.is_empty());
    }

    #[test]
    fn should_return_listed_trading_pairs() {
        init_state_with_order_book();

        let pairs = get_trading_pairs();

        assert_eq!(
            pairs,
            vec![TradingPairInfo {
                base: dex_types::Token {
                    id: dex_types::TokenId::from(icp_token_id()),
                    metadata: dex_types::TokenMetadata {
                        symbol: "ICP".to_string(),
                        decimals: 8,
                    },
                },
                quote: dex_types::Token {
                    id: dex_types::TokenId::from(ckbtc_token_id()),
                    metadata: dex_types::TokenMetadata {
                        symbol: "ckBTC".to_string(),
                        decimals: 8,
                    },
                },
                tick_size: TICK_SIZE.get(),
                lot_size: LOT_SIZE.get(),
            }]
        );
    }
}
