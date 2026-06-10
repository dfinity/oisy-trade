use crate::{
    AddLimitOrderError, Balance, CanceledOrderInfo, GetOrderBookDepthError,
    GetOrderBookDepthRequest, GetOrderBookTickerError, LimitOrderRequest, OrderBookDepth,
    OrderBookTicker, OrderStatus, PriceLevel, Side, Token, TokenId, TokenMetadata, TradingPair,
    TradingPairInfo,
};
use candid::{Nat, Principal};

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
        price: Nat::from(100u64),
        quantity: Nat::from(1_000_000u64),
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
        OrderStatus::Canceled(CanceledOrderInfo {
            remaining_quantity: Nat::from(0u64),
        }),
    ] {
        let encoded = candid::encode_one(&status).unwrap();
        let decoded: OrderStatus = candid::decode_one(&encoded).unwrap();
        assert_eq!(status, decoded);
    }
}

#[test]
fn should_serialize_trading_pair_info() {
    let info = TradingPairInfo {
        base: Token {
            id: TokenId {
                ledger_id: Principal::from_slice(&[0x01]),
            },
            metadata: TokenMetadata {
                symbol: "ckSOL".to_string(),
                decimals: 9,
            },
        },
        quote: Token {
            id: TokenId {
                ledger_id: Principal::from_slice(&[0x02]),
            },
            metadata: TokenMetadata {
                symbol: "ckBTC".to_string(),
                decimals: 8,
            },
        },
        tick_size: Nat::from(10u64),
        lot_size: 1_000_000,
    };
    let encoded = candid::encode_one(&info).unwrap();
    let decoded: TradingPairInfo = candid::decode_one(&encoded).unwrap();
    assert_eq!(info, decoded);
}

#[test]
fn should_serialize_token() {
    let token = Token {
        id: TokenId {
            ledger_id: Principal::from_slice(&[0x01]),
        },
        metadata: TokenMetadata {
            symbol: "ckBTC".to_string(),
            decimals: 8,
        },
    };
    let encoded = candid::encode_one(&token).unwrap();
    let decoded: Token = candid::decode_one(&encoded).unwrap();
    assert_eq!(token, decoded);
}

#[test]
fn should_serialize_token_metadata() {
    let metadata = TokenMetadata {
        symbol: "ckBTC".to_string(),
        decimals: 8,
    };
    let encoded = candid::encode_one(&metadata).unwrap();
    let decoded: TokenMetadata = candid::decode_one(&encoded).unwrap();
    assert_eq!(metadata, decoded);
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
fn should_serialize_order_book_ticker() {
    for ticker in [
        OrderBookTicker {
            bid: None,
            ask: None,
        },
        OrderBookTicker {
            bid: Some(PriceLevel {
                price: Nat::from(100u64),
                quantity: Nat::from(500_000u64),
            }),
            ask: Some(PriceLevel {
                price: Nat::from(110u64),
                quantity: Nat::from(300_000u64),
            }),
        },
    ] {
        let encoded = candid::encode_one(&ticker).unwrap();
        let decoded: OrderBookTicker = candid::decode_one(&encoded).unwrap();
        assert_eq!(ticker, decoded);
    }
}

#[test]
fn should_serialize_order_book_depth() {
    let depth = OrderBookDepth {
        bids: vec![
            PriceLevel {
                price: Nat::from(100u64),
                quantity: Nat::from(1_000u64),
            },
            PriceLevel {
                price: Nat::from(99u64),
                quantity: Nat::from(2_000u64),
            },
        ],
        asks: vec![PriceLevel {
            price: Nat::from(101u64),
            quantity: Nat::from(500u64),
        }],
    };
    let encoded = candid::encode_one(&depth).unwrap();
    let decoded: OrderBookDepth = candid::decode_one(&encoded).unwrap();
    assert_eq!(depth, decoded);
}

#[test]
fn should_serialize_get_order_book_depth_request() {
    for request in [
        GetOrderBookDepthRequest {
            trading_pair: test_trading_pair(),
            limit: None,
        },
        GetOrderBookDepthRequest {
            trading_pair: test_trading_pair(),
            limit: Some(50),
        },
    ] {
        let encoded = candid::encode_one(&request).unwrap();
        let decoded: GetOrderBookDepthRequest = candid::decode_one(&encoded).unwrap();
        assert_eq!(request, decoded);
    }
}

#[test]
fn should_serialize_get_order_book_ticker_error() {
    let err = GetOrderBookTickerError::UnknownTradingPair;
    let encoded = candid::encode_one(&err).unwrap();
    let decoded: GetOrderBookTickerError = candid::decode_one(&encoded).unwrap();
    assert_eq!(err, decoded);
}

#[test]
fn should_serialize_get_order_book_depth_error() {
    for err in [
        GetOrderBookDepthError::UnknownTradingPair,
        GetOrderBookDepthError::LimitTooLarge {
            requested: 5_000,
            max: 1_000,
        },
    ] {
        let encoded = candid::encode_one(&err).unwrap();
        let decoded: GetOrderBookDepthError = candid::decode_one(&encoded).unwrap();
        assert_eq!(err, decoded);
    }
}

#[test]
fn should_serialize_add_limit_order_error() {
    for error in [
        AddLimitOrderError::UnknownTradingPair,
        AddLimitOrderError::InvalidPrice {
            price: Nat::from(7u64),
            tick_size: Nat::from(10u64),
        },
        AddLimitOrderError::InvalidQuantity {
            quantity: Nat::from(500_000u64),
            lot_size: 1_000_000,
        },
    ] {
        let encoded = candid::encode_one(&error).unwrap();
        let decoded: AddLimitOrderError = candid::decode_one(&encoded).unwrap();
        assert_eq!(error, decoded);
    }
}
