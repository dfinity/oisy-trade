mod add_trading_pair {
    use crate::test_fixtures::tokens::SupportedTokens;
    use crate::test_fixtures::{
        ckbtc_token_id, icp_ckbtc_trading_pair, icp_token_id, init_state_with_order_book,
        mocks::{MockRuntime, mock_runtime_for},
        trading_pair_request,
    };
    use crate::{add_trading_pair, state};
    use candid::{Nat, Principal};
    use oisy_trade_types::{AddTradingPairError, TokenId, TokenMetadata};

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

    #[test]
    fn should_reject_indivisible_tick_lot_for_base_decimals() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let base = TokenId {
            ledger_id: Principal::from_slice(&[0x12]),
        };
        let quote = TokenId {
            ledger_id: Principal::from_slice(&[0x13]),
        };
        // tick * lot = 10 * 1_000_000 = 10^7, which is not a multiple of 10^8,
        // so an 8-decimal base would round at settlement.
        let result = add_trading_pair(pair_request(base, 8, quote, 6, 10, 1_000_000), &runtime);

        assert_eq!(
            result,
            Err(AddTradingPairError::IndivisibleTickLotForBaseDecimals {
                tick_size: candid::Nat::from(10u64),
                lot_size: candid::Nat::from(1_000_000u64),
                base_decimals: 8,
            })
        );
    }

    #[test]
    fn should_reject_base_decimals_too_large() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let base = TokenId {
            ledger_id: Principal::from_slice(&[0x14]),
        };
        let quote = TokenId {
            ledger_id: Principal::from_slice(&[0x15]),
        };
        let result = add_trading_pair(
            trading_pair_request(
                base,
                TokenMetadata {
                    symbol: "BASE".to_string(),
                    decimals: 20,
                },
                quote,
                TokenMetadata {
                    symbol: "QUOTE".to_string(),
                    decimals: 6,
                },
            ),
            &runtime,
        );

        assert_eq!(
            result,
            Err(AddTradingPairError::BaseDecimalsTooLarge { decimals: 20 })
        );
    }

    #[test]
    fn should_reject_zero_min_notional() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let base = TokenId {
            ledger_id: Principal::from_slice(&[0x16]),
        };
        let quote = TokenId {
            ledger_id: Principal::from_slice(&[0x17]),
        };
        let mut request = pair_request(base, 8, quote, 6, 10_000, 10_000);
        request.min_notional = candid::Nat::from(0u64);
        request.max_notional = None;

        let result = add_trading_pair(request, &runtime);

        assert_eq!(
            result,
            Err(AddTradingPairError::InvalidNotional {
                min_notional: candid::Nat::from(0u64),
                max_notional: None,
            })
        );
    }

    #[test]
    fn should_reject_max_notional_below_min_notional() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let base = TokenId {
            ledger_id: Principal::from_slice(&[0x18]),
        };
        let quote = TokenId {
            ledger_id: Principal::from_slice(&[0x19]),
        };
        let mut request = pair_request(base, 8, quote, 6, 10_000, 10_000);
        request.min_notional = candid::Nat::from(5u64);
        request.max_notional = Some(candid::Nat::from(4u64));

        let result = add_trading_pair(request, &runtime);

        assert_eq!(
            result,
            Err(AddTradingPairError::InvalidNotional {
                min_notional: candid::Nat::from(5u64),
                max_notional: Some(candid::Nat::from(4u64)),
            })
        );
    }

    /// Builds a request with explicit decimals/tick/lot and unique token ids.
    fn pair_request(
        base: TokenId,
        base_decimals: u8,
        quote: TokenId,
        quote_decimals: u8,
        tick_size: u64,
        lot_size: u64,
    ) -> oisy_trade_types::AddTradingPairRequest {
        oisy_trade_types::AddTradingPairRequest {
            base: oisy_trade_types::Token {
                id: base,
                metadata: TokenMetadata {
                    symbol: "BASE".to_string(),
                    decimals: base_decimals,
                },
            },
            quote: oisy_trade_types::Token {
                id: quote,
                metadata: TokenMetadata {
                    symbol: "QUOTE".to_string(),
                    decimals: quote_decimals,
                },
            },
            tick_size: candid::Nat::from(tick_size),
            lot_size: candid::Nat::from(lot_size),
            maker_fee_bps: 0,
            taker_fee_bps: 0,
            min_notional: candid::Nat::from(1u64),
            max_notional: None,
        }
    }

    /// The pairs selected for initial listing are accepted under their intended
    /// parameters, where `tick_size = price_increment × 10^quote_decimals` and
    /// `lot_size = quantity_increment × 10^base_decimals`.
    #[test]
    fn should_accept_selected_trading_pairs() {
        init_state_with_order_book();
        let mut runtime = mock_runtime_for(Principal::anonymous());
        runtime.expect_is_controller().return_const(true);
        // Shared quote template; `base` is a placeholder that every pair below
        // overrides via `..ckusdt_quote.clone()`.
        let ckusdt_quote = oisy_trade_types::AddTradingPairRequest {
            base: SupportedTokens::CKUSDT.token(),
            quote: SupportedTokens::CKUSDT.token(),
            tick_size: Nat::default(),
            lot_size: Nat::default(),
            maker_fee_bps: 0,
            taker_fee_bps: 20,
            min_notional: Nat::from(5_000_000_u64),
            max_notional: Some(Nat::from(9_000_000_000_000_u64)),
        };
        // Deliberately all ckUSDT-quoted (quote_decimals = 6) — this mirrors the
        // real launch basket rather than maximizing quote-decimal variety.
        let pairs = [
            // ICP/ckUSDT
            oisy_trade_types::AddTradingPairRequest {
                base: SupportedTokens::ICP.token(),
                tick_size: Nat::from(1_000_u32),
                lot_size: Nat::from(1_000_000_u32),
                ..ckusdt_quote.clone()
            },
            // ckBTC/ckUSDT
            oisy_trade_types::AddTradingPairRequest {
                base: SupportedTokens::CKBTC.token(),
                tick_size: Nat::from(10_000_u32),
                lot_size: Nat::from(10_000_u32),
                ..ckusdt_quote.clone()
            },
            // VCHF/ckUSDT
            oisy_trade_types::AddTradingPairRequest {
                base: SupportedTokens::VCHF.token(),
                tick_size: Nat::from(100_u32),
                lot_size: Nat::from(1_000_000_u32),
                ..ckusdt_quote.clone()
            },
            // ckUSDC/ckUSDT
            oisy_trade_types::AddTradingPairRequest {
                base: SupportedTokens::CKUSDC.token(),
                tick_size: Nat::from(10_u32),
                lot_size: Nat::from(1_000_000_u32),
                ..ckusdt_quote.clone()
            },
            // ckETH/ckUSDT
            oisy_trade_types::AddTradingPairRequest {
                base: SupportedTokens::CKETH.token(),
                tick_size: Nat::from(10_000_u32),
                lot_size: Nat::from(100_000_000_000_000_u64),
                ..ckusdt_quote.clone()
            },
        ];

        for pair in pairs {
            let result = add_trading_pair(pair.clone(), &runtime);
            assert_eq!(result, Ok(()), "{pair:?} should be accepted");
        }
    }

    #[test]
    fn should_accept_boundary_base_decimals() {
        init_state_with_order_book();
        let mut runtime = mock_runtime_for(Principal::anonymous());
        runtime.expect_is_controller().return_const(true);
        // base_decimals = 0 (divisor 1) accepts any tick/lot; 19 is the largest
        // base_decimals whose 10^base_decimals still fits the u64 divisor.
        for (base_decimals, tick_size, lot_size) in
            [(0u8, 7u64, 13u64), (19, 1, 10_000_000_000_000_000_000)]
        {
            let base = TokenId {
                ledger_id: Principal::from_slice(&[0x30, base_decimals]),
            };
            let quote = TokenId {
                ledger_id: Principal::from_slice(&[0x31, base_decimals]),
            };
            let result = add_trading_pair(
                pair_request(base, base_decimals, quote, 6, tick_size, lot_size),
                &runtime,
            );
            assert_eq!(
                result,
                Ok(()),
                "base_decimals {base_decimals} should be accepted"
            );
        }
    }

    #[test]
    fn should_reject_maker_fee_bps_above_max() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let token_c = TokenId {
            ledger_id: Principal::from_slice(&[0x03]),
        };
        let mut req = trading_pair_request(
            icp_token_id(),
            TokenMetadata {
                symbol: "ICP".to_string(),
                decimals: 8,
            },
            token_c,
            TokenMetadata {
                symbol: "ckETH".to_string(),
                decimals: 18,
            },
        );
        req.maker_fee_bps = 10_001;

        let result = add_trading_pair(req, &runtime);

        assert_eq!(result, Err(AddTradingPairError::InvalidBasisPoint(10_001)));
    }

    #[test]
    fn should_reject_taker_fee_bps_above_max() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let token_c = TokenId {
            ledger_id: Principal::from_slice(&[0x03]),
        };
        let mut req = trading_pair_request(
            icp_token_id(),
            TokenMetadata {
                symbol: "ICP".to_string(),
                decimals: 8,
            },
            token_c,
            TokenMetadata {
                symbol: "ckETH".to_string(),
                decimals: 18,
            },
        );
        req.taker_fee_bps = u16::MAX;

        let result = add_trading_pair(req, &runtime);

        assert_eq!(
            result,
            Err(AddTradingPairError::InvalidBasisPoint(u16::MAX))
        );
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
    use crate::test_fixtures::mocks::mock_runtime_for;
    use crate::test_fixtures::{
        PRICE_SCALE, TICK_SIZE, fund_user, icp_ckbtc_trading_pair, init_state_with_order_book,
        limit_order_request,
    };
    use crate::{add_limit_order, get_balances, state};
    use candid::Principal;
    use oisy_trade_types::{Balance, FilterToken, LimitOrderRequest, Side};
    use std::collections::BTreeSet;

    const DEFAULT_USER: Principal = Principal::from_slice(&[0x042]);

    fn balance_of(token: oisy_trade_types::TokenId, caller: Principal) -> Balance {
        let mut result = get_balances(Some(vec![FilterToken::ById(token)]), caller).unwrap();
        assert_eq!(result.len(), 1);
        result.remove(0).unwrap().balance
    }

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
        request.pair = oisy_trade_types::TradingPair {
            base: Principal::management_canister(),
            quote: Principal::management_canister(),
        };
        let result = add_limit_order(request, &runtime);
        assert_eq!(
            result.unwrap_err().kind,
            oisy_trade_types::ErrorKind::RequestError(Some(
                oisy_trade_types::AddLimitOrderRequestError::UnknownTradingPair
            ))
        );
    }

    #[test]
    fn should_reject_order_with_invalid_price() {
        init_state_with_order_book();
        let runtime = mock_runtime_for(DEFAULT_USER);

        let cases = vec![(7u64, "not a multiple of tick size"), (0, "zero price")];
        for (price, name) in cases {
            let mut request = limit_order_request();
            request.price = candid::Nat::from(price);
            let result = add_limit_order(request, &runtime);
            assert_eq!(
                result.unwrap_err().kind,
                oisy_trade_types::ErrorKind::RequestError(Some(
                    oisy_trade_types::AddLimitOrderRequestError::InvalidPrice {
                        price: candid::Nat::from(price),
                        tick_size: candid::Nat::from(TICK_SIZE.get()),
                    }
                )),
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
            request.side = oisy_trade_types::Side::Sell;
            request.quantity = candid::Nat::from(quantity);
            let result = add_limit_order(request, &runtime);
            assert_eq!(
                result.unwrap_err().kind,
                oisy_trade_types::ErrorKind::RequestError(Some(
                    oisy_trade_types::AddLimitOrderRequestError::InvalidQuantity {
                        quantity: candid::Nat::from(quantity),
                        lot_size: candid::Nat::from(1_000_000u64),
                    }
                )),
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
            result.unwrap_err().kind,
            oisy_trade_types::ErrorKind::RequestError(Some(
                oisy_trade_types::AddLimitOrderRequestError::InsufficientBalance {
                    token: oisy_trade_types::TokenId {
                        ledger_id: Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap(),
                    },
                    available: candid::Nat::from(0u64),
                    // price * quantity = 100 * 1_000_000
                    required: candid::Nat::from(100_000_000u64),
                }
            ))
        );
    }

    #[test]
    fn should_reject_sell_order_with_insufficient_balance() {
        init_state_with_order_book();
        let user = Principal::from_slice(&[0x01]);
        let runtime = mock_runtime_for(user);
        let mut request = limit_order_request();
        request.side = oisy_trade_types::Side::Sell;
        let result = add_limit_order(request, &runtime);
        assert_eq!(
            result.unwrap_err().kind,
            oisy_trade_types::ErrorKind::RequestError(Some(
                oisy_trade_types::AddLimitOrderRequestError::InsufficientBalance {
                    token: oisy_trade_types::TokenId {
                        ledger_id: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
                    },
                    available: candid::Nat::from(0u64),
                    required: candid::Nat::from(1_000_000u64),
                }
            ))
        );
    }

    #[test]
    fn should_reserve_balance_on_buy_order() {
        init_state_with_order_book();
        let pair = icp_ckbtc_trading_pair();
        let runtime = mock_runtime_for(DEFAULT_USER);
        let price = 100u128;
        let quantity = 1_000_000u128;
        let required = price * quantity;
        let order = LimitOrderRequest {
            pair: icp_ckbtc_trading_pair().into(),
            side: Side::Buy,
            price: candid::Nat::from(price * PRICE_SCALE),
            quantity: candid::Nat::from(quantity),
        };
        // Deposit exactly enough for a buy order: price=100, quantity=1_000_000 → 100_000_000
        state::with_state_mut(|s| {
            s.deposit(
                DEFAULT_USER,
                pair.quote,
                required.into(),
                state::StableMemoryOptions::Write,
            );
        });

        add_limit_order(order, &runtime).unwrap();

        assert_eq!(
            balance_of(pair.base.into(), DEFAULT_USER),
            Balance::default()
        );
        assert_eq!(
            balance_of(pair.quote.into(), DEFAULT_USER),
            Balance {
                free: 0u64.into(),
                reserved: required.into(),
            }
        );
    }

    #[test]
    fn should_reserve_balance_on_sell_order() {
        init_state_with_order_book();
        let pair = icp_ckbtc_trading_pair();
        let runtime = mock_runtime_for(DEFAULT_USER);
        let quantity = 100_000_000u128;
        let order = LimitOrderRequest {
            pair: icp_ckbtc_trading_pair().into(),
            side: Side::Sell,
            price: candid::Nat::from(10 * PRICE_SCALE),
            quantity: candid::Nat::from(quantity),
        };
        // Deposit exactly enough for a sell order: price=X, quantity=100_000_000→ 100_000_000
        state::with_state_mut(|s| {
            s.deposit(
                DEFAULT_USER,
                pair.base,
                quantity.into(),
                state::StableMemoryOptions::Write,
            );
        });

        add_limit_order(order, &runtime).unwrap();

        assert_eq!(
            balance_of(pair.base.into(), DEFAULT_USER),
            Balance {
                free: 0u64.into(),
                reserved: quantity.into(),
            }
        );
        assert_eq!(
            balance_of(pair.quote.into(), DEFAULT_USER),
            Balance::default()
        );
    }
}

mod cancel_limit_order {
    use crate::Timestamp;
    use crate::order::OrderId;
    use crate::state::with_state_mut;
    use crate::test_fixtures::mocks::{mock_runtime_at, mock_runtime_for};
    use crate::test_fixtures::{
        LOT_SIZE, PRICE_SCALE, fund_user, init_state_with_order_book, limit_order_request,
    };
    use crate::{add_limit_order, cancel_limit_order};
    use candid::Principal;
    use oisy_trade_types::{CancelLimitOrderRequestError, ErrorKind};
    use oisy_trade_types_internal::Mode;

    #[test]
    #[should_panic(expected = "is not allowed to call this endpoint in restricted mode")]
    fn should_fail_when_caller_not_allowed() {
        let authorized = Principal::from_slice(&[0x42]);
        let unauthorized = Principal::from_slice(&[0xFF]);
        init_state_with_order_book();
        with_state_mut(|s| s.set_mode(Mode::restricted_to(vec![authorized])));
        let mut runtime = mock_runtime_for(unauthorized);
        runtime.expect_is_controller().return_const(false);

        let _panic = cancel_limit_order("0x00".to_string(), &runtime);
    }

    #[test]
    fn should_reject_cancel_of_unknown_order() {
        init_state_with_order_book();
        let runtime = mock_runtime_for(Principal::from_slice(&[0x01]));

        for unknown_order_id in [OrderId::ZERO.to_string(), "not-a-valid-id".to_string()] {
            let result = cancel_limit_order(unknown_order_id, &runtime);
            assert_eq!(
                result.unwrap_err().kind,
                ErrorKind::RequestError(Some(CancelLimitOrderRequestError::OrderNotFound))
            );
        }
    }

    #[test]
    fn should_reject_cancel_by_non_owner() {
        init_state_with_order_book();
        let owner = Principal::from_slice(&[0x01]);
        let stranger = Principal::from_slice(&[0x02]);
        fund_user(owner);

        let order_id = add_limit_order(limit_order_request(), &mock_runtime_for(owner)).unwrap();

        let result = cancel_limit_order(order_id, &mock_runtime_for(stranger));
        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(CancelLimitOrderRequestError::NotOrderOwner))
        );
    }

    #[test]
    fn should_reject_second_cancel() {
        init_state_with_order_book();
        let owner = Principal::from_slice(&[0x01]);
        fund_user(owner);
        // Distinct placement and cancel times so the asserted timestamp can
        // only be the submission time, not a re-stamp at cancel.
        let order_id = add_limit_order(
            limit_order_request(),
            &mock_runtime_at(owner, Timestamp::new(111)),
        )
        .unwrap();
        assert_eq!(
            cancel_limit_order(
                order_id.clone(),
                &mock_runtime_at(owner, Timestamp::new(222))
            ),
            Ok(oisy_trade_types::OrderRecord {
                owner,
                side: oisy_trade_types::Side::Buy,
                price: candid::Nat::from(100 * PRICE_SCALE),
                quantity: candid::Nat::from(u64::from(LOT_SIZE)),
                filled_quantity: candid::Nat::from(0u64),
                status: oisy_trade_types::OrderStatus::Canceled,
                created_at: 111,
                last_updated_at: Some(222),
            })
        );

        let result = cancel_limit_order(order_id, &mock_runtime_at(owner, Timestamp::new(333)));

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(CancelLimitOrderRequestError::OrderAlreadyCanceled))
        );
    }

    #[test]
    fn should_reject_cancel_of_filled_order() {
        init_state_with_order_book();
        let buyer = Principal::from_slice(&[0x01]);
        let seller = Principal::from_slice(&[0x02]);
        fund_user(buyer);
        fund_user(seller);

        let buy_id = add_limit_order(limit_order_request(), &mock_runtime_for(buyer)).unwrap();
        let mut sell_request = limit_order_request();
        sell_request.side = oisy_trade_types::Side::Sell;
        add_limit_order(sell_request, &mock_runtime_for(seller)).unwrap();
        crate::process_pending_orders(&mock_runtime_for(buyer));

        let result = cancel_limit_order(buy_id, &mock_runtime_for(buyer));
        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(CancelLimitOrderRequestError::OrderAlreadyFilled))
        );
    }

    #[test]
    fn should_succeed_for_owner() {
        init_state_with_order_book();
        let owner = Principal::from_slice(&[0x01]);
        fund_user(owner);
        // Distinct placement and cancel times so the asserted timestamp can
        // only be the submission time, not a re-stamp at cancel.
        let order_id = add_limit_order(
            limit_order_request(),
            &mock_runtime_at(owner, Timestamp::new(111)),
        )
        .unwrap();

        let result = cancel_limit_order(
            order_id.clone(),
            &mock_runtime_at(owner, Timestamp::new(222)),
        );
        let expected = oisy_trade_types::OrderRecord {
            owner,
            side: oisy_trade_types::Side::Buy,
            price: candid::Nat::from(100 * PRICE_SCALE),
            quantity: candid::Nat::from(u64::from(LOT_SIZE)),
            filled_quantity: candid::Nat::from(0u64),
            status: oisy_trade_types::OrderStatus::Canceled,
            created_at: 111,
            last_updated_at: Some(222),
        };
        assert_eq!(result, Ok(expected.clone()));
        let orders = crate::get_my_orders(
            Some(oisy_trade_types::GetMyOrdersArgs::by_id(order_id)),
            owner,
        )
        .unwrap();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order, expected);
    }
}

