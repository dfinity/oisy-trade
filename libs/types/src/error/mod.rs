//! Disposition-tagged, forward-compatible user-facing errors.

#[cfg(test)]
mod tests;

use crate::{Nat, Principal, TokenId};
use candid::CandidType;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A disposition-tagged, forward-compatible user-facing error.
///
/// Every fallible endpoint returns one of these: a [`kind`](Self::kind)
/// carrying the disposition (what the caller should do) plus an advisory,
/// human-readable [`message`](Self::message). The disposition is the contract;
/// clients branch on `kind` and the inner leaf, and **must not** parse
/// `message`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct Error<Request, Temporary, Internal> {
    /// The disposition and, when available, the specific reason.
    pub kind: ErrorKind<Request, Temporary, Internal>,
    /// Advisory, human-readable text derived from the underlying leaf's
    /// `Display`. Clients must not parse it; programmatic handling is on
    /// [`kind`](Self::kind) and the inner leaf only.
    pub message: Option<String>,
}

/// The disposition of an [`Error`], parameterized by its per-endpoint leaves.
///
/// Each arm carries an `Option` of its leaf so that a client built against an
/// older interface decodes an unknown future leaf as `None` while still
/// reading the arm itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum ErrorKind<Request, Temporary, Internal> {
    /// Caller-side: the request will not succeed as-is. Correct the input,
    /// satisfy a precondition, or stop. Do not auto-retry unchanged.
    RequestError(Option<Request>),
    /// Transient: retry the same call after a backoff.
    TemporaryError(Option<Temporary>),
    /// Canister-side fault: surface to operators. Do not retry.
    InternalError(Option<Internal>),
}

impl<Request, Temporary, Internal> Error<Request, Temporary, Internal>
where
    Request: std::error::Error,
    Temporary: std::error::Error,
    Internal: std::error::Error,
{
    /// Build a `RequestError`, deriving `message` from the leaf's `Display`.
    pub fn request(leaf: Request) -> Self {
        Self {
            message: Some(leaf.to_string()),
            kind: ErrorKind::RequestError(Some(leaf)),
        }
    }

    /// Build a `TemporaryError`, deriving `message` from the leaf's `Display`.
    pub fn temporary(leaf: Temporary) -> Self {
        Self {
            message: Some(leaf.to_string()),
            kind: ErrorKind::TemporaryError(Some(leaf)),
        }
    }

    /// Build an `InternalError`, deriving `message` from the leaf's `Display`.
    pub fn internal(leaf: Internal) -> Self {
        Self {
            message: Some(leaf.to_string()),
            kind: ErrorKind::InternalError(Some(leaf)),
        }
    }
}

/// Uninhabited leaf type for disposition arms an endpoint can never produce
/// (rendered by Candid as an empty `opt variant {}`).
pub use never::Never;

mod never {
    #![allow(unreachable_code)]

    use super::{CandidType, Deserialize, Serialize, fmt};

    /// An uninhabited leaf type for a disposition arm an endpoint can never
    /// produce. It still occupies the arm so leaves can be added later without
    /// breaking clients; Candid renders it as an empty `opt variant {}`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, CandidType)]
    pub enum Never {}

    impl fmt::Display for Never {
        fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match *self {}
        }
    }

    impl std::error::Error for Never {}
}

/// Error returned when placing a limit order fails.
pub type AddLimitOrderError = Error<AddLimitOrderRequestError, AddLimitOrderTemporaryError, Never>;

/// Caller-side reasons `add_limit_order` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum AddLimitOrderRequestError {
    /// The amount exceeds the maximum supported value.
    #[error("the amount exceeds the maximum supported value")]
    AmountExceedsMaximum,
    /// The requested trading pair is not registered.
    #[error("the requested trading pair is not registered")]
    UnknownTradingPair,
    /// The price is not a positive multiple of the tick size.
    #[error("price {price} is not a positive multiple of tick size {tick_size}")]
    InvalidPrice {
        /// The rejected price.
        price: Nat,
        /// The required tick size.
        tick_size: Nat,
    },
    /// The quantity is not a positive multiple of the lot size.
    #[error("quantity {quantity} is not a positive multiple of lot size {lot_size}")]
    InvalidQuantity {
        /// The rejected quantity.
        quantity: Nat,
        /// The required lot size.
        lot_size: Nat,
    },
    /// The user does not have enough balance to place the order.
    #[error(
        "insufficient balance for token {}: available {available}, required {required}",
        token.ledger_id
    )]
    InsufficientBalance {
        /// The token for which the balance is insufficient.
        token: TokenId,
        /// The user's available balance.
        available: Nat,
        /// The balance required to place the order.
        required: Nat,
    },
    /// The order's notional (`price × quantity / 10^base_decimals`, in quote
    /// smallest units) is below `min` or above `max`.
    #[error("{}", invalid_notional_message(notional, min, max.as_ref()))]
    InvalidNotional {
        /// The order's notional in quote token smallest units.
        notional: Nat,
        /// The configured minimum notional.
        min: Nat,
        /// The configured maximum notional, if any.
        max: Option<Nat>,
    },
}

fn invalid_notional_message(notional: &Nat, min: &Nat, max: Option<&Nat>) -> String {
    match max {
        Some(max) => format!("notional {notional} is outside the allowed range [{min}, {max}]"),
        None => format!("notional {notional} is below the minimum {min}"),
    }
}

/// Transient reasons `add_limit_order` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum AddLimitOrderTemporaryError {
    /// Trading is halted (globally or on this pair); no new orders are accepted.
    #[error("trading is halted; no new orders are accepted")]
    TradingHalted,
}

