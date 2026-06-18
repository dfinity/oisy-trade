use crate::{
    AddLimitOrderError, AddLimitOrderRequestError, AddLimitOrderTemporaryError, Balance,
    CancelLimitOrderError, CancelLimitOrderRequestError, DepositError, DepositInternalError,
    DepositRequestError, DepositTemporaryError, ErrorKind, GetMyOrdersArgs, GetMyOrdersFilter,
    GetMyOrdersPage, GetOrderBookDepthError, GetOrderBookDepthRequest, GetOrderBookTickerError,
    LimitOrderRequest, OrderBookDepth, OrderBookTicker, OrderStatus, PriceLevel, Side, Token,
    TokenId, TokenMetadata, TradingPair, TradingPairInfo, TradingStatus, WithdrawError,
    WithdrawInternalError, WithdrawRequestError, WithdrawTemporaryError,
};
use candid::{CandidType, Nat, Principal};
use serde::{Deserialize, Serialize};

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

#[test]
fn should_serialize_add_limit_order_error() {
    for error in [
        AddLimitOrderError::request(AddLimitOrderRequestError::UnknownTradingPair),
        AddLimitOrderError::request(AddLimitOrderRequestError::InvalidPrice {
            price: Nat::from(7u64),
            tick_size: Nat::from(10u64),
        }),
        AddLimitOrderError::request(AddLimitOrderRequestError::InvalidQuantity {
            quantity: Nat::from(500_000u64),
            lot_size: Nat::from(1_000_000u64),
        }),
        AddLimitOrderError::temporary(AddLimitOrderTemporaryError::TradingHalted),
    ] {
        let encoded = candid::encode_one(&error).unwrap();
        let decoded: AddLimitOrderError = candid::decode_one(&encoded).unwrap();
        assert_eq!(error, decoded);
    }
}

#[test]
fn should_set_message_from_leaf_display() {
    let leaf = DepositRequestError::AmountExceedsMaximum;
    let error = DepositError::request(leaf.clone());
    assert_eq!(
        error.kind,
        ErrorKind::RequestError(Some(DepositRequestError::AmountExceedsMaximum))
    );
    assert_eq!(error.message, Some(leaf.to_string()));
    assert!(!error.message.unwrap().is_empty());
}

#[test]
fn should_place_deposit_leaves_under_their_disposition_arm() {
    let request = DepositError::request(DepositRequestError::InsufficientFunds {
        balance: Nat::from(1u64),
    });
    assert!(matches!(request.kind, ErrorKind::RequestError(Some(_))));
    assert!(!request.message.unwrap().is_empty());

    let temporary = DepositError::temporary(DepositTemporaryError::LedgerTemporarilyUnavailable);
    assert!(matches!(temporary.kind, ErrorKind::TemporaryError(Some(_))));
    assert!(!temporary.message.unwrap().is_empty());

    let internal = DepositError::internal(DepositInternalError::LedgerError {
        reason: "boom".to_string(),
    });
    assert!(matches!(internal.kind, ErrorKind::InternalError(Some(_))));
    assert!(!internal.message.unwrap().is_empty());
}

#[test]
fn should_place_withdraw_leaves_under_their_disposition_arm() {
    let request = WithdrawError::request(WithdrawRequestError::AmountTooSmall {
        min_amount: Nat::from(2u64),
    });
    assert!(matches!(request.kind, ErrorKind::RequestError(Some(_))));
    assert!(!request.message.unwrap().is_empty());

    let temporary = WithdrawError::temporary(WithdrawTemporaryError::OperationInProgress);
    assert!(matches!(temporary.kind, ErrorKind::TemporaryError(Some(_))));
    assert!(!temporary.message.unwrap().is_empty());

    let internal = WithdrawError::internal(WithdrawInternalError::LedgerInsufficientFunds {
        balance: Nat::from(0u64),
    });
    assert!(matches!(internal.kind, ErrorKind::InternalError(Some(_))));
    assert!(!internal.message.unwrap().is_empty());
}

#[test]
fn should_place_cancel_leaves_under_request_arm() {
    let error = CancelLimitOrderError::request(CancelLimitOrderRequestError::OrderNotFound);
    assert!(matches!(error.kind, ErrorKind::RequestError(Some(_))));
    assert!(!error.message.unwrap().is_empty());
}

// R5: a client built against today's interface decodes an error whose inner
// arm gained a future leaf as `None`, while still reading `kind` and `message`.
#[test]
fn should_decode_future_leaf_as_none_keeping_arm_and_message() {
    // A superset of `DepositRequestError` with an extra leaf the shipped type
    // does not know about.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
    enum FutureDepositRequestError {
        AmountExceedsMaximum,
        UnsupportedToken { token_id: TokenId },
        InsufficientFunds { balance: Nat },
        InsufficientAllowance { allowance: Nat },
        SomeFutureReason { detail: String },
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
    #[allow(clippy::enum_variant_names)]
    enum FutureErrorKind {
        RequestError(Option<FutureDepositRequestError>),
        TemporaryError(Option<crate::Never>),
        InternalError(Option<crate::Never>),
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
    struct FutureDepositError {
        kind: FutureErrorKind,
        message: Option<String>,
    }

    let future = FutureDepositError {
        kind: FutureErrorKind::RequestError(Some(FutureDepositRequestError::SomeFutureReason {
            detail: "from the future".to_string(),
        })),
        message: Some("a future reason".to_string()),
    };

    let encoded = candid::encode_one(&future).unwrap();
    let decoded: DepositError = candid::decode_one(&encoded).unwrap();

    assert_eq!(decoded.kind, ErrorKind::RequestError(None));
    assert_eq!(decoded.message, Some("a future reason".to_string()));
}
