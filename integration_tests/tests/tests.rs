use assert_matches::assert_matches;
use candid::{Nat, Principal};
use dex_client::{DexClient, Runtime};
use dex_int_tests::{LOT_SIZE, Setup, TICK_SIZE};
use dex_types::{
    AddTradingPairError, AddTradingPairRequest, Balance, DepositError, DepositRequest,
    LedgerTransferFromError, Token, TokenId, TradingPairInfo,
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
            quantity: 1_000_000,
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
            quantity: 1_000_000,
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
            quantity: 1_000_000,
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
            quantity: 1_000_000,
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

        // TODO DEFI-2740: Both orders are fully filled and should still be tracked
        // TODO DEFI-2740: verify user's balances
        assert_eq!(
            setup.dex_client().get_order_status(buy_order_id).await,
            OrderStatus::NotFound
        );
        assert_eq!(
            setup.dex_client().get_order_status(sell_order_id).await,
            OrderStatus::NotFound
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
                symbol: "ckSOL".to_string(),
                decimals: 9,
            },
            quote: Token {
                id: setup.quote_token_id(),
                symbol: "ckBTC".to_string(),
                decimals: 8,
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
    let setup = Setup::new().await;

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
    let setup = Setup::new().await;

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
async fn should_fail_deposit_when_ledger_is_dex_canister() {
    // TODO(DEFI-2741): Remove or modify this test once we have a proper check for supported tokens.
    let setup = Setup::new().await;

    let user = Principal::from_slice(&[0x05]);
    let fake_token = TokenId {
        ledger_id: setup.dex_id(),
    };

    let result = setup
        .dex_client_with_caller(user)
        .deposit(DepositRequest {
            token_id: fake_token,
            amount: Nat::from(1_000_000u64),
        })
        .await;

    assert_matches!(
        result,
        Err(DepositError::CallFailed { reason, .. }) if reason.contains("Canister has no update method 'icrc2_transfer_from'")
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_deposit_when_ledger_is_stopped() {
    let setup = Setup::new().await;

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
                symbol: "ckSOL".to_string(),
                decimals: 9,
            },
            quote: Token {
                id: TokenId {
                    ledger_id: setup.base_ledger_id(),
                },
                symbol: "ckSOL".to_string(),
                decimals: 9,
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
async fn should_get_logs() {
    let setup = Setup::new().await;

    let logs = setup.retrieve_logs(&Priority::Info).await;

    assert_eq!(logs.len(), 1);
    assert!(logs[0].message.contains("[init]"));

    setup.drop().await;
}
