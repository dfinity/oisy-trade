use crate::{
    AddLimitOrderError, Balance, LimitOrderRequest, OrderStatus, Side, TokenId, TradingPair,
    TradingPairInfo,
};
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
fn should_serialize_trading_pair_info() {
    let info = TradingPairInfo {
        base_asset: TokenId {
            ledger_id: Principal::from_slice(&[0x01]),
        },
        quote_asset: TokenId {
            ledger_id: Principal::from_slice(&[0x02]),
        },
        tick_size: 10,
        lot_size: 1_000_000,
    };
    let encoded = candid::encode_one(&info).unwrap();
    let decoded: TradingPairInfo = candid::decode_one(&encoded).unwrap();
    assert_eq!(info, decoded);
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

#[test]
fn should_serialize_balance() {
    let balance = Balance {
        free: candid::Nat::from(1_000_000_u64),
        reserved: candid::Nat::from(500_000_u64),
    };
    let encoded = candid::encode_one(&balance).unwrap();
    let decoded: Balance = candid::decode_one(&encoded).unwrap();
    assert_eq!(balance, decoded);
}

#[test]
fn should_serialize_add_limit_order_error() {
    for error in [
        AddLimitOrderError::UnknownTradingPair,
        AddLimitOrderError::InvalidPrice {
            price: 7,
            tick_size: 10,
        },
        AddLimitOrderError::InvalidQuantity {
            quantity: 500_000,
            lot_size: 1_000_000,
        },
    ] {
        let encoded = candid::encode_one(&error).unwrap();
        let decoded: AddLimitOrderError = candid::decode_one(&encoded).unwrap();
        assert_eq!(error, decoded);
    }
}
