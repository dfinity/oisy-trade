use crate::{
    DepositError, DepositRequestError, DepositTemporaryError, ErrorKind, TokenId, WithdrawError,
    WithdrawRequestError,
};
use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

const KNOWN_PRINCIPAL_TEXT: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";

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
fn should_display_principals_as_text() {
    struct TestCase {
        desc: &'static str,
        rendered: String,
    }

    let principal = Principal::from_text(KNOWN_PRINCIPAL_TEXT).unwrap();

    let cases = vec![
        TestCase {
            desc: "DepositError CallFailed carries a ledger principal",
            rendered: DepositError::temporary(DepositTemporaryError::CallFailed {
                ledger: principal,
                method: "icrc2_transfer_from".to_string(),
                reason: "timed out".to_string(),
            })
            .to_string(),
        },
        TestCase {
            desc: "WithdrawError UnsupportedToken carries a TokenId",
            rendered: WithdrawError::request(WithdrawRequestError::UnsupportedToken {
                token_id: TokenId {
                    ledger_id: principal,
                },
            })
            .to_string(),
        },
    ];

    for case in cases {
        assert!(
            case.rendered.contains(KNOWN_PRINCIPAL_TEXT),
            "{}: expected textual principal in {:?}",
            case.desc,
            case.rendered
        );
        assert!(
            !case.rendered.contains("[10,"),
            "{}: expected no byte-array fragment in {:?}",
            case.desc,
            case.rendered
        );
    }
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
