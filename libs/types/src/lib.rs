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
pub struct TokenId {
    /// The canister ID of the token's ledger.
    pub ledger_id: Principal,
}

/// Request to deposit tokens into the DEX.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct DepositRequest {
    /// The token to deposit.
    pub token_id: TokenId,
    /// The amount to deposit.
    pub amount: Nat,
}

/// Error returned by the deposit endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum DepositError {
    /// The inter-canister call to the token ledger failed.
    CallFailed {
        /// The ledger canister that was called.
        ledger: Principal,
        /// The name of the method that was called.
        method: String,
        /// The reason the call failed.
        reason: String,
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
        expected_fee: Nat,
    },
    /// The burn amount was below the minimum.
    BadBurn {
        /// The minimum burn amount.
        min_burn_amount: Nat,
    },
    /// The source account does not hold enough funds.
    InsufficientFunds {
        /// The current balance of the source account.
        balance: Nat,
    },
    /// The caller's allowance is not large enough.
    InsufficientAllowance {
        /// The current allowance.
        allowance: Nat,
    },
    /// The transaction is too old.
    TooOld,
    /// The transaction was created in the future.
    CreatedInFuture {
        /// The current ledger time.
        ledger_time: u64,
    },
    /// The transaction is a duplicate.
    Duplicate {
        /// The block index of the duplicate transaction.
        duplicate_of: Nat,
    },
    /// The ledger is temporarily unavailable.
    TemporarilyUnavailable,
    /// A generic error from the ledger.
    GenericError {
        /// The error code.
        error_code: Nat,
        /// The error message.
        message: String,
    },
}

/// Response after a successful deposit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct DepositResponse {
    /// The block index of the transfer on the token ledger.
    pub block_index: Nat,
}
