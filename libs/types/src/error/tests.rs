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
    /// Carries the same payload as [`CurrentRequestError::KnownReason`], so a
    /// leaf the current interface already knows decodes fully typed rather than
    /// softening — which is what lets a test tell "softened because it is
    /// unknown" from "softened because everything softens".
    KnownReason {
        detail: String,
    },
    SomeFutureReason {
        detail: String,
    },
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
        expected: &'static str,
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
            expected: "temporary error: call to ryjl3-tyaaa-aaaaa-aaaba-cai.icrc2_transfer_from failed: timed out",
        },
        TestCase {
            desc: "WithdrawError UnsupportedToken carries a TokenId",
            rendered: WithdrawError::request(WithdrawRequestError::UnsupportedToken {
                token_id: TokenId {
                    ledger_id: principal,
                },
            })
            .to_string(),
            expected: "request error: token ryjl3-tyaaa-aaaaa-aaaba-cai is not supported",
        },
    ];

    for case in cases {
        assert_eq!(case.rendered, case.expected, "{}", case.desc);
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

/// The same softening inside a batch reply, which nests the envelope one level
/// deeper: an unknown future leaf softens **in its own item**, a sibling item's
/// known leaf still decodes fully typed, and `message` survives at the item
/// level and at the batch level alike.
///
/// This is what lets a batch endpoint add a per-item reason without breaking
/// existing clients — and therefore why the per-item `variant { Ok; Err }` never
/// needs, and never gets, a third arm.
#[test]
fn should_decode_a_future_leaf_as_none_per_item_and_at_the_batch_level() {
    type FutureBatchReply = Result<Vec<Result<u64, FutureError>>, FutureError>;
    type CurrentBatchReply = Result<Vec<Result<u64, CurrentError>>, CurrentError>;

    let future_leaf = |detail: &str, message: &str| FutureError {
        kind: FutureErrorKind::RequestError(Some(FutureRequestError::SomeFutureReason {
            detail: detail.to_string(),
        })),
        message: Some(message.to_string()),
    };
    let partial_success: FutureBatchReply = Ok(vec![
        Ok(7),
        Err(future_leaf("from the future", "a future reason")),
        Err(FutureError {
            kind: FutureErrorKind::RequestError(Some(FutureRequestError::KnownReason {
                detail: "bad request".to_string(),
            })),
            message: Some("request failed: bad request".to_string()),
        }),
    ]);

    let encoded = candid::encode_one(&partial_success).unwrap();
    let decoded: CurrentBatchReply = candid::decode_one(&encoded).unwrap();

    let items = decoded.expect("the batch itself succeeded");
    assert_eq!(
        items,
        vec![
            Ok(7),
            Err(CurrentError {
                kind: ErrorKind::RequestError(None),
                message: Some("a future reason".to_string()),
            }),
            Err(CurrentError::request(CurrentRequestError::KnownReason {
                detail: "bad request".to_string(),
            })),
        ]
    );

    let rejected: FutureBatchReply = Err(future_leaf("too large", "the batch is too large"));

    let encoded = candid::encode_one(&rejected).unwrap();
    let decoded: CurrentBatchReply = candid::decode_one(&encoded).unwrap();

    assert_eq!(
        decoded,
        Err(CurrentError {
            kind: ErrorKind::RequestError(None),
            message: Some("the batch is too large".to_string()),
        })
    );
}
