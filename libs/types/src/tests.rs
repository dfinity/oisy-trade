use crate::{
    Balance, DepositRequest, GetMyOrdersArgs, GetMyOrdersFilter, GetMyOrdersPage,
    GetOrderBookDepthRequest, LimitOrderRequest, OrderBookDepth, OrderBookTicker, OrderRecord,
    OrderStatus, PriceLevel, Side, TimeInForce, Token, TokenId, TokenMetadata, TradingPair,
    TradingPairInfo, TradingStatus, WithdrawRequest,
};
use candid::{Nat, Principal};

const KNOWN_PRINCIPAL_TEXT: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";

fn test_trading_pair() -> TradingPair {
    TradingPair {
        base: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
        quote: Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap(),
    }
}

#[test]
fn should_display_principals_as_text() {
    struct TestCase {
        desc: &'static str,
        rendered: String,
        expected: &'static str,
    }

    let principal = Principal::from_text(KNOWN_PRINCIPAL_TEXT).unwrap();
    let token_id = TokenId {
        ledger_id: principal,
    };
    let pair = TradingPair {
        base: principal,
        quote: principal,
    };

    let cases = vec![
        TestCase {
            desc: "TokenId",
            rendered: token_id.to_string(),
            expected: "ryjl3-tyaaa-aaaaa-aaaba-cai",
        },
        TestCase {
            desc: "TradingPair",
            rendered: pair.to_string(),
            expected: "ryjl3-tyaaa-aaaaa-aaaba-cai/ryjl3-tyaaa-aaaaa-aaaba-cai",
        },
        TestCase {
            desc: "LimitOrderRequest",
            rendered: LimitOrderRequest {
                pair,
                side: Side::Buy,
                price: Nat::from(100u64),
                quantity: Nat::from(1_000_000u64),
                time_in_force: Some(TimeInForce::FillOrKill),
            }
            .to_string(),
            expected: "LimitOrderRequest(pair=ryjl3-tyaaa-aaaaa-aaaba-cai/ryjl3-tyaaa-aaaaa-aaaba-cai, side=Buy, price=100, quantity=1_000_000, time_in_force=Some(FillOrKill))",
        },
        TestCase {
            desc: "OrderRecord",
            rendered: OrderRecord {
                owner: principal,
                side: Side::Sell,
                price: Nat::from(100u64),
                quantity: Nat::from(1_000_000u64),
                filled_quantity: Nat::from(0u64),
                status: OrderStatus::Open,
                created_at: 42,
                last_updated_at: None,
                time_in_force: TimeInForce::GoodTilCanceled,
                filled_quote: Nat::from(0u64),
                filled_fee: Nat::from(0u64),
                placed_by: Some(principal),
            }
            .to_string(),
            expected: "OrderRecord(owner=ryjl3-tyaaa-aaaaa-aaaba-cai, side=Sell, price=100, quantity=1_000_000, filled_quantity=0, status=Open, created_at=42, last_updated_at=None, time_in_force=GoodTilCanceled, filled_quote=0, filled_fee=0, placed_by=Some(ryjl3-tyaaa-aaaaa-aaaba-cai))",
        },
        TestCase {
            desc: "OrderRecord placed_by=None",
            rendered: OrderRecord {
                owner: principal,
                side: Side::Sell,
                price: Nat::from(100u64),
                quantity: Nat::from(1_000_000u64),
                filled_quantity: Nat::from(0u64),
                status: OrderStatus::Open,
                created_at: 42,
                last_updated_at: None,
                time_in_force: TimeInForce::GoodTilCanceled,
                filled_quote: Nat::from(0u64),
                filled_fee: Nat::from(0u64),
                placed_by: None,
            }
            .to_string(),
            expected: "OrderRecord(owner=ryjl3-tyaaa-aaaaa-aaaba-cai, side=Sell, price=100, quantity=1_000_000, filled_quantity=0, status=Open, created_at=42, last_updated_at=None, time_in_force=GoodTilCanceled, filled_quote=0, filled_fee=0, placed_by=None)",
        },
        TestCase {
            desc: "DepositRequest",
            rendered: DepositRequest {
                token_id: token_id.clone(),
                amount: Nat::from(500u64),
            }
            .to_string(),
            expected: "DepositRequest(token_id=ryjl3-tyaaa-aaaaa-aaaba-cai, amount=500)",
        },
        TestCase {
            desc: "WithdrawRequest",
            rendered: WithdrawRequest {
                token_id,
                amount: Nat::from(500u64),
            }
            .to_string(),
            expected: "WithdrawRequest(token_id=ryjl3-tyaaa-aaaaa-aaaba-cai, amount=500)",
        },
    ];

    for case in cases {
        assert_eq!(case.rendered, case.expected, "{}", case.desc);
    }
}

#[test]
fn should_serialize_limit_order_request() {
    let request = LimitOrderRequest {
        pair: test_trading_pair(),
        side: Side::Buy,
        price: Nat::from(100u64),
        quantity: Nat::from(1_000_000u64),
        time_in_force: Some(TimeInForce::FillOrKill),
    };
    let encoded = candid::encode_one(&request).unwrap();
    let decoded: LimitOrderRequest = candid::decode_one(&encoded).unwrap();
    assert_eq!(request, decoded);
}

#[test]
fn should_serialize_order_status() {
    for status in [
        OrderStatus::Pending,
        OrderStatus::Open,
        OrderStatus::Filled,
        OrderStatus::Canceled,
        OrderStatus::Expired,
    ] {
        let encoded = candid::encode_one(&status).unwrap();
        let decoded: OrderStatus = candid::decode_one(&encoded).unwrap();
        assert_eq!(status, decoded);
    }
}

#[test]
fn should_serialize_time_in_force() {
    for tif in [TimeInForce::GoodTilCanceled, TimeInForce::FillOrKill] {
        let encoded = candid::encode_one(tif).unwrap();
        let decoded: TimeInForce = candid::decode_one(&encoded).unwrap();
        assert_eq!(tif, decoded);
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
        status: TradingStatus::Trading,
        tick_size: Nat::from(10u64),
        lot_size: Nat::from(1_000_000u64),
        maker_fee_bps: 10,
        taker_fee_bps: 23,
        min_notional: Nat::from(5_000_000u64),
        max_notional: Some(Nat::from(9_000_000_000_000u64)),
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
fn should_serialize_get_my_orders_args() {
    for args in [
        GetMyOrdersArgs {
            filter: GetMyOrdersFilter::ById("order-1".to_string()),
        },
        GetMyOrdersArgs {
            filter: GetMyOrdersFilter::ByPage(GetMyOrdersPage {
                after: None,
                length: 50,
            }),
        },
        GetMyOrdersArgs {
            filter: GetMyOrdersFilter::ByPage(GetMyOrdersPage {
                after: Some("order-2".to_string()),
                length: 100,
            }),
        },
    ] {
        let encoded = candid::encode_one(&args).unwrap();
        let decoded: GetMyOrdersArgs = candid::decode_one(&encoded).unwrap();
        assert_eq!(args, decoded);
    }
}