mod order_status_via_get_my_orders {
    use crate::test_fixtures::mocks::mock_runtime_for;
    use crate::test_fixtures::{fund_user, init_state_with_order_book, limit_order_request};
    use crate::{add_limit_order, get_my_orders};
    use candid::Principal;
    use oisy_trade_types::{GetMyOrdersArgs, OrderStatus};

    fn status_of(owner: Principal, order_id: oisy_trade_types::OrderId) -> Option<OrderStatus> {
        let orders = get_my_orders(Some(GetMyOrdersArgs::by_id(order_id)), owner).unwrap();
        orders.into_iter().next().map(|o| o.order.status)
    }

    #[test]
    fn should_return_pending_for_existing_order() {
        init_state_with_order_book();
        fund_user(Principal::anonymous());
        let runtime = mock_runtime_for(Principal::anonymous());
        let order_id = add_limit_order(limit_order_request(), &runtime).unwrap();
        assert_eq!(
            status_of(Principal::anonymous(), order_id),
            Some(OrderStatus::Pending)
        );
    }

    #[test]
    fn should_return_filled_after_matching() {
        init_state_with_order_book();
        let buyer = Principal::from_slice(&[0x01]);
        let seller = Principal::from_slice(&[0x02]);
        fund_user(buyer);
        fund_user(seller);

        let buy_id = add_limit_order(limit_order_request(), &mock_runtime_for(buyer)).unwrap();
        let mut sell_request = limit_order_request();
        sell_request.side = oisy_trade_types::Side::Sell;
        let sell_id = add_limit_order(sell_request, &mock_runtime_for(seller)).unwrap();

        crate::process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        assert_eq!(status_of(buyer, buy_id), Some(OrderStatus::Filled));
        assert_eq!(status_of(seller, sell_id), Some(OrderStatus::Filled));
    }

