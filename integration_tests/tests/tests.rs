use candid::{Nat, Principal};
use dex_int_tests::Setup;
use dex_types::{DepositRequest, LimitOrderRequest, OrderStatus, Token};
use icrc_ledger_types::icrc1::account::Account;

#[tokio::test]
async fn should_add_limit_order_and_query_status() {
    let setup = Setup::new().await;
    let client = setup.client();

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
        ledger_canister_id: setup.base_ledger_id(),
    };
    let ckbtc = Token {
        ledger_canister_id: setup.quote_ledger_id(),
    };

    let dex_account = Account {
        owner: setup.canister_id(),
        subaccount: None,
    };

    let base_ledger = setup.base_token_ledger();
    let quote_ledger = setup.quote_token_ledger();

    // Mint tokens to users
    base_ledger
        .icrc1_transfer(
            setup.controller(),
            Account {
                owner: user1,
                subaccount: None,
            },
            Nat::from(10_000_000u64),
        )
        .await;
    base_ledger
        .icrc1_transfer(
            setup.controller(),
            Account {
                owner: user2,
                subaccount: None,
            },
            Nat::from(20_000_000u64),
        )
        .await;
    quote_ledger
        .icrc1_transfer(
            setup.controller(),
            Account {
                owner: user1,
                subaccount: None,
            },
            Nat::from(10_000_000u64),
        )
        .await;
    quote_ledger
        .icrc1_transfer(
            setup.controller(),
            Account {
                owner: user2,
                subaccount: None,
            },
            Nat::from(20_000_000u64),
        )
        .await;

    // Approve DEX canister to spend on behalf of users
    base_ledger
        .icrc2_approve(user1, dex_account, Nat::from(u64::MAX))
        .await;
    base_ledger
        .icrc2_approve(user2, dex_account, Nat::from(u64::MAX))
        .await;
    quote_ledger
        .icrc2_approve(user1, dex_account, Nat::from(u64::MAX))
        .await;
    quote_ledger
        .icrc2_approve(user2, dex_account, Nat::from(u64::MAX))
        .await;

    let client1 = setup.client_with_caller(user1);
    let client2 = setup.client_with_caller(user2);

    // Deposit ckSOL and ckBTC for both users
    client1
        .deposit(DepositRequest {
            token: cksol.clone(),
            amount: Nat::from(1_000_000u64),
        })
        .await
        .expect("user1 ckSOL deposit failed");
    client1
        .deposit(DepositRequest {
            token: ckbtc.clone(),
            amount: Nat::from(500_000u64),
        })
        .await
        .expect("user1 ckBTC deposit failed");
    client2
        .deposit(DepositRequest {
            token: cksol.clone(),
            amount: Nat::from(2_000_000u64),
        })
        .await
        .expect("user2 ckSOL deposit failed");
    client2
        .deposit(DepositRequest {
            token: ckbtc.clone(),
            amount: Nat::from(1_000_000u64),
        })
        .await
        .expect("user2 ckBTC deposit failed");

    // Verify balances
    assert_eq!(
        client1.get_balance(cksol.clone()).await,
        Nat::from(1_000_000u64)
    );
    assert_eq!(
        client1.get_balance(ckbtc.clone()).await,
        Nat::from(500_000u64)
    );
    assert_eq!(
        client2.get_balance(cksol.clone()).await,
        Nat::from(2_000_000u64)
    );
    assert_eq!(
        client2.get_balance(ckbtc.clone()).await,
        Nat::from(1_000_000u64)
    );

    // Cross-check: user1 has no balance for a token they didn't interact with beyond what was deposited
    // and user2's ckSOL balance is independent of user1's
    assert_ne!(
        client1.get_balance(cksol.clone()).await,
        client2.get_balance(cksol.clone()).await
    );

    setup.drop().await;
}
