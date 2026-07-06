//! Disposition-tagged, forward-compatible user-facing errors.

#[cfg(test)]
mod tests;

use crate::{FilterToken, Nat, Principal, TokenId};
use candid::CandidType;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A disposition-tagged, forward-compatible user-facing error.
///
/// The user-facing error types use this shape: a [`kind`](Self::kind)
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

impl<Request, Temporary, Internal> fmt::Display for Error<Request, Temporary, Internal> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { kind, message } = self;
        let disposition = match kind {
            ErrorKind::RequestError(_) => "request",
            ErrorKind::TemporaryError(_) => "temporary",
            ErrorKind::InternalError(_) => "internal",
        };
        write!(
            f,
            "{disposition} error: {}",
            message.as_deref().unwrap_or("<no detail>")
        )
    }
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
    // The `CandidType` derive on the uninhabited `Never` expands to a match
    // over its (empty) variant set, which the compiler flags as unreachable.
    // The allow cannot be narrowed to the item: it must cover the derive's
    // generated impl, and this module contains only `Never`.
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
    /// The supplied order id was not a well-formed order id.
    #[error("the supplied order id is not a well-formed order id")]
    InvalidOrderId,
    /// No order with the given ID exists.
    #[error("no order with the given id exists")]
    OrderNotFound,
    /// The caller does not own the order.
    #[error("the caller does not own the order")]
    NotOrderOwner,
    /// The order has reached a terminal state (Filled, Canceled, or Expired)
    /// and can no longer be canceled.
    #[error("the order has reached a terminal state and can no longer be canceled")]
    OrderAlreadyTerminal,
}

/// Error returned by the `get_my_orders` query.
pub type GetMyOrdersError = Error<GetMyOrdersRequestError, Never, Never>;

/// Caller-side reasons `get_my_orders` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum GetMyOrdersRequestError {
    /// An order id in the filter (`ById` target or `ByPage.after` cursor) was
    /// not a well-formed order id.
    #[error("the supplied order id is not a well-formed order id")]
    InvalidOrderId,
    /// A well-formed order id in the filter (`ById` target or `ByPage.after`
    /// cursor) is unknown or not owned by the caller.
    #[error("no order with the given id exists for the caller")]
    OrderNotFound,
}

/// Error returned by the deposit endpoint.
pub type DepositError = Error<DepositRequestError, DepositTemporaryError, DepositInternalError>;

/// Caller-side reasons a deposit can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum DepositRequestError {
    /// The amount exceeds the maximum supported value.
    #[error("the amount exceeds the maximum supported value")]
    AmountExceedsMaximum,
    /// The token is not part of any trading pair on this canister.
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
    /// The caller's ICRC-2 allowance to this canister is not large enough.
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

/// Canister-side reasons a deposit can fail.
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
    /// The token is not part of any trading pair on this canister.
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
    /// The ledger fee changed between fetch and transfer; nothing happened on
    /// the ledger. This is rare; retry.
    #[error("the ledger fee changed before the transfer was applied")]
    LedgerFeeChanged,
}

/// Canister-side reasons a withdrawal can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum WithdrawInternalError {
    /// The `icrc1_transfer` call returned an unexpected ledger error.
    #[error("ledger error: {reason}")]
    LedgerError {
        /// A human-readable description of the ledger error.
        reason: String,
    },
    /// The ledger reported insufficient funds even though this canister's
    /// accounting credited the balance — a genuine invariant violation.
    #[error(
        "ledger reported insufficient funds (balance {balance}) against OISY TRADE's own accounting"
    )]
    LedgerInsufficientFunds {
        /// The balance the ledger reported for this canister.
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

/// Error returned by the `get_order_book_ticker` query.
pub type GetOrderBookTickerError = Error<GetOrderBookTickerRequestError, Never, Never>;

/// Caller-side reasons `get_order_book_ticker` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum GetOrderBookTickerRequestError {
    /// The requested trading pair is not registered on the OISY TRADE.
    #[error("the requested trading pair is not registered")]
    UnknownTradingPair,
}

/// Error returned by the `get_order_book_depth` query.
pub type GetOrderBookDepthError = Error<GetOrderBookDepthRequestError, Never, Never>;