    #[test]
    fn should_return_empty_for_nonexistent_order() {
        init_state_with_order_book();
        // Valid hex format but refers to a non-existent book/seq.
        assert_eq!(
            status_of(
                Principal::anonymous(),
                "ffffffffffffffffffffffffffffffff".to_string()
            ),
            None
        );
    }

    #[test]
    fn should_reject_syntactically_invalid_order_id() {
        init_state_with_order_book();
        let result = get_my_orders(
            Some(GetMyOrdersArgs::by_id("not-a-valid-order-id".to_string())),
            Principal::anonymous(),
        );
        assert!(matches!(
            result,
            Err(crate::GetMyOrdersError::InvalidOrderId(_))
        ));
    }
}

mod deposit {
    use crate::deposit;
    use crate::guard::UserOpGuard;
    use crate::order::{Quantity, TokenId};
    use crate::state;
    use crate::test_fixtures::mocks::CapturingRuntime;
    use crate::test_fixtures::{
        ckbtc_token_id, icp_token_id, init_state_with_order_book, transfer_from_response,
    };
    use candid::{Nat, Principal};
    use icrc_ledger_types::icrc2::transfer_from::TransferFromError;
    use oisy_trade_types::{DepositRequest, DepositRequestError, DepositTemporaryError, ErrorKind};

