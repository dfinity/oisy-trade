use dex_int_tests::Setup;
use dex_types::{DummyRequest, DummyResponse};

#[tokio::test]
async fn should_greet() {
    let setup = Setup::new().await;
    let client = setup.client();

    let response = client
        .greet(DummyRequest {
            input: "world".to_string(),
        })
        .await;

    assert_eq!(
        response,
        DummyResponse {
            output: "Hello, world!".to_string()
        }
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