/// Caller-side reasons `get_order_book_depth` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum GetOrderBookDepthRequestError {
    /// The requested trading pair is not registered on the OISY TRADE.
    #[error("the requested trading pair is not registered")]
    UnknownTradingPair,
    /// The requested depth limit exceeds the maximum supported value.
    #[error("requested depth limit {requested} exceeds the maximum {max}")]
    LimitTooLarge {
        /// The rejected limit.
        requested: u32,
        /// The maximum limit the OISY TRADE will serve.
        max: u32,
    },
}

/// Error returned by the `add_trading_account` endpoint.
pub type AddTradingAccountError =
    Error<AddTradingAccountRequestError, AddTradingAccountTemporaryError, Never>;

/// Caller-side reasons `add_trading_account` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum AddTradingAccountRequestError {
    /// The granter has never deposited, so it is not a registered funding
    /// account. Granting requires an existing, economically established
    /// account and never creates one.
    #[error("the granter is not a registered user")]
    GranterNotRegistered,
    /// The granter tried to whitelist itself as its own trading account.
    #[error("a funding account cannot whitelist itself")]
    SelfGrant,
    /// The principal is already a trading account, of the granter or of
    /// someone else — a trading account maps to exactly one funding account.
    #[error("the principal is already a trading account")]
    AlreadyTradingAccount,
    /// The principal has already deposited, so it is a registered user; a
    /// trading key must be a fresh principal.
    #[error("the principal is already a registered user")]
    AlreadyRegisteredUser,
    /// The granter is itself a trading account; delegation chains are not
    /// allowed.
    #[error("the granter is itself a trading account")]
    GranterIsTradingAccount,
    /// The granter already has the maximum number of trading accounts.
    #[error("the granter already has the maximum of {max} trading accounts")]
    TooManyTradingAccounts {
        /// The per-funding-account cap on trading accounts.
        max: u32,
    },
}

/// Transient reasons `add_trading_account` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum AddTradingAccountTemporaryError {
    /// The principal has an in-flight deposit or withdrawal. Retry once it
    /// completes; whitelisting it now could strand a balance it is about to
    /// acquire.
    #[error("the principal has an in-flight deposit or withdrawal")]
    FundingOperationInProgress,
}

/// Error returned by the `get_my_trading_accounts` query.
///
/// The query cannot currently fail, so all three disposition arms are reserved
/// (always-`null`) — the forward-compatible slot for future leaves.
pub type GetMyTradingAccountsError = Error<Never, Never, Never>;

/// Error returned by the `get_my_trades` query.
pub type GetMyTradesError = Error<GetMyTradesRequestError, Never, Never>;

/// Caller-side reasons `get_my_trades` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum GetMyTradesRequestError {
    /// The `order_id` in a `ByOrder` filter was not a well-formed order id.
    #[error("the supplied order id is not a well-formed order id")]
    InvalidOrderId,
    /// The `after` cursor was not a well-formed `TradeId`.
    #[error("the supplied cursor is not a well-formed trade id")]
    InvalidTradeId,
    /// The `order_id` in a `ByOrder` filter is unknown or not owned by the
    /// caller.
    #[error("no order with the given id exists for the caller")]
    OrderNotFound,
}

/// Error returned by the `get_balances` / `get_fee_balances` queries.
///
/// These are read endpoints, so a failure dooms the whole call: the response is
/// either every requested balance or this single error (see the spec's
/// batch-semantic design decision). A blind retry of a read is cheap and
/// side-effect-free, so there is no per-entry partial-success result.
pub type GetBalancesError = Error<GetBalancesRequestError, Never, Never>;

/// Caller-side reasons `get_balances` / `get_fee_balances` can fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType, thiserror::Error)]
pub enum GetBalancesRequestError {
    /// The filter referenced a token that the OISY TRADE does not support.
    #[error("the filter referenced an unsupported token {0:?}")]
    TokenNotSupported(FilterToken),
    /// The filter exceeded the maximum number of entries.
    #[error("the filter has {len} entries, exceeding the maximum {max}")]
    FilterTooLarge {
        /// The submitted filter length.
        len: u32,
        /// The maximum allowed filter length.
        max: u32,
    },
}
