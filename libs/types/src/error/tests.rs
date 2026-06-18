use crate::{DepositError, DepositRequestError, ErrorKind};
use candid::CandidType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
enum CurrentRequestError {
    #[error("known reason")]
    KnownReason,
}

type CurrentError = crate::Error<CurrentRequestError, crate::Never, crate::Never>;

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
    for error in [
        CurrentError::request(CurrentRequestError::KnownReason),
        CurrentError {
            kind: ErrorKind::TemporaryError(None),
            message: None,
        },
        CurrentError {
            kind: ErrorKind::InternalError(None),
            message: None,
        },
    ] {
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
