use assert_matches::assert_matches;
use candid::{Nat, Principal};
use dex_client::{DexClient, Runtime};
use dex_int_tests::icrc_ledger::BASE_LEDGER_FEE;
use dex_int_tests::{LOT_SIZE, Setup, TICK_SIZE};
use dex_types::{
    AddTradingPairError, AddTradingPairRequest, Balance, DepositError, DepositRequest,
    LedgerTransferFromError, LimitOrderRequest, Side, Token, TokenId, TokenMetadata,
    TradingPairInfo, WithdrawError, WithdrawRequest,
};
use dex_types_internal::log::Priority;
use icrc_ledger_types::icrc1::account::Account;

fn expected_balance(free: u64) -> Balance {
    Balance {
        free: Nat::from(free),
        reserved: Nat::from(0u64),
    }
}

#[allow(clippy::too_many_arguments)]
async fn assert_balances<R: Runtime>(
    client1: &DexClient<R>,
    client2: &DexClient<R>,
    cksol: &TokenId,
    ckbtc: &TokenId,
    expected_user1_cksol: u64,
    expected_user1_ckbtc: u64,
    expected_user2_cksol: u64,
    expected_user2_ckbtc: u64,
) {
    assert_eq!(
        client1.get_balance(cksol.clone()).await,
        expected_balance(expected_user1_cksol),
        "user1 ckSOL balance mismatch"
    );
    assert_eq!(
        client1.get_balance(ckbtc.clone()).await,
        expected_balance(expected_user1_ckbtc),
        "user1 ckBTC balance mismatch"
    );
    assert_eq!(
        client2.get_balance(cksol.clone()).await,
        expected_balance(expected_user2_cksol),
        "user2 ckSOL balance mismatch"
    );
    assert_eq!(
        client2.get_balance(ckbtc.clone()).await,
        expected_balance(expected_user2_ckbtc),
        "user2 ckBTC balance mismatch"
    );
}

mod add_limit_order {
    use assert_matches::assert_matches;
    use candid::{Encode, Principal};
    use dex_int_tests::Setup;
    use dex_int_tests::icrc_ledger::{BASE_LEDGER_FEE, QUOTE_LEDGER_FEE};
    use dex_types::{AddLimitOrderError, Balance, LimitOrderRequest, OrderStatus, Side};
    use pocket_ic::{RejectCode, RejectResponse};

    #[tokio::test]
    async fn should_add_limit_buy_order_and_query_status() {
        let setup = Setup::new().await.with_trading_pair().await;
        let client = setup.dex_client();
        let token_id = setup.quote_token_id();
        let fee = QUOTE_LEDGER_FEE;
        // buy 1M base tokens for a price of 100 quote tokens per base token
        // need 100M quote tokens
        let order = LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Buy,
            price: 100,
            quantity: 1_000_000u64.into(),
        };

        let required = 100_000_000u64;
        assert_eq!(
            client.add_limit_order(order.clone()).await,
            Err(AddLimitOrderError::InsufficientBalance {
                token: token_id.clone(),
                available: 0u64.into(),
                required: required.into(),
            })
        );

        setup
            .deposit_flow(setup.user(), token_id.clone())
            .mint(required + 2 * fee)
            .approve(required + fee)
            .deposit(required)
            .execute()
            .await;

        let order_id = client.add_limit_order(order).await.unwrap();
        assert_eq!(
            client.get_balance(token_id).await,
            Balance {
                free: 0u64.into(),
                reserved: required.into(),
            }
        );
        // The matching timer fires eagerly after placement; with no counterparty
        // the order rests in the book as Open.
        assert_eq!(client.get_order_status(order_id).await, OrderStatus::Open);

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_add_limit_sell_order_and_query_status() {
        let setup = Setup::new().await.with_trading_pair().await;
        let client = setup.dex_client();
        let token_id = setup.base_token_id();
        let fee = BASE_LEDGER_FEE;
        // sell 1M base tokens at a price of 100 quote tokens per base token
        // need 1M base tokens
        let order = LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Sell,
            price: 100,
            quantity: 1_000_000u64.into(),
        };

