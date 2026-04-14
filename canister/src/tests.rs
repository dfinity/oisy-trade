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
    fn should_return_filled_after_matching() {
        init_state_with_order_book();
        let buyer = Principal::from_slice(&[0x01]);
        let seller = Principal::from_slice(&[0x02]);
        fund_user(buyer);
        fund_user(seller);
        let mut buyer_rt = MockRuntime::new();
        buyer_rt.expect_msg_caller().return_const(buyer);
        let mut seller_rt = MockRuntime::new();
        seller_rt.expect_msg_caller().return_const(seller);

        let buy_id = add_limit_order(limit_order_request(), &buyer_rt).unwrap();
        let mut sell_request = limit_order_request();
        sell_request.side = dex_types::Side::Sell;
        let sell_id = add_limit_order(sell_request, &seller_rt).unwrap();

        crate::process_pending_orders();

        assert_eq!(get_order_status(buy_id), OrderStatus::Filled);
        assert_eq!(get_order_status(sell_id), OrderStatus::Filled);
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

mod withdraw {
    use crate::order::{Quantity, TokenId};
    use crate::test_fixtures::mocks::MockRuntime;
    use crate::{state, withdraw};
    use candid::{Nat, Principal, encode_args};
    use dex_types::{LedgerTransferError, WithdrawError, WithdrawRequest};
    use ic_cdk::call::Response;
    use icrc_ledger_types::icrc1::transfer::TransferError;
    use mockall::Sequence;

    const USER: Principal = Principal::from_slice(&[0x42]);
    const TOKEN_LEDGER: Principal = Principal::from_slice(&[0xAA]);

    fn token_id() -> dex_types::TokenId {
        dex_types::TokenId {
            ledger_id: TOKEN_LEDGER,
        }
    }

    fn init_state_with_balance(amount: u64) {
        state::init_state(
            state::State::try_from(dex_types_internal::InitArg {
                mode: dex_types_internal::Mode::GeneralAvailability,
            })
            .unwrap(),
        );
        state::with_state_mut(|s| {
            s.record_token(
                TokenId::from(token_id()),
                crate::order::TokenMetadata {
                    symbol: "TEST".to_string(),
                    decimals: 8,
                },
            );
            s.deposit(USER, TokenId::from(token_id()), Quantity::from(amount));
        });
    }

    /// Construct a [`Response`] from Candid-encoded bytes.
    ///
    /// `Response` has a private field, but is a newtype over `Vec<u8>` with
    /// identical layout. This is test-only code; the transmute is sound because
    /// the struct contains a single `Vec<u8>` field.
    fn mock_response(bytes: Vec<u8>) -> Response {
        assert_eq!(
            std::mem::size_of::<Response>(),
            std::mem::size_of::<Vec<u8>>(),
            "Response layout changed — update this helper"
        );
        unsafe { std::mem::transmute::<Vec<u8>, Response>(bytes) }
    }

    fn transfer_response(result: Result<Nat, TransferError>) -> Response {
        mock_response(encode_args((result,)).unwrap())
    }

    fn mock_runtime_returning(responses: Vec<Response>) -> MockRuntime {
        let mut runtime = MockRuntime::new();
        runtime.expect_msg_caller().return_const(USER);

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

    #[tokio::test]
    async fn should_return_error_when_fee_changes_between_retries() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        // First call: BadFee(5000). Second call: BadFee(9999).
        let runtime = mock_runtime_returning(vec![
            transfer_response(Err(TransferError::BadFee {
                expected_fee: Nat::from(5_000u64),
            })),
            transfer_response(Err(TransferError::BadFee {
                expected_fee: Nat::from(9_999u64),
            })),
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
            result,
            Err(WithdrawError::LedgerError(
                LedgerTransferError::InternalError(
                    "ledger fee changed between retries".to_string()
                )
            ))
        );
        // Balance credited back.
        assert_balance(deposit);
        // Fee cache updated from the most recent BadFee.
        assert_cached_fee(9_999);
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
            result,
            Err(WithdrawError::LedgerError(
                LedgerTransferError::TemporarilyUnavailable
            ))
        );
        assert_balance(deposit);
        assert_cached_fee(5_000);
    }

    #[tokio::test]
    async fn should_return_insufficient_funds_error() {
        let deposit = 1_000_000u64;
        init_state_with_balance(deposit);

        // The ledger says the DEX canister doesn't hold enough tokens.
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
            result,
            Err(WithdrawError::LedgerError(
                LedgerTransferError::InsufficientFunds {
                    balance: Nat::from(0u64)
                }
            ))
        );
        assert_balance(deposit);
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
        let mut runtime = MockRuntime::new();
        runtime.expect_msg_caller().return_const(USER);
        let mut seq = Sequence::new();
        // First attempt: fee capped to amount - 1, transfer_amount = 1.
        runtime
            .expect_call_unbounded_wait()
            .times(1)
            .in_sequence(&mut seq)
            .withf(|canister_id, method, _args| {
                canister_id == &TOKEN_LEDGER && method == "icrc1_transfer"
            })
            .return_once(move |_, _, _| {
                Ok(transfer_response(Err(TransferError::BadFee {
                    expected_fee: Nat::from(real_fee),
                })))
            });
        // Retry: fee = real_fee (100), transfer_amount = 199_900.
        let block_index_clone = block_index.clone();
        runtime
            .expect_call_unbounded_wait()
            .times(1)
            .in_sequence(&mut seq)
            .withf(move |canister_id, method, _args| {
                canister_id == &TOKEN_LEDGER && method == "icrc1_transfer"
            })
            .return_once(move |_, _, _| Ok(transfer_response(Ok(block_index_clone))));

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
            Ok(dex_types::WithdrawResponse {
                block_index: block_index.clone()
            })
        );
        assert_balance(deposit - withdraw_amount);
        assert_cached_fee(real_fee);
    }

    #[tokio::test]
    async fn should_reject_unsupported_token() {
        // Init state without registering any token.
        state::init_state(
            state::State::try_from(dex_types_internal::InitArg {
                mode: dex_types_internal::Mode::GeneralAvailability,
            })
            .unwrap(),
        );

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
            result,
            Err(WithdrawError::UnsupportedToken {
                token_id: token_id(),
            })
        );
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
            result,
            Err(WithdrawError::AmountTooSmall {
                min_amount: Nat::from(1u64),
            })
        );
        // Balance untouched — no debit happened.
        assert_balance(deposit);
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
        let runtime = mock_runtime_returning(vec![transfer_response(Err(TransferError::BadFee {
            expected_fee: Nat::from(real_fee),
        }))]);

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
            Err(WithdrawError::AmountTooSmall {
                min_amount: Nat::from(real_fee + 1),
            })
        );
        // Balance credited back.
        assert_balance(deposit);
        // Fee cache updated to the real fee from the BadFee response.
        assert_cached_fee(real_fee);
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
