use dex_int_tests::Setup;
use dex_types::{LimitOrderRequest, OrderStatus};

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
