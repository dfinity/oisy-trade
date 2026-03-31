use assert_matches::assert_matches;
use candid::{Encode, Nat, Principal};
use dex_client::{DexClient, Runtime};
use dex_int_tests::Setup;
use dex_types::{
    DepositError, DepositRequest, LedgerTransferFromError, LimitOrderRequest, OrderStatus, Token,
};
use icrc_ledger_types::icrc1::account::Account;

#[allow(clippy::too_many_arguments)]
async fn assert_balances<R: Runtime>(
    client1: &DexClient<R>,
    client2: &DexClient<R>,
    cksol: &Token,
    ckbtc: &Token,
    expected_user1_cksol: u64,
    expected_user1_ckbtc: u64,
    expected_user2_cksol: u64,
    expected_user2_ckbtc: u64,
) {
    assert_eq!(
        client1.get_balance(cksol.clone()).await,
        Nat::from(expected_user1_cksol),
        "user1 ckSOL balance mismatch"
    );
    assert_eq!(
        client1.get_balance(ckbtc.clone()).await,
        Nat::from(expected_user1_ckbtc),
        "user1 ckBTC balance mismatch"
    );
    assert_eq!(
        client2.get_balance(cksol.clone()).await,
        Nat::from(expected_user2_cksol),
        "user2 ckSOL balance mismatch"
    );
    assert_eq!(
        client2.get_balance(ckbtc.clone()).await,
        Nat::from(expected_user2_ckbtc),
        "user2 ckBTC balance mismatch"
    );
}

#[tokio::test]
async fn should_add_limit_order_and_query_status() {
    let setup = Setup::new().await;
    let client = setup.dex_client();

    let response = client.add_limit_order(LimitOrderRequest {}).await;

    let status = client.get_order_status(response.order_id).await;
    assert_eq!(status, OrderStatus::Pending);

    let not_found = client.get_order_status(u64::MAX).await;
    assert_eq!(not_found, OrderStatus::NotFound);

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

    let cksol = Token {
        ledger_id: setup.base_ledger_id(),
    };
    let ckbtc = Token {
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
            token: cksol.clone(),
            amount: Nat::from(1_000_000u64),
        })
        .await
        .expect("user1 ckSOL deposit failed");
    assert_balances(&client1, &client2, &cksol, &ckbtc, 1_000_000, 0, 0, 0).await;

    // Deposit ckBTC for user1
    client1
        .deposit(DepositRequest {
            token: ckbtc.clone(),
            amount: Nat::from(500_000u64),
        })
        .await
        .expect("user1 ckBTC deposit failed");
    assert_balances(&client1, &client2, &cksol, &ckbtc, 1_000_000, 500_000, 0, 0).await;

    // Deposit ckSOL for user2
    client2
        .deposit(DepositRequest {
            token: cksol.clone(),
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
            token: ckbtc.clone(),
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

#[tokio::test]
async fn should_fail_deposit_with_insufficient_funds() {
    let setup = Setup::new().await;

    let user = Principal::from_slice(&[0x03]);
    let cksol = Token {
        ledger_id: setup.base_ledger_id(),
    };
    let dex_account = Account {
        owner: setup.dex_id(),
        subaccount: None,
    };
    let fee = setup.base_token_ledger().icrc1_fee().await;
    let minted = Nat::from(1_000_000u64);

    // Mint only 1_000_000 tokens to the user
    setup.mint_base_tokens(user, minted.clone()).await;

    // Approve more than the user holds
    setup
        .base_token_ledger()
        .icrc2_approve(user, dex_account, Nat::from(5_000_000u64))
        .await;

    // Try to deposit more than the user holds
    let result = setup
        .dex_client_with_caller(user)
        .deposit(DepositRequest {
            token: cksol,
            amount: Nat::from(2_000_000u64),
        })
        .await;

    // The user's balance is the minted amount minus the fee charged for icrc2_approve
    assert_eq!(
        result,
        Err(DepositError::LedgerError(
            LedgerTransferFromError::InsufficientFunds {
                balance: minted - fee,
            }
        ))
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_deposit_with_insufficient_allowance() {
    let setup = Setup::new().await;

    let user = Principal::from_slice(&[0x04]);
    let cksol = Token {
        ledger_id: setup.base_ledger_id(),
    };
    let dex_account = Account {
        owner: setup.dex_id(),
        subaccount: None,
    };
    // Mint plenty of tokens to the user
    setup.mint_base_tokens(user, Nat::from(10_000_000u64)).await;

    // Approve only 500_000
    setup
        .base_token_ledger()
        .icrc2_approve(user, dex_account, Nat::from(500_000u64))
        .await;

    // Try to deposit more than the allowance
    let result = setup
        .dex_client_with_caller(user)
        .deposit(DepositRequest {
            token: cksol,
            amount: Nat::from(1_000_000u64),
        })
        .await;

    assert_eq!(
        result,
        Err(DepositError::LedgerError(
            LedgerTransferFromError::InsufficientAllowance {
                allowance: Nat::from(500_000u64),
            }
        ))
    );

    setup.drop().await;
}

#[tokio::test]
async fn should_fail_deposit_when_ledger_is_dex_canister() {
    let setup = Setup::new().await;

    let user = Principal::from_slice(&[0x05]);
    let fake_token = Token {
        ledger_id: setup.dex_id(),
    };

    let result = setup
        .dex_client_with_caller(user)
        .deposit(DepositRequest {
            token: fake_token,
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
async fn should_fail_deposit_when_ledger_has_no_cycles() {
    let setup = Setup::new().await;

    let user = Principal::from_slice(&[0x06]);
    let controller = setup.controller();

    // Install a ledger with no cycles added
    let env = setup.env();
    let ledger_id = env
        .create_canister_with_settings(
            None,
            Some(pocket_ic::CanisterSettings {
                controllers: Some(vec![controller]),
                ..pocket_ic::CanisterSettings::default()
            }),
        )
        .await;
    // Add enough cycles to install the canister, but not enough to allow for an infinite freezing threshold.
    env.add_cycles(ledger_id, 1_000_000_000_000).await;
    let ledger_arg = dex_int_tests::icrc_ledger::LedgerArg::Init(Box::new(
        dex_int_tests::icrc_ledger::cksol_init_args(controller),
    ));
    env.install_canister(
        ledger_id,
        dex_int_tests::ledger_wasm(),
        Encode!(&ledger_arg).unwrap(),
        Some(controller),
    )
    .await;

    // Set freezing_threshold to u64::MAX so the subnet considers the canister frozen
    env.update_canister_settings(
        ledger_id,
        Some(controller),
        pocket_ic::CanisterSettings {
            freezing_threshold: Some(Nat::from(u64::MAX)),
            ..pocket_ic::CanisterSettings::default()
        },
    )
    .await
    .expect("Failed to update canister settings");

    let token = Token { ledger_id };

    let result = setup
        .dex_client_with_caller(user)
        .deposit(DepositRequest {
            token,
            amount: Nat::from(1_000_000u64),
        })
        .await;

    assert_matches!(
        result,
        Err(DepositError::CallFailed { reason, .. }) if reason.contains("out of cycles")
    );

    setup.drop().await;
}
