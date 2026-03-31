use crate::{LimitOrderRequest, LimitOrderResponse, OrderStatus, Side, TradingPair};
use candid::Principal;

fn test_trading_pair() -> TradingPair {
    TradingPair {
        base: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
        quote: Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap(),
    }
}

#[test]
fn should_serialize_limit_order_request() {
    let request = LimitOrderRequest {
        pair: test_trading_pair(),
        side: Side::Buy,
        price: 100,
        quantity: 1_000_000,
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
        OrderStatus::Canceled,
    ] {
        let encoded = candid::encode_one(&status).unwrap();
        let decoded: OrderStatus = candid::decode_one(&encoded).unwrap();
        assert_eq!(status, decoded);
    }
}

#[test]
fn should_serialize_side() {
    for side in [Side::Buy, Side::Sell] {
        let encoded = candid::encode_one(side).unwrap();
        let decoded: Side = candid::decode_one(&encoded).unwrap();
        assert_eq!(side, decoded);
    }
}

#[test]
fn should_serialize_trading_pair() {
    let pair = test_trading_pair();
    let encoded = candid::encode_one(pair).unwrap();
    let decoded: TradingPair = candid::decode_one(&encoded).unwrap();
    assert_eq!(pair, decoded);
}