        let required = 1_000_000u64;
        assert_eq!(
            client.add_limit_order(order.clone()).await,
            Err(AddLimitOrderError::InsufficientBalance {
                token: token_id.clone(),
                available: 0u64.into(),
                required: required.into(),
            })
        );

        setup
            .deposit_flow(setup.user(), token_id.clone())
            .mint(required + 2 * fee)
            .approve(required + fee)
            .deposit(required)
            .execute()
            .await;

        let order_id = client.add_limit_order(order).await.unwrap();
        assert_eq!(
            client.get_balance(token_id).await,
            Balance {
                free: 0u64.into(),
                reserved: required.into(),
            }
        );
        // The matching timer fires eagerly after placement; with no counterparty
        // the order rests in the book as Open.
        assert_eq!(client.get_order_status(order_id).await, OrderStatus::Open);

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_fail_to_get_order_status() {
        let setup = Setup::new().await;

        // Valid hex format but non-existent order
        let not_found = setup
            .dex_client()
            .get_order_status("ffffffffffffffffffffffffffffffff".to_string())
            .await;
        assert_eq!(not_found, OrderStatus::NotFound);

        let result = setup
            .env()
            .query_call(
                setup.dex_id(),
                Principal::anonymous(),
                "get_order_status",
                Encode!(&"not-a-valid-id".to_string()).unwrap(),
            )
            .await;

        assert_matches!(
            result,
            Err(RejectResponse { reject_code: RejectCode::CanisterError, reject_message, .. })
            if reject_message.contains("invalid order ID")
        );

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_match_crossing_orders() {
        let setup = Setup::new().await.with_trading_pair().await;
        let buyer = Principal::from_slice(&[0x01]);
        let buyer_client = setup.dex_client_with_caller(buyer);
        let seller = Principal::from_slice(&[0x02]);
        let seller_client = setup.dex_client_with_caller(seller);

        // buy 1M base tokens for a price of 100 quote tokens per base token
        // need 100M quote tokens
        let buy_order = LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Buy,
            price: 100,
            quantity: 1_000_000u64.into(),
        };
        let required_quote_amount = 100_000_000u64;
        setup
            .deposit_flow(buyer, setup.quote_token_id())
            .mint(required_quote_amount + 2 * QUOTE_LEDGER_FEE)
            .approve(required_quote_amount + QUOTE_LEDGER_FEE)
            .deposit(required_quote_amount)
            .execute()
            .await;
        let buy_order_id = buyer_client
            .add_limit_order(buy_order.clone())
            .await
            .unwrap();

        // sell 1M base tokens at a price of 100 quote tokens per base token
        // need 1M base tokens
        let sell_order = LimitOrderRequest {
            pair: setup.trading_pair(),
            side: Side::Sell,
            price: 100,
            quantity: 1_000_000u64.into(),
        };
        let required_base_amount = 1_000_000u64;
        setup
            .deposit_flow(seller, setup.base_token_id())
            .mint(required_base_amount + 2 * BASE_LEDGER_FEE)
            .approve(required_base_amount + BASE_LEDGER_FEE)
            .deposit(required_base_amount)
            .execute()
            .await;
        let sell_order_id = seller_client.add_limit_order(sell_order).await.unwrap();

        setup.env().tick().await;

        // Both orders are fully filled
        assert_eq!(
            setup.dex_client().get_order_status(buy_order_id).await,
            OrderStatus::Filled
        );
        assert_eq!(
            setup.dex_client().get_order_status(sell_order_id).await,
            OrderStatus::Filled
        );

        // Buyer: received 1M base tokens, spent 100M quote tokens
        assert_eq!(
            buyer_client.get_balance(setup.base_token_id()).await,
            Balance {
                free: required_base_amount.into(),
                reserved: 0u64.into()
            },
        );
        assert_eq!(
            buyer_client.get_balance(setup.quote_token_id()).await,
            Balance {
                free: 0u64.into(),
                reserved: 0u64.into()
            },
        );

        // Seller: spent 1M base tokens, received 100M quote tokens
        assert_eq!(
            seller_client.get_balance(setup.base_token_id()).await,
            Balance {
                free: 0u64.into(),
                reserved: 0u64.into()
            },
        );
        assert_eq!(
            seller_client.get_balance(setup.quote_token_id()).await,
            Balance {
                free: required_quote_amount.into(),
                reserved: 0u64.into()
            },
        );

        setup.drop().await;
    }
}