    const USER: Principal = Principal::from_slice(&[0x42]);
    const OTHER_USER: Principal = Principal::from_slice(&[0x43]);

    #[tokio::test]
    async fn should_return_operation_in_progress_when_deposit_already_in_flight() {
        init_state_with_order_book();
        let _held = UserOpGuard::new(USER, icp_token_id()).expect("test setup: acquire guard");
        let runtime = CapturingRuntime::new(USER, vec![]);

        let result = deposit(deposit_request(icp_token_id()), &runtime).await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::TemporaryError(Some(DepositTemporaryError::OperationInProgress))
        );
        assert!(runtime.captured_calls().is_empty());
    }

    #[tokio::test]
    async fn should_not_block_deposit_for_distinct_token() {
        init_state_with_order_book();
        let _held = UserOpGuard::new(USER, ckbtc_token_id()).expect("test setup: acquire guard");
        let runtime =
            CapturingRuntime::new(USER, vec![Ok(transfer_from_response(Ok(Nat::from(7u64))))]);

        let result = deposit(deposit_request(icp_token_id()), &runtime).await;

        assert!(result.is_ok(), "got {result:?}");
    }

    #[tokio::test]
    async fn should_not_block_deposit_for_distinct_caller() {
        init_state_with_order_book();
        let _held =
            UserOpGuard::new(OTHER_USER, icp_token_id()).expect("test setup: acquire guard");
        let runtime =
            CapturingRuntime::new(USER, vec![Ok(transfer_from_response(Ok(Nat::from(7u64))))]);

        let result = deposit(deposit_request(icp_token_id()), &runtime).await;

        assert!(result.is_ok(), "got {result:?}");
    }

    #[tokio::test]
    async fn should_release_guard_after_deposit_success() {
        init_state_with_order_book();
        let runtime =
            CapturingRuntime::new(USER, vec![Ok(transfer_from_response(Ok(Nat::from(7u64))))]);

        let result = deposit(deposit_request(icp_token_id()), &runtime).await;

        assert!(result.is_ok(), "got {result:?}");
        assert_in_flight_empty();
        state::with_state(|s| {
            let balance = s.get_balance(&USER, &icp_token_id());
            assert_eq!(balance.free(), &Quantity::from(1_000_000u64));
        });
    }

    #[tokio::test]
    async fn should_release_guard_after_deposit_failure() {
        init_state_with_order_book();
        let runtime = CapturingRuntime::new(
            USER,
            vec![Ok(transfer_from_response(Err(
                TransferFromError::TemporarilyUnavailable,
            )))],
        );

        let result = deposit(deposit_request(icp_token_id()), &runtime).await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::TemporaryError(Some(DepositTemporaryError::LedgerTemporarilyUnavailable))
        );
        assert_in_flight_empty();
    }

    #[tokio::test]
    async fn should_reject_unsupported_token() {
        init_state_with_order_book();
        let runtime = CapturingRuntime::new(USER, vec![]);

        let unsupported = TokenId::new(Principal::from_slice(&[0xAB]));
        let result = deposit(deposit_request(unsupported), &runtime).await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(DepositRequestError::UnsupportedToken {
                token_id: unsupported.into(),
            }))
        );
        assert!(runtime.captured_calls().is_empty());
    }

    fn deposit_request(token: TokenId) -> DepositRequest {
        DepositRequest {
            token_id: token.into(),
            amount: Nat::from(1_000_000u64),
        }
    }

    fn assert_in_flight_empty() {
        state::with_state(|s| {
            assert!(
                s.in_flight_user_ops().is_empty(),
                "in_flight_user_ops should be empty after the call returns"
            );
        });
    }
}

mod withdraw {
    use crate::order::{Quantity, TokenId};
    use crate::state::event::{Event, EventType, WithdrawEvent};
    use crate::storage;
    use crate::test_fixtures::mocks::{CapturingRuntime, MockRuntime};
    use crate::test_fixtures::transfer_response;
    use crate::{state, withdraw};
    use candid::{Nat, Principal};
    use ic_cdk::call::Response;
    use icrc_ledger_types::icrc1::transfer::{TransferArg, TransferError};
    use mockall::Sequence;
    use oisy_trade_types::{
        ErrorKind, WithdrawInternalError, WithdrawRequest, WithdrawRequestError,
        WithdrawTemporaryError,
    };

    const USER: Principal = Principal::from_slice(&[0x42]);
    const TOKEN_LEDGER: Principal = Principal::from_slice(&[0xAA]);

    fn token_id() -> oisy_trade_types::TokenId {
        oisy_trade_types::TokenId {
            ledger_id: TOKEN_LEDGER,
        }
    }

    fn init_state_with_balance(amount: u64) {
        state::init_state(crate::test_fixtures::state_vmem());
        state::with_state_mut(|s| {
            s.record_token(
                TokenId::from(token_id()),
                crate::order::TokenMetadata {
                    symbol: "TEST".to_string(),
                    decimals: 8,
                },
            );
            s.deposit(
                USER,
                TokenId::from(token_id()),
                Quantity::from(amount),
                state::StableMemoryOptions::Write,
            );
        });
    }

    fn mock_runtime_returning(responses: Vec<Response>) -> MockRuntime {
        let mut runtime = MockRuntime::new();
        runtime.expect_msg_caller().return_const(USER);
        runtime.expect_time().return_const(crate::Timestamp::EPOCH);

        let mut seq = Sequence::new();
        for response in responses {
            runtime
                .expect_call_unbounded_wait()
                .times(1)
                .in_sequence(&mut seq)
                .withf(|canister_id, method, _args| {
                    canister_id == &TOKEN_LEDGER && method == "icrc1_transfer"
                })
                .return_once(|_, _, _| Ok(response));
        }
        runtime
    }

    fn assert_balance(expected_free: u64) {
        state::with_state(|s| {
            let balance = s.get_balance(&USER, &TokenId::from(token_id()));
            assert_eq!(balance.free(), &Quantity::from(expected_free));
        });
    }

