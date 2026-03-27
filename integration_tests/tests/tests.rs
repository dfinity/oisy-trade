use dex_int_tests::Setup;
use dex_types::{LimitOrderRequest, LimitOrderResponse, OrderStatus};

#[tokio::test]
async fn should_add_limit_order_and_query_status() {
    let setup = Setup::new().await;
    let client = setup.client();

    let response = client.add_limit_order(LimitOrderRequest {}).await;
    assert_eq!(response, LimitOrderResponse { order_id: 0 });

    let status = client.get_order_status(response.order_id).await;
    assert_eq!(status, OrderStatus::Pending);

    let not_found = client.get_order_status(999).await;
    assert_eq!(not_found, OrderStatus::NotFound);

    setup.drop().await;
}
