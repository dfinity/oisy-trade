//! Candid types used by the candid interface of the DEX canister.

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(test)]
mod tests;

use candid::{CandidType, Nat, Principal};
use serde::{Deserialize, Serialize};

/// Unique identifier for an order.
pub type OrderId = u64;

/// Request to place a new limit order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct LimitOrderRequest {
    // TODO DEFI-2723: add fields: price, quantity, side, etc.
}

/// Response after successfully placing a limit order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct LimitOrderResponse {
    /// The unique identifier assigned to the new order.
    pub order_id: OrderId,
}

/// Status of an order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum OrderStatus {
    /// The order was not found.
    NotFound,
    /// The order is pending processing.
    Pending,
}

/// A token identified by its ledger canister ID.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, CandidType,
)]
pub struct Token {
    /// The canister ID of the token's ledger.
    pub ledger_canister_id: Principal,
}

/// Request to deposit tokens into the DEX.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct DepositRequest {
    /// The token to deposit.
    pub token: Token,
    /// The amount to deposit.
    pub amount: Nat,
}

/// Error returned by the deposit endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum DepositError {
    /// The inter-canister call to the token ledger failed.
    CallFailed {
        /// The ledger canister that was called.
        pub ledger: Principal,
        /// The name of the method that was called.
        pub method: String,
        /// The reason the call failed.
        pub reason: String,
    },
    /// The icrc2_transfer_from call to the token ledger returned an error.
    LedgerError(LedgerTransferFromError),
}

/// Errors that can be returned by icrc2_transfer_from.
///
/// Mirrors [`icrc_ledger_types::icrc2::transfer_from::TransferFromError`]
/// so that `dex_types` does not depend on `icrc-ledger-types`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum LedgerTransferFromError {
    /// The fee provided was incorrect.
    BadFee {
        /// The fee that the ledger expected.
        pub expected_fee: Nat,
    },
    /// The burn amount was below the minimum.
    BadBurn {
        /// The minimum burn amount.
        pub min_burn_amount: Nat,
    },
    /// The source account does not hold enough funds.
    InsufficientFunds {
        /// The current balance of the source account.
        pub balance: Nat,
    },
    /// The caller's allowance is not large enough.
    InsufficientAllowance {
        /// The current allowance.
        pub allowance: Nat,
    },
    /// The transaction is too old.
    TooOld,
    /// The transaction was created in the future.
    CreatedInFuture {
        /// The current ledger time.
        pub ledger_time: u64,
    },
    /// The transaction is a duplicate.
    Duplicate {
        /// The block index of the duplicate transaction.
        pub duplicate_of: Nat,
    },
    /// The ledger is temporarily unavailable.
    TemporarilyUnavailable,
    /// A generic error from the ledger.
    GenericError {
        /// The error code.
        pub error_code: Nat,
        /// The error message.
        pub message: String,
    },
}

/// Response after a successful deposit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct DepositResponse {
    /// The block index of the transfer on the token ledger.
    pub block_index: Nat,
}
