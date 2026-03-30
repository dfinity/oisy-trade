use crate::{LimitOrderRequest, LimitOrderResponse, OrderStatus, Token};

#[test]
fn should_serialize_limit_order_request() {
    let request = LimitOrderRequest {};
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
    for status in [OrderStatus::NotFound, OrderStatus::Pending] {
        let encoded = candid::encode_one(&status).unwrap();
        let decoded: OrderStatus = candid::decode_one(&encoded).unwrap();
        assert_eq!(status, decoded);
    }
}

#[test]
fn should_serialize_token() {
    let token = Token {
        name: "Internet Computer".to_string(),
        symbol: "ICP".to_string(),
        decimals: 8,
        ledger_id: candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
        fee: candid::Nat::from(10_000_u64),
    };
    let encoded = candid::encode_one(&token).unwrap();
    let decoded: Token = candid::decode_one(&encoded).unwrap();
    assert_eq!(token, decoded);
}
