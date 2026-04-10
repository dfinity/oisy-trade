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
        let runtime = mock_runtime_for(DEFAULT_USER);
        let mut order_ids = BTreeSet::new();
        let num_orders = 100;

        for _ in 0..num_orders {
            let order_id = add_limit_order(limit_order_request(), &runtime).unwrap();
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

        add_limit_order(order, &runtime).unwrap();

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

        add_limit_order(order, &runtime).unwrap();

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
    use crate::add_limit_order;
    use crate::get_order_status;
    use crate::test_fixtures::{
        fund_user, init_state_with_order_book, limit_order_request, mocks::MockRuntime,
    };
    use candid::Principal;
    use dex_types::OrderStatus;

    #[test]
    fn should_return_pending_for_existing_order() {
        init_state_with_order_book();
        fund_user(Principal::anonymous());
        let mut runtime = MockRuntime::new();
        runtime
            .expect_msg_caller()
            .return_const(Principal::anonymous());
        let order_id = add_limit_order(limit_order_request(), &runtime).unwrap();
        let status = get_order_status(order_id);
        assert_eq!(status, OrderStatus::Pending);
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
    use crate::order::{TokenId, TradingPair};
    use crate::state;
    use crate::state::init_state;
    use crate::test_fixtures;
    use crate::test_fixtures::{LOT_SIZE, TICK_SIZE};
    use candid::Principal;
    use dex_types::TradingPairInfo;

    #[test]
    fn should_return_empty_when_no_trading_pairs() {
        init_state(test_fixtures::state());
        let pairs = get_trading_pairs();
        assert!(pairs.is_empty());
    }

    #[test]
    fn should_return_listed_trading_pairs() {
        init_state(test_fixtures::state());
        let base = TokenId::new(Principal::from_slice(&[0x01]));
        let quote = TokenId::new(Principal::from_slice(&[0x02]));
        state::with_state_mut(|s| {
            s.add_trading_pair(TradingPair { base, quote }, TICK_SIZE, LOT_SIZE)
                .unwrap();
        });

        let pairs = get_trading_pairs();

        assert_eq!(
            pairs,
            vec![TradingPairInfo {
                base_asset: dex_types::TokenId::from(base),
                quote_asset: dex_types::TokenId::from(quote),
                tick_size: TICK_SIZE.get(),
                lot_size: LOT_SIZE.get(),
            }]
        );
    }
}