    fn assert_cached_fee(expected: u64) {
        state::with_state(|s| {
            assert_eq!(
                s.get_cached_ledger_fee(&TokenId::from(token_id())),
                Nat::from(expected)
            );
        });
    }

    /// Returns the sole `WithdrawEvent` recorded in the event log, or panics
    /// if there are zero or more than one.
    fn unique_withdraw_event() -> WithdrawEvent {
        let withdraws: Vec<WithdrawEvent> = storage::with_event_iter(|it| {
            it.filter_map(|Event { payload, .. }| match payload {
                EventType::Withdraw(e) => Some(e),
                _ => None,
            })
            .collect()
        });
        assert_eq!(
            withdraws.len(),
            1,
            "expected exactly one WithdrawEvent, got {}",
            withdraws.len()
        );
        withdraws.into_iter().next().unwrap()
    }

    /// Asserts the event log contains no `WithdrawEvent`s. Use this on failure
    /// paths where the withdrawal did not complete.
    fn assert_no_withdraw_event() {
        let count = storage::with_event_iter(|it| {
            it.filter(|Event { payload, .. }| matches!(payload, EventType::Withdraw(_)))
                .count()
        });
        assert_eq!(count, 0, "expected no WithdrawEvent, got {count}");
    }

    fn decode_transfer_arg(runtime: &CapturingRuntime, call_index: usize) -> TransferArg {
        let calls = runtime.captured_calls();
        let call = &calls[call_index];
        assert_eq!(
            call.canister_id, TOKEN_LEDGER,
            "call {call_index}: wrong canister"
        );
        assert_eq!(
            call.method, "icrc1_transfer",
            "call {call_index}: wrong method"
        );
        let (arg,): (TransferArg,) = call.decode_args();
        arg
    }

    #[tokio::test]
    async fn should_return_error_when_fee_changes_between_retries() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        // First call: BadFee(5000). Second call: BadFee(9999).
        let runtime = CapturingRuntime::new(
            USER,
            vec![
                Ok(transfer_response(Err(TransferError::BadFee {
                    expected_fee: Nat::from(5_000u64),
                }))),
                Ok(transfer_response(Err(TransferError::BadFee {
                    expected_fee: Nat::from(9_999u64),
                }))),
            ],
        );

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(deposit),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::TemporaryError(Some(WithdrawTemporaryError::LedgerFeeChanged))
        );
        // Balance credited back.
        assert_balance(deposit);
        // Fee cache updated from the most recent BadFee.
        assert_cached_fee(9_999);
        // First attempt: fee = 0 (cache empty), transfer_amount = deposit.
        let arg0 = decode_transfer_arg(&runtime, 0);
        assert_eq!(arg0.amount, Nat::from(deposit));
        assert_eq!(arg0.fee, Some(Nat::from(0u64)));
        // Retry: fee = 5_000 (from first BadFee), transfer_amount = deposit - 5_000.
        let arg1 = decode_transfer_arg(&runtime, 1);
        assert_eq!(arg1.amount, Nat::from(deposit - 5_000));
        assert_eq!(arg1.fee, Some(Nat::from(5_000u64)));
        // Failed withdrawals leave no event in the log.
        assert_no_withdraw_event();
    }

    #[tokio::test]
    async fn should_return_ledger_error_when_retry_fails() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        // First call: BadFee(5000). Retry: TemporarilyUnavailable.
        let runtime = mock_runtime_returning(vec![
            transfer_response(Err(TransferError::BadFee {
                expected_fee: Nat::from(5_000u64),
            })),
            transfer_response(Err(TransferError::TemporarilyUnavailable)),
        ]);

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(deposit),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::TemporaryError(Some(WithdrawTemporaryError::LedgerTemporarilyUnavailable))
        );
        assert_balance(deposit);
        assert_cached_fee(5_000);
        assert_no_withdraw_event();
    }

    #[tokio::test]
    async fn should_return_insufficient_funds_error() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        // The ledger says the OISY TRADE canister doesn't hold enough tokens.
        let runtime = mock_runtime_returning(vec![transfer_response(Err(
            TransferError::InsufficientFunds {
                balance: Nat::from(0u64),
            },
        ))]);

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(deposit),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::InternalError(Some(WithdrawInternalError::LedgerInsufficientFunds {
                balance: Nat::from(0u64)
            }))
        );
        assert_balance(deposit);
        assert_no_withdraw_event();
    }

    #[tokio::test]
    async fn should_succeed_when_cached_fee_is_stale_high() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        // Simulate a previously cached fee that is higher than the current ledger fee.
        let stale_fee = 500_000u64;
        let real_fee = 100u64;
        state::with_state_mut(|s| {
            s.set_cached_ledger_fee(TokenId::from(token_id()), Nat::from(stale_fee));
        });

        // Withdraw an amount between the real fee and the stale cached fee.
        let withdraw_amount = 200_000u64;
        // First call: cached fee (500_000) > amount (200_000), so the fee is
        // capped to amount - 1. The ledger rejects with BadFee(real_fee).
        // Retry: amount (200_000) > real_fee (100), so transfer succeeds.
        let block_index = Nat::from(42u64);
        let runtime = CapturingRuntime::new(
            USER,
            vec![
                Ok(transfer_response(Err(TransferError::BadFee {
                    expected_fee: Nat::from(real_fee),
                }))),
                Ok(transfer_response(Ok(block_index.clone()))),
            ],
        );

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(withdraw_amount),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result,
            Ok(oisy_trade_types::WithdrawResponse {
                block_index: block_index.clone()
            })
        );
        assert_balance(deposit - withdraw_amount);
        assert_cached_fee(real_fee);
        // First attempt: fee capped to amount - 1 = 199_999, transfer_amount = 1.
        let arg0 = decode_transfer_arg(&runtime, 0);
        assert_eq!(arg0.amount, Nat::from(1u64));
        assert_eq!(arg0.fee, Some(Nat::from(withdraw_amount - 1)));
        // Retry: fee = real_fee (100), transfer_amount = 199_900.
        let arg1 = decode_transfer_arg(&runtime, 1);
        assert_eq!(arg1.amount, Nat::from(withdraw_amount - real_fee));
        assert_eq!(arg1.fee, Some(Nat::from(real_fee)));
        // The successful withdrawal is recorded with the ledger block_index.
        assert_eq!(
            unique_withdraw_event(),
            WithdrawEvent {
                block_index: 42,
                user: USER,
                token: TokenId::from(token_id()),
                amount: Quantity::from(withdraw_amount),
            }
        );
    }

    #[tokio::test]
    async fn should_reject_unsupported_token() {
        // Init state without registering any token.
        state::init_state(crate::test_fixtures::state_vmem());

        let mut runtime = MockRuntime::new();
        runtime.expect_msg_caller().return_const(USER);

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(1_000_000u64),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::UnsupportedToken {
                token_id: token_id(),
            }))
        );
        assert_no_withdraw_event();
    }

    #[tokio::test]
    async fn should_reject_zero_amount() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        let mut runtime = MockRuntime::new();
        runtime.expect_msg_caller().return_const(USER);
        // No call_unbounded_wait expectations — the ledger should never be called.

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(0u64),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::AmountTooSmall {
                min_amount: Nat::from(1u64),
            }))
        );
        // Balance untouched — no debit happened.
        assert_balance(deposit);
        assert_no_withdraw_event();
    }

    #[tokio::test]
    async fn should_reject_when_amount_below_real_fee_and_cached_fee_is_stale_high() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        let stale_fee = 10_000u64;
        let real_fee = 100u64;
        state::with_state_mut(|s| {
            s.set_cached_ledger_fee(TokenId::from(token_id()), Nat::from(stale_fee));
        });

        // Withdraw an amount below the real fee.
        let withdraw_amount = 50u64;
        // First call: capped_fee = min(10_000, 49) = 49, transfer_amount = 1.
        // Ledger returns BadFee(100). amount(50) <= expected_fee(100) → AmountTooSmall.
        let runtime = CapturingRuntime::new(
            USER,
            vec![Ok(transfer_response(Err(TransferError::BadFee {
                expected_fee: Nat::from(real_fee),
            })))],
        );

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(withdraw_amount),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::AmountTooSmall {
                min_amount: Nat::from(real_fee + 1),
            }))
        );
        // Balance credited back.
        assert_balance(deposit);
        // Fee cache updated to the real fee from the BadFee response.
        assert_cached_fee(real_fee);
        // capped_fee = min(10_000, 49) = 49, transfer_amount = 1.
        let arg0 = decode_transfer_arg(&runtime, 0);
        assert_eq!(arg0.amount, Nat::from(1u64));
        assert_eq!(arg0.fee, Some(Nat::from(withdraw_amount - 1)));
        assert_no_withdraw_event();
    }

    #[tokio::test]
    async fn should_not_emit_event_on_insufficient_balance() {
        let deposit = 1_000u64;
        init_state_with_balance(deposit);

        let mut runtime = MockRuntime::new();
        runtime.expect_msg_caller().return_const(USER);
        // No ledger expectation — the user-balance check fails before the
        // async call is made.

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(deposit + 1),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::RequestError(Some(WithdrawRequestError::InsufficientBalance {
                available: Nat::from(deposit),
            }))
        );
        assert_balance(deposit);
        assert_no_withdraw_event();
    }

    fn assert_in_flight_empty() {
        state::with_state(|s| {
            assert!(
                s.in_flight_user_ops().is_empty(),
                "in_flight_user_ops should be empty after the call returns"
            );
        });
    }

    #[tokio::test]
    async fn should_return_operation_in_progress_when_withdraw_already_in_flight() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);
        let _held = crate::guard::UserOpGuard::new(USER, TokenId::from(token_id()))
            .expect("test setup: acquire guard");

        let mut runtime = MockRuntime::new();
        runtime.expect_msg_caller().return_const(USER);
        // No ledger expectation: the guard short-circuits before any ledger call.

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(deposit),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::TemporaryError(Some(WithdrawTemporaryError::OperationInProgress))
        );
        // Balance untouched, no event recorded.
        assert_balance(deposit);
        assert_no_withdraw_event();
    }

    #[tokio::test]
    async fn should_block_withdraw_when_concurrent_deposit_in_flight() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);
        let _held = crate::guard::UserOpGuard::new(USER, TokenId::from(token_id()))
            .expect("test setup: acquire guard simulating in-flight deposit");

        let mut runtime = MockRuntime::new();
        runtime.expect_msg_caller().return_const(USER);

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(deposit),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::TemporaryError(Some(WithdrawTemporaryError::OperationInProgress))
        );
        assert_balance(deposit);
    }

    #[tokio::test]
    async fn should_release_guard_after_withdraw_success() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        let runtime = mock_runtime_returning(vec![transfer_response(Ok(Nat::from(42u64)))]);

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(deposit),
            },
            &runtime,
        )
        .await;

        assert!(result.is_ok(), "got {result:?}");
        assert_in_flight_empty();
    }

    #[tokio::test]
    async fn should_release_guard_after_withdraw_failure_with_rollback() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        let runtime = mock_runtime_returning(vec![transfer_response(Err(
            TransferError::TemporarilyUnavailable,
        ))]);

        let result = withdraw(
            WithdrawRequest {
                token_id: token_id(),
                amount: Nat::from(deposit),
            },
            &runtime,
        )
        .await;

        assert_eq!(
            result.unwrap_err().kind,
            ErrorKind::TemporaryError(Some(WithdrawTemporaryError::LedgerTemporarilyUnavailable))
        );
        // Rollback fully restored the free balance, the guard was released,
        // and no event was emitted for the failed withdrawal.
        assert_balance(deposit);
        assert_in_flight_empty();
        assert_no_withdraw_event();
    }
}

