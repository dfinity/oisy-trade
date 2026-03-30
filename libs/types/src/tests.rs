use crate::{LimitOrderRequest, LimitOrderResponse, OrderStatus, Side};

#[test]
fn should_serialize_limit_order_request() {
    let request = LimitOrderRequest {
        side: Side::Buy,
        price: 100,
        quantity: 10,
    };
    let encoded = candid::encode_one(&request).unwrap();
    let decoded: LimitOrderRequest = candid::decode_one(&encoded).unwrap();
    assert_eq!(request, decoded);
}

#[test]
fn should_serialize_limit_order_response() {
    let response = LimitOrderResponse { order_id: 42 };
    let encoded = candid::encode_one(&response).unwrap();
    let decoded: LimitOrderResponse = candid::decode_one(&encoded).unwrap();
    assert_eq!(response, decoded);
}

#[test]
fn should_serialize_order_status() {
    for status in [
        OrderStatus::NotFound,
        OrderStatus::Pending,
        OrderStatus::Open,
        OrderStatus::Filled,
        OrderStatus::Cancelled,
    ] {
        let encoded = candid::encode_one(&status).unwrap();
        let decoded: OrderStatus = candid::decode_one(&encoded).unwrap();
        assert_eq!(status, decoded);
    }
}

#[test]
fn should_serialize_side() {
    for side in [Side::Buy, Side::Sell] {
        let encoded = candid::encode_one(&side).unwrap();
        let decoded: Side = candid::decode_one(&encoded).unwrap();
        assert_eq!(side, decoded);
    }
}