/// Error returned when canceling a limit order fails.
pub type CancelLimitOrderError = Error<CancelLimitOrderRequestError, Never, Never>;

/// Caller-side reasons `cancel_limit_order` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum CancelLimitOrderRequestError {
    /// No order with the given ID exists.
    #[error("no order with the given id exists")]
    OrderNotFound,
    /// The caller does not own the order.
    #[error("the caller does not own the order")]
    NotOrderOwner,
    /// The order has already been fully filled and cannot be canceled.
    #[error("the order has already been fully filled")]
    OrderAlreadyFilled,
    /// The order has already been canceled.
    #[error("the order has already been canceled")]
    OrderAlreadyCanceled,
}

/// Error returned by the deposit endpoint.
pub type DepositError = Error<DepositRequestError, DepositTemporaryError, DepositInternalError>;

/// Caller-side reasons a deposit can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum DepositRequestError {
    /// The amount exceeds the maximum supported value.
    #[error("the amount exceeds the maximum supported value")]
    AmountExceedsMaximum,
    /// The token is not part of any trading pair on the OISY TRADE.
    #[error("token {} is not supported", token_id.ledger_id)]
    UnsupportedToken {
        /// The unsupported token.
        token_id: TokenId,
    },
    /// The caller's external wallet does not hold enough funds for the transfer.
    #[error("insufficient funds: balance {balance}")]
    InsufficientFunds {
        /// The current balance of the source account.
        balance: Nat,
    },
    /// The caller's ICRC-2 allowance to the OISY TRADE is not large enough.
    #[error("insufficient allowance: {allowance}")]
    InsufficientAllowance {
        /// The current allowance.
        allowance: Nat,
    },
}

/// Transient reasons a deposit can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum DepositTemporaryError {
    /// Another deposit or withdrawal is already in flight for this
    /// `(caller, token)`. Retry once the previous operation completes.
    #[error("another deposit or withdrawal is already in flight")]
    OperationInProgress,
    /// The token ledger is temporarily unavailable.
    #[error("the token ledger is temporarily unavailable")]
    LedgerTemporarilyUnavailable,
    /// The inter-canister call to the token ledger failed.
    #[error("call to {ledger}.{method} failed: {reason}")]
    CallFailed {
        /// The ledger canister that was called.
        ledger: Principal,
        /// The name of the method that was called.
        method: String,
        /// The reason the call failed.
        reason: String,
    },
}

/// DEX-side reasons a deposit can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum DepositInternalError {
    /// The `icrc2_transfer_from` call returned an unexpected ledger error.
    #[error("ledger error: {reason}")]
    LedgerError {
        /// A human-readable description of the ledger error.
        reason: String,
    },
    /// The ledger's response could not be Candid-decoded.
    #[error("failed to decode the response from {ledger}.{method}: {reason}")]
    CandidDecodeFailed {
        /// The ledger canister that was called.
        ledger: Principal,
        /// The name of the method that was called.
        method: String,
        /// The reason the response could not be decoded.
        reason: String,
    },
}

/// Error returned by the withdraw endpoint.
pub type WithdrawError = Error<WithdrawRequestError, WithdrawTemporaryError, WithdrawInternalError>;

/// Caller-side reasons a withdrawal can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum WithdrawRequestError {
    /// The amount exceeds the maximum supported value.
    #[error("the amount exceeds the maximum supported value")]
    AmountExceedsMaximum,
    /// The requested amount is too small to cover the ledger transfer fee.
    #[error("the amount is too small; the minimum withdrawal is {min_amount}")]
    AmountTooSmall {
        /// The minimum withdrawal amount (ledger fee + 1).
        min_amount: Nat,
    },
    /// The token is not part of any trading pair on the OISY TRADE.
    #[error("token {} is not supported", token_id.ledger_id)]
    UnsupportedToken {
        /// The unsupported token.
        token_id: TokenId,
    },
    /// The caller does not have enough free balance.
    #[error("insufficient free balance: available {available}")]
    InsufficientBalance {
        /// The caller's available free balance.
        available: Nat,
    },
}

/// Transient reasons a withdrawal can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum WithdrawTemporaryError {
    /// Another deposit or withdrawal is already in flight for this
    /// `(caller, token)`. Retry once the previous operation completes.
    #[error("another deposit or withdrawal is already in flight")]
    OperationInProgress,
    /// The token ledger is temporarily unavailable.
    #[error("the token ledger is temporarily unavailable")]
    LedgerTemporarilyUnavailable,
    /// The inter-canister call to the token ledger failed.
    #[error("call to {ledger}.{method} failed: {reason}")]
    CallFailed {
        /// The ledger canister that was called.
        ledger: Principal,
        /// The name of the method that was called.
        method: String,
        /// The reason the call failed.
        reason: String,
    },
}

/// DEX-side reasons a withdrawal can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum WithdrawInternalError {
    /// The `icrc1_transfer` call returned an unexpected ledger error.
    #[error("ledger error: {reason}")]
    LedgerError {
        /// A human-readable description of the ledger error.
        reason: String,
    },
    /// The ledger reported insufficient funds even though the OISY TRADE's
    /// accounting credited the balance — a genuine invariant violation.
    #[error(
        "ledger reported insufficient funds (balance {balance}) against the OISY TRADE's own accounting"
    )]
    LedgerInsufficientFunds {
        /// The balance the ledger reported for the OISY TRADE.
        balance: Nat,
    },
    /// The ledger's response could not be Candid-decoded.
    #[error("failed to decode the response from {ledger}.{method}: {reason}")]
    CandidDecodeFailed {
        /// The ledger canister that was called.
        ledger: Principal,
        /// The name of the method that was called.
        method: String,
        /// The reason the response could not be decoded.
        reason: String,
    },
}