mod get_order_book_ticker {
    use crate::get_order_book_ticker;
    use crate::test_fixtures::mocks::mock_runtime_for;
    use crate::test_fixtures::{
        LOT_SIZE, PRICE_SCALE, fund_user, icp_ckbtc_trading_pair, init_state_with_order_book,
        place_limit_order,
    };
    use candid::{Nat, Principal};
    use oisy_trade_types::{
        GetOrderBookTickerError, OrderBookTicker, PriceLevel, Side, TradingPair,
    };

    #[test]
    fn should_return_unknown_trading_pair_for_unregistered() {
        let unknown_pair = TradingPair {
            base: Principal::from_slice(&[0xaa]),
            quote: Principal::from_slice(&[0xbb]),
        };
        init_state_with_order_book();
        assert_eq!(
            get_order_book_ticker(unknown_pair),
            Err(GetOrderBookTickerError::UnknownTradingPair),
        );
    }

    #[test]
    fn should_return_empty_ticker_for_empty_book() {
        init_state_with_order_book();
        assert_eq!(
            get_order_book_ticker(icp_ckbtc_trading_pair().into()),
            Ok(OrderBookTicker {
                bid: None,
                ask: None,
            }),
        );
    }

    #[test]
    fn should_return_best_bid_and_ask_aggregated_across_same_price() {
        init_state_with_order_book();
        let u1 = Principal::from_slice(&[0x01]);
        let u2 = Principal::from_slice(&[0x02]);
        let u3 = Principal::from_slice(&[0x03]);
        fund_user(u1);
        fund_user(u2);
        fund_user(u3);

        let lot = u64::from(LOT_SIZE);
        // Two buys at 100, one at 90; one sell at 110. None cross.
        place_limit_order(u1, Side::Buy, 100 * PRICE_SCALE, lot);
        place_limit_order(u2, Side::Buy, 100 * PRICE_SCALE, 3 * lot);
        place_limit_order(u3, Side::Buy, 90 * PRICE_SCALE, 2 * lot);
        place_limit_order(u1, Side::Sell, 110 * PRICE_SCALE, 5 * lot);
        crate::process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        assert_eq!(
            get_order_book_ticker(icp_ckbtc_trading_pair().into()),
            Ok(OrderBookTicker {
                bid: Some(PriceLevel {
                    price: Nat::from(100 * PRICE_SCALE),
                    quantity: Nat::from(4 * lot),
                }),
                ask: Some(PriceLevel {
                    price: Nat::from(110 * PRICE_SCALE),
                    quantity: Nat::from(5 * lot),
                }),
            }),
        );
    }
}

mod get_order_book_depth {
    use crate::get_order_book_depth;
    use crate::test_fixtures::mocks::mock_runtime_for;
    use crate::test_fixtures::{
        LOT_SIZE, PRICE_SCALE, fund_user, icp_ckbtc_trading_pair, init_state_with_order_book,
        place_limit_order,
    };
    use candid::{Nat, Principal};
    use oisy_trade_types::{
        GetOrderBookDepthError, GetOrderBookDepthRequest, OrderBookDepth, PriceLevel, Side,
        TradingPair,
    };

