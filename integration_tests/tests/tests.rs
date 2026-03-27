use dex_int_tests::Setup;
use dex_types::{LimitOrderRequest, LimitOrderResponse, OrderStatus};

#[tokio::test]
async fn should_add_limit_order() {
    let setup = Setup::new().await;
    let client = setup.client();

    let response = client.add_limit_order(LimitOrderRequest {}).await;

    assert_eq!(response, LimitOrderResponse { order_id: 0 });

    setup.drop().await;
}

#[tokio::test]
async fn should_return_pending_for_existing_order() {
    let setup = Setup::new().await;
    let client = setup.client();

    let response = client.add_limit_order(LimitOrderRequest {}).await;
    let status = client.get_order_status(response.order_id).await;

    assert_eq!(status, OrderStatus::Pending);

    setup.drop().await;
}

#[tokio::test]
async fn should_return_not_found_for_nonexistent_order() {
    let setup = Setup::new().await;
    let client = setup.client();

    let status = client.get_order_status(999).await;

    assert_eq!(status, OrderStatus::NotFound);

    setup.drop().await;
}

#[tokio::test]
async fn should_assign_incrementing_order_ids() {
    let setup = Setup::new().await;
    let client = setup.client();

    let first = client.add_limit_order(LimitOrderRequest {}).await;
    let second = client.add_limit_order(LimitOrderRequest {}).await;

    assert_eq!(first.order_id, 0);
    assert_eq!(second.order_id, 1);

    setup.drop().await;
}
