use candid::Nat;
use dex_int_tests::Setup;
use dex_types::{LimitOrderRequest, OrderStatus, Token};

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
async fn should_return_no_supported_tokens_initially() {
    let setup = Setup::new().await;
    let client = setup.client();

    let tokens = client.get_supported_tokens().await;
    assert!(tokens.is_empty());

    setup.drop().await;
}

#[tokio::test]
async fn should_add_and_get_supported_tokens() {
    let setup = Setup::new().await;
    let client = setup.client();

    let base = setup.base_token_ledger();
    let quote = setup.quote_token_ledger();

    let cksol = Token {
        name: "ckSOL".to_string(),
        symbol: "ckSOL".to_string(),
        decimals: base.icrc1_decimals().await,
        ledger_id: setup.base_ledger_id(),
        fee: base.icrc1_fee().await,
    };
    let ckbtc = Token {
        name: "ckBTC".to_string(),
        symbol: "ckBTC".to_string(),
        decimals: quote.icrc1_decimals().await,
        ledger_id: setup.quote_ledger_id(),
        fee: quote.icrc1_fee().await,
    };

    client.add_supported_token(cksol.clone()).await;
    client.add_supported_token(ckbtc.clone()).await;

    let tokens = client.get_supported_tokens().await;
    assert_eq!(tokens.len(), 2);
    assert!(tokens.contains(&cksol));
    assert!(tokens.contains(&ckbtc));

    setup.drop().await;
}

#[tokio::test]
async fn should_not_duplicate_supported_token() {
    let setup = Setup::new().await;
    let client = setup.client();

    let token = Token {
        name: "ckSOL".to_string(),
        symbol: "ckSOL".to_string(),
        decimals: 9,
        ledger_id: setup.base_ledger_id(),
        fee: Nat::from(5_000u64),
    };

    client.add_supported_token(token.clone()).await;
    client.add_supported_token(token).await;

    let tokens = client.get_supported_tokens().await;
    assert_eq!(tokens.len(), 1);

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
