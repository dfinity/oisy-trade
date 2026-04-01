use crate::{LimitOrderRequest, LimitOrderResponse, OrderStatus, TradingPairInfo};
use candid::Principal;

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
fn should_serialize_trading_pair_info() {
    let info = TradingPairInfo {
        base_asset: Principal::from_slice(&[0x01]),
        quote_asset: Principal::from_slice(&[0x02]),
        tick_size: 10,
        lot_size: 1_000_000,
    };
    let encoded = candid::encode_one(&info).unwrap();
    let decoded: TradingPairInfo = candid::decode_one(&encoded).unwrap();
    assert_eq!(info, decoded);
}
