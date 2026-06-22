use crate::{
    DepositError, DepositRequestError, ErrorKind, FilterToken, GetBalancesError,
    GetBalancesFilterError, GetBalancesRequestError, GetBalancesTokenError, GetOrderBookDepthError,
    GetOrderBookDepthRequestError, GetOrderBookTickerError, GetOrderBookTickerRequestError,
    TokenId,
};
use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
enum CurrentRequestError {
    #[error("request failed: {detail}")]
    KnownReason {
        /// A distinctive detail so the `Display` is non-trivial.
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
enum CurrentTemporaryError {
    #[error("temporary failure: {detail}")]
    KnownReason {
        /// A distinctive detail so the `Display` is non-trivial.
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
enum CurrentInternalError {
    #[error("internal failure: {detail}")]
    KnownReason {
        /// A distinctive detail so the `Display` is non-trivial.
        detail: String,
    },
}

type CurrentError = crate::Error<CurrentRequestError, CurrentTemporaryError, CurrentInternalError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
enum FutureRequestError {
    KnownReason,
    SomeFutureReason { detail: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
#[allow(clippy::enum_variant_names)]
enum FutureErrorKind {
    RequestError(Option<FutureRequestError>),
    TemporaryError(Option<crate::Never>),
    InternalError(Option<crate::Never>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
struct FutureError {
    kind: FutureErrorKind,
    message: Option<String>,
}

#[test]
fn should_round_trip_each_disposition_arm() {
    let cases = [
        CurrentError::request(CurrentRequestError::KnownReason {
            detail: "bad request".to_string(),
        }),
        CurrentError::temporary(CurrentTemporaryError::KnownReason {
            detail: "try again".to_string(),
        }),
        CurrentError::internal(CurrentInternalError::KnownReason {
            detail: "ledger broke".to_string(),
        }),
    ];

    let expected_messages = [
        Some("request failed: bad request".to_string()),
        Some("temporary failure: try again".to_string()),
        Some("internal failure: ledger broke".to_string()),
    ];

    for (error, expected_message) in cases.into_iter().zip(expected_messages) {
        assert_eq!(error.message, expected_message);

        let encoded = candid::encode_one(&error).unwrap();
        let decoded: CurrentError = candid::decode_one(&encoded).unwrap();
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
fn should_decode_future_leaf_as_none_keeping_arm_and_message() {
    let future = FutureError {
        kind: FutureErrorKind::RequestError(Some(FutureRequestError::SomeFutureReason {
            detail: "from the future".to_string(),
        })),
        message: Some("a future reason".to_string()),
    };

    let encoded = candid::encode_one(&future).unwrap();
    let decoded: CurrentError = candid::decode_one(&encoded).unwrap();

    assert_eq!(decoded.kind, ErrorKind::RequestError(None));
    assert_eq!(decoded.message, Some("a future reason".to_string()));
}

#[test]
fn should_place_get_order_book_ticker_leaf_under_request_error() {
    let leaf = GetOrderBookTickerRequestError::UnknownTradingPair;
    let error = GetOrderBookTickerError::request(leaf.clone());
    assert_eq!(error.kind, ErrorKind::RequestError(Some(leaf.clone())));
    assert_eq!(error.message, Some(leaf.to_string()));
    assert!(!error.message.unwrap().is_empty());
}

#[test]
fn should_place_get_order_book_depth_leaves_under_request_error() {
    let leaves = [
        GetOrderBookDepthRequestError::UnknownTradingPair,
        GetOrderBookDepthRequestError::LimitTooLarge {
            requested: 1_001,
            max: 1_000,
        },
    ];
    for leaf in leaves {
        let error = GetOrderBookDepthError::request(leaf.clone());
        assert_eq!(error.kind, ErrorKind::RequestError(Some(leaf.clone())));
        assert_eq!(error.message, Some(leaf.to_string()));
        assert!(!error.message.unwrap().is_empty());
    }
}

#[test]
fn should_place_get_balances_token_leaf_under_request_error() {
    let leaf = GetBalancesTokenError::TokenNotSupported(FilterToken::ById(TokenId {
        ledger_id: Principal::from_slice(&[0xFF]),
    }));
    let error = GetBalancesError::request(leaf.clone());
    assert_eq!(error.kind, ErrorKind::RequestError(Some(leaf.clone())));
    assert_eq!(error.message, Some(leaf.to_string()));
    assert!(!error.message.unwrap().is_empty());
}

#[test]
fn should_place_get_balances_filter_leaf_under_request_error() {
    let leaf = GetBalancesFilterError::FilterTooLarge { len: 101, max: 100 };
    let error = GetBalancesRequestError::request(leaf.clone());
    assert_eq!(error.kind, ErrorKind::RequestError(Some(leaf.clone())));
    assert_eq!(error.message, Some(leaf.to_string()));
    assert!(!error.message.unwrap().is_empty());
}