    fn request(pair: TradingPair, limit: Option<u32>) -> GetOrderBookDepthRequest {
        GetOrderBookDepthRequest {
            trading_pair: pair,
            limit,
        }
    }

    fn level(price: u128, quantity: u64) -> PriceLevel {
        PriceLevel {
            price: Nat::from(price),
            quantity: Nat::from(quantity),
        }
    }

    #[test]
    fn should_return_unknown_trading_pair_for_unregistered() {
        let unknown_pair = TradingPair {
            base: Principal::from_slice(&[0xaa]),
            quote: Principal::from_slice(&[0xbb]),
        };
        init_state_with_order_book();
        assert_eq!(
            get_order_book_depth(request(unknown_pair, None)),
            Err(GetOrderBookDepthError::UnknownTradingPair),
        );
    }

    #[test]
    fn should_return_empty_depth_for_empty_book() {
        init_state_with_order_book();
        assert_eq!(
            get_order_book_depth(request(icp_ckbtc_trading_pair().into(), None)),
            Ok(OrderBookDepth {
                bids: vec![],
                asks: vec![],
            }),
        );
    }

    #[test]
    fn should_aggregate_across_orders_at_the_same_price() {
        init_state_with_order_book();
        let u1 = Principal::from_slice(&[0x01]);
        let u2 = Principal::from_slice(&[0x02]);
        let u3 = Principal::from_slice(&[0x03]);
        let u4 = Principal::from_slice(&[0x04]);
        fund_user(u1);
        fund_user(u2);
        fund_user(u3);
        fund_user(u4);

        let lot = u64::from(LOT_SIZE);
        place_limit_order(u1, Side::Buy, 100 * PRICE_SCALE, lot);
        place_limit_order(u2, Side::Buy, 100 * PRICE_SCALE, 3 * lot);
        place_limit_order(u3, Side::Buy, 90 * PRICE_SCALE, 2 * lot);
        place_limit_order(u4, Side::Sell, 110 * PRICE_SCALE, 5 * lot);
        crate::process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        assert_eq!(
            get_order_book_depth(request(icp_ckbtc_trading_pair().into(), None)),
            Ok(OrderBookDepth {
                bids: vec![
                    level(100 * PRICE_SCALE, 4 * lot),
                    level(90 * PRICE_SCALE, 2 * lot)
                ],
                asks: vec![level(110 * PRICE_SCALE, 5 * lot)],
            }),
        );
    }

    #[test]
    fn should_truncate_to_requested_limit() {
        init_state_with_order_book();
        let users: Vec<_> = (1u8..=3).map(|b| Principal::from_slice(&[b])).collect();
        for u in &users {
            fund_user(*u);
        }
        let lot = u64::from(LOT_SIZE);
        place_limit_order(users[0], Side::Buy, 100 * PRICE_SCALE, lot);
        place_limit_order(users[1], Side::Buy, 90 * PRICE_SCALE, lot);
        place_limit_order(users[2], Side::Buy, 80 * PRICE_SCALE, lot);
        crate::process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let depth =
            get_order_book_depth(request(icp_ckbtc_trading_pair().into(), Some(2))).unwrap();
        assert_eq!(
            depth.bids,
            vec![level(100 * PRICE_SCALE, lot), level(90 * PRICE_SCALE, lot)]
        );
        assert_eq!(depth.asks, vec![]);
    }

    #[test]
    fn should_default_to_100_when_limit_is_none() {
        // Place 101 bids at distinct prices so the default cuts 1 off.
        init_state_with_order_book();
        let lot = u64::from(LOT_SIZE);
        let tick = crate::test_fixtures::TICK_SIZE.get();
        for i in 0..101u64 {
            let user = Principal::from_slice(&(i as u16).to_be_bytes());
            fund_user(user);
            place_limit_order(user, Side::Buy, u128::from(i + 1) * tick, lot);
        }
        crate::process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let depth = get_order_book_depth(request(icp_ckbtc_trading_pair().into(), None)).unwrap();
        assert_eq!(depth.bids.len(), 100);
        assert_eq!(depth.asks.len(), 0);
    }

    #[test]
    fn should_reject_limit_above_max() {
        init_state_with_order_book();
        assert_eq!(
            get_order_book_depth(request(icp_ckbtc_trading_pair().into(), Some(1_001))),
            Err(GetOrderBookDepthError::LimitTooLarge {
                requested: 1_001,
                max: 1_000,
            }),
        );
    }

    #[test]
    fn should_accept_zero_limit_and_return_empty() {
        init_state_with_order_book();
        let user = Principal::from_slice(&[0x01]);
        fund_user(user);
        place_limit_order(user, Side::Buy, 100 * PRICE_SCALE, u64::from(LOT_SIZE));
        crate::process_pending_orders(&mock_runtime_for(Principal::anonymous()));

        let depth =
            get_order_book_depth(request(icp_ckbtc_trading_pair().into(), Some(0))).unwrap();
        assert_eq!(depth.bids, vec![]);
        assert_eq!(depth.asks, vec![]);
    }
}

mod get_my_orders {
    use crate::test_fixtures::mocks::mock_runtime_for;
    use crate::test_fixtures::{
        LOT_SIZE, fund_user, icp_ckbtc_trading_pair, init_state_with_order_book,
    };
    use crate::{GetMyOrdersError, add_limit_order, get_my_orders};
    use candid::{Nat, Principal};
    use oisy_trade_types::{
        GetMyOrdersArgs, LimitOrderRequest, MAX_ORDERS_PER_RESPONSE, OrderId, Side,
    };

    fn by_page(after: Option<OrderId>, length: u32) -> Option<GetMyOrdersArgs> {
        Some(GetMyOrdersArgs::by_page(after, length))
    }

    /// Places `count` resting buys for `user` and returns their ids in
    /// placement order, so `ids[0]` is the oldest and `ids[count - 1]` the
    /// newest.
    fn place_resting_buys(user: Principal, count: u32) -> Vec<OrderId> {
        fund_user(user);
        let runtime = mock_runtime_for(user);
        let ids = (0..count)
            .map(|_| {
                add_limit_order(
                    LimitOrderRequest {
                        pair: icp_ckbtc_trading_pair().into(),
                        side: Side::Buy,
                        price: Nat::from(100u64),
                        quantity: Nat::from(u64::from(LOT_SIZE)),
                    },
                    &runtime,
                )
                .unwrap()
            })
            .collect();
        crate::process_pending_orders(&mock_runtime_for(Principal::anonymous()));
        ids
    }

    #[test]
    fn rejects_malformed_cursor() {
        init_state_with_order_book();
        let result = get_my_orders(
            by_page(Some("not-a-valid-order-id".to_string()), 10),
            Principal::from_slice(&[0x01]),
        );
        assert!(matches!(result, Err(GetMyOrdersError::InvalidOrderId(_))));
    }

    #[test]
    fn caps_length_at_max_orders_per_response_and_paginates() {
        init_state_with_order_book();
        let user = Principal::from_slice(&[0x01]);
        // ids[0] is the oldest order, ids[MAX_ORDERS_PER_RESPONSE] the newest.
        let ids = place_resting_buys(user, MAX_ORDERS_PER_RESPONSE + 1);

        let page = |after| {
            get_my_orders(by_page(after, u32::MAX), user)
                .unwrap()
                .into_iter()
                .map(|o| o.id)
                .collect::<Vec<_>>()
        };

        // First page: clamped to MAX_ORDERS_PER_RESPONSE, newest-first — every
        // order but the oldest (ids[0]).
        let first = page(None);
        let expected_first: Vec<_> = (1..=MAX_ORDERS_PER_RESPONSE as usize)
            .rev()
            .map(|i| ids[i].clone())
            .collect();
        assert_eq!(first, expected_first);

        // Second page resumes after the first page's last id → just the oldest.
        let second = page(Some(first.last().unwrap().clone()));
        assert_eq!(second, vec![ids[0].clone()]);
    }

