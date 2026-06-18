use crate::{
    AddLimitOrderError, AddLimitOrderRequestError, AddLimitOrderTemporaryError,
    CancelLimitOrderError, CancelLimitOrderRequestError, DepositError, DepositInternalError,
    DepositRequestError, DepositTemporaryError, ErrorKind, Principal, WithdrawError,
    WithdrawInternalError, WithdrawRequestError, WithdrawTemporaryError,
};
use candid::{CandidType, Nat};
use serde::{Deserialize, Serialize};

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
fn should_serialize_cancel_limit_order_error() {
    for error in [
        CancelLimitOrderError::request(CancelLimitOrderRequestError::OrderNotFound),
        CancelLimitOrderError::request(CancelLimitOrderRequestError::NotOrderOwner),
        CancelLimitOrderError::request(CancelLimitOrderRequestError::OrderAlreadyFilled),
        CancelLimitOrderError::request(CancelLimitOrderRequestError::OrderAlreadyCanceled),
    ] {
        let encoded = candid::encode_one(&error).unwrap();
        let decoded: CancelLimitOrderError = candid::decode_one(&encoded).unwrap();
        assert_eq!(error, decoded);
    }
}

#[test]
fn should_serialize_deposit_error() {
    for error in [
        DepositError::request(DepositRequestError::AmountExceedsMaximum),
        DepositError::request(DepositRequestError::InsufficientFunds {
            balance: Nat::from(42u64),
        }),
        DepositError::temporary(DepositTemporaryError::OperationInProgress),
        DepositError::temporary(DepositTemporaryError::CallFailed {
            ledger: Principal::anonymous(),
            method: "icrc2_transfer_from".to_string(),
            reason: "timeout".to_string(),
        }),
        DepositError::internal(DepositInternalError::LedgerError {
            reason: "unexpected".to_string(),
        }),
    ] {
        let encoded = candid::encode_one(&error).unwrap();
        let decoded: DepositError = candid::decode_one(&encoded).unwrap();
        assert_eq!(error, decoded);
    }
}

#[test]
fn should_serialize_withdraw_error() {
    for error in [
        WithdrawError::request(WithdrawRequestError::AmountExceedsMaximum),
        WithdrawError::request(WithdrawRequestError::AmountTooSmall {
            min_amount: Nat::from(11u64),
        }),
        WithdrawError::request(WithdrawRequestError::InsufficientBalance {
            available: Nat::from(7u64),
        }),
        WithdrawError::temporary(WithdrawTemporaryError::LedgerTemporarilyUnavailable),
        WithdrawError::internal(WithdrawInternalError::LedgerInsufficientFunds {
            balance: Nat::from(3u64),
        }),
    ] {
        let encoded = candid::encode_one(&error).unwrap();
        let decoded: WithdrawError = candid::decode_one(&encoded).unwrap();
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