#[tokio::test]
async fn should_return_empty_trading_pairs() {
    let setup = Setup::new().await;
    let client = setup.dex_client();
    assert_eq!(client.get_trading_pairs().await, vec![]);

    let setup = setup.with_trading_pair().await;
    let client = setup.dex_client();

    assert_eq!(
        client.get_trading_pairs().await,
        vec![TradingPairInfo {
            base: Token {
                id: setup.base_token_id(),
                metadata: TokenMetadata {
                    symbol: "ckSOL".to_string(),
                    decimals: 9,
                },
            },
            quote: Token {
                id: setup.quote_token_id(),
                metadata: TokenMetadata {
                    symbol: "ckBTC".to_string(),
                    decimals: 8,
                },
            },
            tick_size: TICK_SIZE,
            lot_size: LOT_SIZE,
        }]
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_have_different_ledger_configs() {
    let setup = Setup::new().await;
    let base = setup.base_token_ledger();
    let quote = setup.quote_token_ledger();

    let base_decimals = base.icrc1_decimals().await;
    let quote_decimals = quote.icrc1_decimals().await;
    assert_ne!(base_decimals, quote_decimals);

    let base_fee = base.icrc1_fee().await;
    let quote_fee = quote.icrc1_fee().await;
    assert_ne!(base_fee, quote_fee);

    setup.drop().await;
}

#[tokio::test]
async fn should_deposit_and_track_balances() {
    let setup = Setup::new().await.with_trading_pair().await;

    let user1 = Principal::from_slice(&[0x01]);
    let user2 = Principal::from_slice(&[0x02]);

    let cksol = TokenId {
        ledger_id: setup.base_ledger_id(),
    };
    let ckbtc = TokenId {
        ledger_id: setup.quote_ledger_id(),
    };

    let dex_account = Account {
        owner: setup.dex_id(),
        subaccount: None,
    };

    // Mint tokens to users
    setup
        .mint_base_tokens(user1, Nat::from(10_000_000u64))
        .await;
    setup
        .mint_base_tokens(user2, Nat::from(20_000_000u64))
        .await;
    setup
        .mint_quote_tokens(user1, Nat::from(10_000_000u64))
        .await;
    setup
        .mint_quote_tokens(user2, Nat::from(20_000_000u64))
        .await;

    // Approve DEX canister to spend on behalf of users
    let base_ledger = setup.base_token_ledger();
    let quote_ledger = setup.quote_token_ledger();
    base_ledger
        .icrc2_approve(user1, dex_account, Nat::from(5_000_000u64))
        .await;
    base_ledger
        .icrc2_approve(user2, dex_account, Nat::from(5_000_000u64))
        .await;
    quote_ledger
        .icrc2_approve(user1, dex_account, Nat::from(5_000_000u64))
        .await;
    quote_ledger
        .icrc2_approve(user2, dex_account, Nat::from(5_000_000u64))
        .await;

    let client1 = setup.dex_client_with_caller(user1);
    let client2 = setup.dex_client_with_caller(user2);

    // Verify initial balances are zero
    assert_balances(&client1, &client2, &cksol, &ckbtc, 0, 0, 0, 0).await;

    // Deposit ckSOL for user1
    client1
        .deposit(DepositRequest {
            token_id: cksol.clone(),
            amount: Nat::from(1_000_000u64),
        })
        .await
        .expect("user1 ckSOL deposit failed");
    assert_balances(&client1, &client2, &cksol, &ckbtc, 1_000_000, 0, 0, 0).await;

    // Deposit ckBTC for user1
    client1
        .deposit(DepositRequest {
            token_id: ckbtc.clone(),
            amount: Nat::from(500_000u64),
        })
        .await
        .expect("user1 ckBTC deposit failed");
    assert_balances(&client1, &client2, &cksol, &ckbtc, 1_000_000, 500_000, 0, 0).await;

    // Deposit ckSOL for user2
    client2
        .deposit(DepositRequest {
            token_id: cksol.clone(),
            amount: Nat::from(2_000_000u64),
        })
        .await
        .expect("user2 ckSOL deposit failed");
    assert_balances(
        &client1, &client2, &cksol, &ckbtc, 1_000_000, 500_000, 2_000_000, 0,
    )
    .await;

    // Deposit ckBTC for user2
    client2
        .deposit(DepositRequest {
            token_id: ckbtc.clone(),
            amount: Nat::from(3_000_000u64),
        })
        .await
        .expect("user2 ckBTC deposit failed");
    assert_balances(
        &client1, &client2, &cksol, &ckbtc, 1_000_000, 500_000, 2_000_000, 3_000_000,
    )
    .await;

    setup.drop().await;
}

/// Test parameters for deposit failure scenarios.
struct DepositFailureCase {
    mint_amount: u64,
    approve_amount: u64,
    deposit_amount: u64,
    expected_error: fn(fee: Nat) -> DepositError,
}

async fn test_deposit_failure(case: DepositFailureCase) {
    let setup = Setup::new().await.with_trading_pair().await;

    let user = Principal::from_slice(&[0x03]);
    let cksol = TokenId {
        ledger_id: setup.base_ledger_id(),
    };
    let dex_account = Account {
        owner: setup.dex_id(),
        subaccount: None,
    };
    let fee = setup.base_token_ledger().icrc1_fee().await;

    setup
        .mint_base_tokens(user, Nat::from(case.mint_amount))
        .await;
    setup
        .base_token_ledger()
        .icrc2_approve(user, dex_account, Nat::from(case.approve_amount))
        .await;

    let result = setup
        .dex_client_with_caller(user)
        .deposit(DepositRequest {
            token_id: cksol,
            amount: Nat::from(case.deposit_amount),
        })
        .await;

    assert_eq!(result, Err((case.expected_error)(fee)));

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_deposit_with_insufficient_funds() {
    test_deposit_failure(DepositFailureCase {
        mint_amount: 1_000_000,
        approve_amount: 5_000_000,
        deposit_amount: 2_000_000,
        expected_error: |fee| {
            DepositError::LedgerError(LedgerTransferFromError::InsufficientFunds {
                // The user's balance is the minted amount minus the fee charged for icrc2_approve
                balance: Nat::from(1_000_000u64) - fee,
            })
        },
    })
    .await;
}

#[tokio::test]
async fn should_fail_deposit_with_insufficient_allowance() {
    test_deposit_failure(DepositFailureCase {
        mint_amount: 10_000_000,
        approve_amount: 500_000,
        deposit_amount: 1_000_000,
        expected_error: |_fee| {
            DepositError::LedgerError(LedgerTransferFromError::InsufficientAllowance {
                allowance: Nat::from(500_000u64),
            })
        },
    })
    .await;
}

#[tokio::test]
async fn should_fail_deposit_with_unsupported_token() {
    let setup = Setup::new().await;

    let user = Principal::from_slice(&[0x05]);
    let fake_token = TokenId {
        ledger_id: setup.dex_id(),
    };

    let result = setup
        .dex_client_with_caller(user)
        .deposit(DepositRequest {
            token_id: fake_token.clone(),
            amount: Nat::from(1_000_000u64),
        })
        .await;

    assert_eq!(
        result,
        Err(DepositError::UnsupportedToken {
            token_id: fake_token,
        })
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_deposit_when_ledger_is_stopped() {
    let setup = Setup::new().await.with_trading_pair().await;

    let user = Principal::from_slice(&[0x06]);
    let controller = setup.controller();
    let ledger_id = setup.base_ledger_id();

    // Stop the ledger so it rejects all incoming messages
    setup
        .env()
        .stop_canister(ledger_id, Some(controller))
        .await
        .expect("Failed to stop canister");

    let result = setup
        .dex_client_with_caller(user)
        .deposit(DepositRequest {
            token_id: TokenId { ledger_id },
            amount: Nat::from(1_000_000u64),
        })
        .await;

    assert_matches!(
        result,
        Err(DepositError::CallFailed { reason, .. }) if reason.contains("is stopped")
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_add_trading_pair() {
    let setup = Setup::new().await;
    let controller_client = setup.dex_client_with_caller(setup.controller());
    let user = Principal::from_slice(&[0x01]);
    let user_client = setup.dex_client_with_caller(user);

    // not controller
    let result = user_client
        .add_trading_pair(setup.add_trading_pair_request())
        .await;
    assert_eq!(result, Err(AddTradingPairError::NotController));

    // base equals quote
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            base: Token {
                id: TokenId {
                    ledger_id: setup.base_ledger_id(),
                },
                metadata: TokenMetadata {
                    symbol: "ckSOL".to_string(),
                    decimals: 9,
                },
            },
            quote: Token {
                id: TokenId {
                    ledger_id: setup.base_ledger_id(),
                },
                metadata: TokenMetadata {
                    symbol: "ckSOL".to_string(),
                    decimals: 9,
                },
            },
            ..setup.add_trading_pair_request()
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::BaseEqualsQuote));

    // zero tick size
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            tick_size: 0,
            ..setup.add_trading_pair_request()
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::InvalidTickSize));

    // zero lot size
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            lot_size: 0,
            ..setup.add_trading_pair_request()
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::InvalidLotSize));

    // already exists
    let result = controller_client
        .add_trading_pair(setup.add_trading_pair_request())
        .await;
    assert_eq!(result, Ok(()));

    let result = controller_client
        .add_trading_pair(setup.add_trading_pair_request())
        .await;
    assert_eq!(result, Err(AddTradingPairError::TradingPairAlreadyExists));

    setup.drop().await;
}

#[tokio::test]
async fn should_replay_events_on_upgrade() {
    use dex_int_tests::icrc_ledger::BASE_LEDGER_FEE;
    use dex_types_internal::event::EventType;

    // 1) Init -> Upgrade -> no trading pairs
    let setup = Setup::new().await;
    setup.upgrade(None).await;
    assert_eq!(setup.dex_client().get_trading_pairs().await, vec![]);
    setup.assert_that_events().await.satisfy(|events| {
        assert_eq!(events.len(), 1);
        assert_matches!(&events[0], EventType::Init(_));
    });

    // 2) Add trading pair -> Upgrade -> trading pair preserved
    setup.add_trading_pair().await;
    let pairs_before = setup.dex_client().get_trading_pairs().await;
    assert_eq!(pairs_before.len(), 1);
    setup.upgrade(None).await;
    assert_eq!(setup.dex_client().get_trading_pairs().await, pairs_before);
    setup.assert_that_events().await.satisfy(|events| {
        assert_eq!(events.len(), 2);
        assert_matches!(&events[1], EventType::AddTradingPair(e) => {
            assert_eq!(e.base, setup.base_token_id());
            assert_eq!(e.quote, setup.quote_token_id());
            assert_eq!(e.tick_size, TICK_SIZE);
            assert_eq!(e.lot_size, LOT_SIZE);
            assert_eq!(e.base_metadata, TokenMetadata { symbol: "ckSOL".to_string(), decimals: 9 });
            assert_eq!(e.quote_metadata, TokenMetadata { symbol: "ckBTC".to_string(), decimals: 8 });
        });
    });

    // 3) Deposit -> Upgrade -> balance preserved
    let deposit_amount: u64 = 1_000_000;
    setup
        .deposit_flow(setup.user(), setup.base_token_id())
        .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
        .approve(deposit_amount + BASE_LEDGER_FEE)
        .deposit(deposit_amount)
        .execute()
        .await;
    let balance_before = setup.dex_client().get_balance(setup.base_token_id()).await;
    assert_eq!(balance_before.free, Nat::from(deposit_amount));
    setup.upgrade(None).await;
    let balance_after = setup.dex_client().get_balance(setup.base_token_id()).await;
    assert_eq!(balance_after, balance_before);
    setup.assert_that_events().await.satisfy(|events| {
        assert_eq!(events.len(), 3);
        assert_matches!(&events[2], EventType::Deposit(e) => {
            assert_eq!(e.user, setup.user());
            assert_eq!(e.token, setup.base_token_id());
            assert_eq!(e.amount, Nat::from(deposit_amount));
        });
    });

    setup.drop().await;
}

#[tokio::test]
async fn should_withdraw_and_receive_tokens_on_ledger() {
    let setup = Setup::new().await.with_trading_pair().await;
    let user = Principal::from_slice(&[0x01]);
    let client = setup.dex_client_with_caller(user);
    let cksol = TokenId {
        ledger_id: setup.base_ledger_id(),
    };
    let fee = Nat::from(BASE_LEDGER_FEE);

    // Deposit tokens first
    let deposit_amount = 10_000_000u64;
    setup
        .deposit_flow(user, cksol.clone())
        .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
        .approve(deposit_amount + BASE_LEDGER_FEE)
        .deposit(deposit_amount)
        .execute()
        .await;
    assert_eq!(
        client.get_balance(cksol.clone()).await,
        expected_balance(deposit_amount)
    );

    // Withdraw half
    let withdraw_amount = 5_000_000u64;
    let result = client
        .withdraw(WithdrawRequest {
            token_id: cksol.clone(),
            amount: Nat::from(withdraw_amount),
        })
        .await;
    assert!(result.is_ok(), "withdrawal should succeed: {result:?}");

    // DEX balance decreased by the full withdraw amount
    assert_eq!(
        client.get_balance(cksol.clone()).await,
        expected_balance(deposit_amount - withdraw_amount)
    );

    // User received (withdraw_amount - fee) on the ledger
    let ledger_balance = setup.base_token_ledger().icrc1_balance_of(user).await;
    assert_eq!(ledger_balance, Nat::from(withdraw_amount) - fee);

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_withdraw_on_negative_cases() {
    let setup = Setup::new().await.with_trading_pair().await;
    let cksol = setup.base_token_id();
    let fee = Nat::from(BASE_LEDGER_FEE);

    // --- Unsupported token: token not part of any trading pair ---
    {
        let unknown_token = TokenId {
            ledger_id: Principal::from_slice(&[0xFF]),
        };
        let result = setup
            .dex_client_with_caller(Principal::from_slice(&[0x0F]))
            .withdraw(WithdrawRequest {
                token_id: unknown_token.clone(),
                amount: Nat::from(1_000_000u64),
            })
            .await;

        assert_eq!(
            result,
            Err(WithdrawError::UnsupportedToken {
                token_id: unknown_token,
            })
        );
    }

    // --- Zero balance: withdraw should fail with InsufficientBalance ---
    {
        let user = Principal::from_slice(&[0x10]);
        let client = setup.dex_client_with_caller(user);

        // Withdraw an amount larger than the fee so the AmountTooSmall check passes,
        // and the balance check is reached.
        let result = client
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: Nat::from(1_000_000u64),
            })
            .await;

        assert_eq!(
            result,
            Err(WithdrawError::InsufficientBalance {
                available: Nat::from(0u64),
            })
        );
    }

    // --- Insufficient balance: withdraw more than deposited ---
    {
        let user = Principal::from_slice(&[0x11]);
        let client = setup.dex_client_with_caller(user);

        let deposit_amount = 1_000_000u64;
        setup
            .deposit_flow(user, cksol.clone())
            .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
            .approve(deposit_amount + BASE_LEDGER_FEE)
            .deposit(deposit_amount)
            .execute()
            .await;

        let result = client
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: Nat::from(2_000_000u64),
            })
            .await;

        assert_eq!(
            result,
            Err(WithdrawError::InsufficientBalance {
                available: Nat::from(deposit_amount),
            })
        );

        assert_eq!(
            client.get_balance(cksol.clone()).await,
            expected_balance(deposit_amount)
        );
    }

    // --- Amount too small: withdraw exactly the fee ---
    {
        let user = Principal::from_slice(&[0x12]);
        let client = setup.dex_client_with_caller(user);

        let deposit_amount = fee.clone();
        setup
            .deposit_flow(user, cksol.clone())
            .mint(deposit_amount.clone() + 2 * BASE_LEDGER_FEE)
            .approve(deposit_amount.clone() + BASE_LEDGER_FEE)
            .deposit(deposit_amount.clone())
            .execute()
            .await;

        let result = client
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: fee.clone(),
            })
            .await;

        assert_eq!(
            result,
            Err(WithdrawError::AmountTooSmall {
                min_amount: fee.clone() + 1u64,
            })
        );
    }

    // --- Reserved balance: all funds locked in an open order ---
    {
        let user = Principal::from_slice(&[0x13]);
        let client = setup.dex_client_with_caller(user);

        let deposit_amount = 1_000_000u64;
        setup
            .deposit_flow(user, cksol.clone())
            .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
            .approve(deposit_amount + BASE_LEDGER_FEE)
            .deposit(deposit_amount)
            .execute()
            .await;

        client
            .add_limit_order(LimitOrderRequest {
                pair: setup.trading_pair(),
                side: Side::Sell,
                price: 100,
                quantity: Nat::from(deposit_amount),
            })
            .await
            .unwrap();

        assert_eq!(
            client.get_balance(cksol.clone()).await,
            Balance {
                free: 0u64.into(),
                reserved: deposit_amount.into(),
            }
        );

        let result = client
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: Nat::from(deposit_amount),
            })
            .await;

        assert_eq!(
            result,
            Err(WithdrawError::InsufficientBalance {
                available: Nat::from(0u64),
            })
        );

        assert_eq!(
            client.get_balance(cksol.clone()).await,
            Balance {
                free: 0u64.into(),
                reserved: deposit_amount.into(),
            }
        );
    }

    // --- Ledger stopped (must be last since it alters global state) ---
    {
        let user = Principal::from_slice(&[0x14]);

        let deposit_amount = 1_000_000u64;
        setup
            .deposit_flow(user, cksol.clone())
            .mint(deposit_amount + 2 * BASE_LEDGER_FEE)
            .approve(deposit_amount + BASE_LEDGER_FEE)
            .deposit(deposit_amount)
            .execute()
            .await;

        setup
            .env()
            .stop_canister(setup.base_ledger_id(), Some(setup.controller()))
            .await
            .expect("Failed to stop canister");

        let result = setup
            .dex_client_with_caller(user)
            .withdraw(WithdrawRequest {
                token_id: cksol.clone(),
                amount: Nat::from(500_000u64),
            })
            .await;

        assert_matches!(
            result,
            Err(WithdrawError::CallFailed { reason, .. }) if reason.contains("is stopped")
        );

        assert_eq!(
            setup.dex_client_with_caller(user).get_balance(cksol).await,
            expected_balance(deposit_amount)
        );
    }

    setup.drop().await;
}

#[tokio::test]
async fn should_get_events() {
    let setup = Setup::new().await;

    setup.assert_that_events().await.satisfy(|events| {
        assert_eq!(events.len(), 1);
        assert_matches!(events[0], dex_types_internal::event::EventType::Init(_));
    });

    setup.drop().await;
}

#[tokio::test]
async fn should_get_logs() {
    let setup = Setup::new().await;

    let logs = setup.retrieve_logs(&Priority::Info).await;

    assert_eq!(logs.len(), 1);
    assert!(logs[0].message.contains("[init]"));

    setup.drop().await;
}