    #[test]
    fn zero_length_returns_empty_page() {
        init_state_with_order_book();
        let user = Principal::from_slice(&[0x01]);
        place_resting_buys(user, 1);

        let orders = get_my_orders(by_page(None, 0), user).unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn absent_args_default_to_first_page_newest_first() {
        init_state_with_order_book();
        let user = Principal::from_slice(&[0x01]);
        let ids = place_resting_buys(user, 3);

        let default_orders = get_my_orders(None, user).unwrap();

        let expected: Vec<_> = ids.iter().rev().cloned().collect();
        assert_eq!(
            default_orders
                .iter()
                .map(|o| o.id.clone())
                .collect::<Vec<_>>(),
            expected,
        );

        let explicit_orders = get_my_orders(by_page(None, MAX_ORDERS_PER_RESPONSE), user).unwrap();
        assert_eq!(default_orders, explicit_orders);
    }
}

mod get_trading_pairs {
    use crate::get_trading_pairs;
    use crate::order::{BasisPoint, FeeRates};
    use crate::state::init_state;
    use crate::test_fixtures;
    use crate::test_fixtures::{
        LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, TICK_SIZE, ckbtc_token_id, icp_token_id,
        init_state_with_order_book_and_fees,
    };
    use oisy_trade_types::TradingPairInfo;

    const MAKER_FEE_BPS: u16 = 7;
    const TAKER_FEE_BPS: u16 = 23;

    #[test]
    fn should_return_empty_when_no_trading_pairs() {
        init_state(test_fixtures::state_vmem());
        let pairs = get_trading_pairs();
        assert!(pairs.is_empty());
    }

    #[test]
    fn should_return_listed_trading_pairs() {
        init_state_with_order_book_and_fees(FeeRates {
            maker: BasisPoint::new(MAKER_FEE_BPS).unwrap(),
            taker: BasisPoint::new(TAKER_FEE_BPS).unwrap(),
        });

        let pairs = get_trading_pairs();

        assert_eq!(
            pairs,
            vec![TradingPairInfo {
                base: oisy_trade_types::Token {
                    id: oisy_trade_types::TokenId::from(icp_token_id()),
                    metadata: oisy_trade_types::TokenMetadata {
                        symbol: "ICP".to_string(),
                        decimals: 8,
                    },
                },
                quote: oisy_trade_types::Token {
                    id: oisy_trade_types::TokenId::from(ckbtc_token_id()),
                    metadata: oisy_trade_types::TokenMetadata {
                        symbol: "ckBTC".to_string(),
                        decimals: 8,
                    },
                },
                status: oisy_trade_types::TradingStatus::Trading,
                tick_size: candid::Nat::from(TICK_SIZE.get()),
                lot_size: LOT_SIZE.into(),
                maker_fee_bps: MAKER_FEE_BPS,
                taker_fee_bps: TAKER_FEE_BPS,
                min_notional: MIN_NOTIONAL.into(),
                max_notional: Some(MAX_NOTIONAL.into()),
            }]
        );
    }
}

mod get_balances {
    use crate::get_balances;
    use crate::state::reset_state;
    use crate::test_fixtures::arbitrary::arb_filter_tokens;
    use crate::test_fixtures::init_state_with_order_book;
    use candid::Principal;
    use oisy_trade_types::{GetBalancesRequestError, MAX_FILTER_LEN};
    use proptest::{prop_assert, prop_assert_eq, proptest};

    const USER: Principal = Principal::from_slice(&[0xAA]);

    proptest! {
        #[test]
        fn should_enforce_filter_length_cap(
            filter in arb_filter_tokens(0..=(MAX_FILTER_LEN as usize + 10)),
        ) {
            reset_state();
            init_state_with_order_book();
            let len = filter.len() as u32;

            let result = get_balances(Some(filter), USER);

            if len <= MAX_FILTER_LEN {
                prop_assert!(result.is_ok());
            } else {
                prop_assert_eq!(
                    result.unwrap_err(),
                    GetBalancesRequestError::FilterTooLarge {
                        len,
                        max: MAX_FILTER_LEN,
                    }
                );
            }
        }
    }
}

mod get_fee_balances {
    use crate::get_fee_balances;
    use crate::state::reset_state;
    use crate::test_fixtures::arbitrary::arb_filter_tokens;
    use crate::test_fixtures::init_state_with_order_book;
    use oisy_trade_types::{GetBalancesRequestError, MAX_FILTER_LEN};
    use proptest::{prop_assert, prop_assert_eq, proptest};

    proptest! {
        #[test]
        fn should_enforce_filter_length_cap(
            filter in arb_filter_tokens(0..=(MAX_FILTER_LEN as usize + 10)),
        ) {
            reset_state();
            init_state_with_order_book();
            let len = filter.len() as u32;

            let result = get_fee_balances(Some(filter));

            if len <= MAX_FILTER_LEN {
                prop_assert!(result.is_ok());
            } else {
                prop_assert_eq!(
                    result.unwrap_err(),
                    GetBalancesRequestError::FilterTooLarge {
                        len,
                        max: MAX_FILTER_LEN,
                    }
                );
            }
        }
    }
}

mod matching_timer_coalescing {
    use crate::state;
    use crate::test_fixtures::state_vmem;

    #[test]
    fn should_schedule_a_single_timer_until_it_fires() {
        state::init_state(state_vmem());
        state::with_state_mut(|s| {
            assert!(
                s.try_mark_matching_timer_scheduled(),
                "first kickoff schedules a timer"
            );
            assert!(
                !s.try_mark_matching_timer_scheduled(),
                "a burst of further kickoffs must not schedule more timers"
            );

            // The timer fires and clears the flag, allowing the next kickoff
            // (or the `MoreWork` self-reschedule) to arm a fresh timer.
            s.clear_matching_timer_scheduled();
            assert!(
                s.try_mark_matching_timer_scheduled(),
                "a kickoff after the timer fired schedules again"
            );
        });
    }
}

mod set_halt {
    use crate::test_fixtures::mocks::{MockRuntime, mock_runtime_for};
    use crate::test_fixtures::{ckbtc_token_id, icp_token_id, init_state_with_order_book};
    use crate::{MAX_HALT_BOOKS, halt_trading, resume_trading};
    use candid::Principal;
    use oisy_trade_types::TradingPair;

    #[test]
    fn should_accept_max_halt_books_pairs() {
        init_state_with_order_book();
        let runtime = controller_runtime();
        let pairs = vec![registered_pair(); MAX_HALT_BOOKS];

        assert_eq!(halt_trading(Some(pairs.clone()), &runtime), Ok(()));
        assert_eq!(resume_trading(Some(pairs), &runtime), Ok(()));
    }

    fn registered_pair() -> TradingPair {
        TradingPair {
            base: *icp_token_id().as_principal(),
            quote: *ckbtc_token_id().as_principal(),
        }
    }

    fn controller_runtime() -> MockRuntime {
        let mut mock = mock_runtime_for(Principal::anonymous());
        mock.expect_is_controller().return_const(true);
        mock
    }
}
