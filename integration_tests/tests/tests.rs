use candid::{Encode, Principal};
use dex_int_tests::Setup;
use dex_types::{LimitOrderRequest, OrderStatus, Side, TradingPair};

#[tokio::test]
async fn should_add_limit_order_and_query_status() {
    let setup = Setup::new().await;
    let client = setup.client();

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

    assert!(
        result.is_err(),
        "expected canister to trap on invalid order ID"
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
