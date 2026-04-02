use assert_matches::assert_matches;
use candid::{Nat, Principal};
use dex_client::{DexClient, Runtime};
use dex_int_tests::Setup;
use dex_types::{
    AddTradingPairError, AddTradingPairRequest, Balance, DepositError, DepositRequest,
    LedgerTransferFromError, TokenId,
};
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
    use dex_int_tests::{Setup, test_trading_pair};
    use dex_types::{LimitOrderRequest, OrderStatus, Side, TradingPair};
    use pocket_ic::{RejectCode, RejectResponse};

    #[tokio::test]
    async fn should_add_limit_order_and_query_status() {
        let setup = Setup::new().await;
        let client = setup.dex_client();

        let order_id = client
            .add_limit_order(LimitOrderRequest {
                pair: TradingPair {
                    base: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
                    quote: Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap(),
                },
                side: Side::Buy,
                price: 100,
                quantity: 1_000_000,
            })
            .await
            .unwrap();

        let status = client.get_order_status(order_id).await;
        assert_eq!(status, OrderStatus::Pending);

        // Valid hex format but non-existent order
        let not_found = client
            .get_order_status("ffffffffffffffffffffffffffffffff".to_string())
            .await;
        assert_eq!(not_found, OrderStatus::NotFound);

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_trap_on_syntactically_invalid_order_id() {
        let setup = Setup::new().await;

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
    async fn should_reject_invalid_orders() {
        let setup = Setup::new().await;
        let client = setup.dex_client();
        let pair = test_trading_pair();

        let cases = vec![
            (
                "unknown trading pair",
                LimitOrderRequest {
                    pair: TradingPair {
                        base: Principal::management_canister(),
                        quote: Principal::management_canister(),
                    },
                    side: Side::Buy,
                    price: 100,
                    quantity: 1_000_000,
                },
                dex_types::AddLimitOrderError::UnknownTradingPair,
            ),
            (
                "price not a multiple of tick size",
                LimitOrderRequest {
                    pair,
                    side: Side::Buy,
                    price: 7,
                    quantity: 1_000_000,
                },
                dex_types::AddLimitOrderError::InvalidPrice {
                    price: 7,
                    tick_size: 10,
                },
            ),
            (
                "zero price",
                LimitOrderRequest {
                    pair,
                    side: Side::Buy,
                    price: 0,
                    quantity: 1_000_000,
                },
                dex_types::AddLimitOrderError::InvalidPrice {
                    price: 0,
                    tick_size: 10,
                },
            ),
            (
                "quantity not a multiple of lot size",
                LimitOrderRequest {
                    pair,
                    side: Side::Sell,
                    price: 100,
                    quantity: 500_000,
                },
                dex_types::AddLimitOrderError::InvalidQuantity {
                    quantity: 500_000,
                    lot_size: 1_000_000,
                },
            ),
            (
                "zero quantity",
                LimitOrderRequest {
                    pair,
                    side: Side::Sell,
                    price: 100,
                    quantity: 0,
                },
                dex_types::AddLimitOrderError::InvalidQuantity {
                    quantity: 0,
                    lot_size: 1_000_000,
                },
            ),
        ];

        for (name, request, expected_error) in cases {
            let result = client.add_limit_order(request).await;
            assert_eq!(result, Err(expected_error), "case: {name}");
        }

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_match_crossing_orders() {
        let setup = Setup::new().await;
        let client = setup.dex_client();
        let pair = test_trading_pair();

        let sell_id = client
            .add_limit_order(LimitOrderRequest {
                pair,
                side: Side::Sell,
                price: 100,
                quantity: 1_000_000,
            })
            .await
            .unwrap();
        let buy_id = client
            .add_limit_order(LimitOrderRequest {
                pair,
                side: Side::Buy,
                price: 100,
                quantity: 1_000_000,
            })
            .await
            .unwrap();

        // Tick to let the zero-duration matching timers fire
        setup.env().tick().await;

        // Both orders are fully filled and no longer tracked
        // TODO DEFI-2740: verify user's balances
        assert_eq!(
            client.get_order_status(sell_id).await,
            OrderStatus::NotFound
        );
        assert_eq!(client.get_order_status(buy_id).await, OrderStatus::NotFound);

        setup.drop().await;
    }

    #[tokio::test]
    async fn should_rest_unmatched_order_as_open() {
        let setup = Setup::new().await;
        let client = setup.dex_client();

        let order_id = client
            .add_limit_order(LimitOrderRequest {
                pair: test_trading_pair(),
                side: Side::Buy,
                price: 100,
                quantity: 1_000_000,
            })
            .await
            .unwrap();

        // Tick to let the zero-duration matching timer fire
        setup.env().tick().await;

        // No counterparty — order rests in the book as Open
        // TODO DEFI-2740: verify user's balances
        assert_eq!(client.get_order_status(order_id).await, OrderStatus::Open);

        setup.drop().await;
    }
}

#[tokio::test]
async fn should_return_empty_trading_pairs() {
    let setup = Setup::new().await;
    let client = setup.dex_client();

    let pairs = client.get_trading_pairs().await;
    // TODO DEFI-2723: there should only be a trading pair if one was added by an admin.
    // Currently it's hard-coded in the init args.
    assert!(!pairs.is_empty());

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

fn add_trading_pair_request(setup: &Setup) -> AddTradingPairRequest {
    AddTradingPairRequest {
        base: TokenId {
            ledger_id: setup.base_ledger_id(),
        },
        quote: TokenId {
            ledger_id: setup.quote_ledger_id(),
        },
        tick_size: 10,
        lot_size: 1_000_000,
    }
}

#[tokio::test]
async fn should_add_trading_pair_as_controller() {
    let setup = Setup::new().await;
    let controller_client = setup.dex_client_with_caller(setup.controller());

    let result = controller_client
        .add_trading_pair(add_trading_pair_request(&setup))
        .await;
    assert_eq!(result, Ok(()));

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
        .add_trading_pair(add_trading_pair_request(&setup))
        .await;
    assert_eq!(result, Err(AddTradingPairError::NotController));

    // base equals quote
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            base: TokenId {
                ledger_id: setup.base_ledger_id(),
            },
            quote: TokenId {
                ledger_id: setup.base_ledger_id(),
            },
            tick_size: 10,
            lot_size: 1_000_000,
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::BaseEqualsQuote));

    // zero tick size
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            tick_size: 0,
            ..add_trading_pair_request(&setup)
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::InvalidTickSize));

    // zero lot size
    let result = controller_client
        .add_trading_pair(AddTradingPairRequest {
            lot_size: 0,
            ..add_trading_pair_request(&setup)
        })
        .await;
    assert_eq!(result, Err(AddTradingPairError::InvalidLotSize));

    // already exists
    let result = controller_client
        .add_trading_pair(add_trading_pair_request(&setup))
        .await;
    assert_eq!(result, Ok(()));

    let result = controller_client
        .add_trading_pair(add_trading_pair_request(&setup))
        .await;
    assert_eq!(result, Err(AddTradingPairError::TradingPairAlreadyExists));

    setup.drop().await;
}
