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

/// Information about a listed trading pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct TradingPairInfo {
    /// Token ID of the base asset.
    pub base_asset: TokenId,
    /// Token ID of the quote asset.
    pub quote_asset: TokenId,
    /// Minimum price increment.
    pub tick_size: u64,
    /// Minimum order quantity.
    pub lot_size: u64,
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

/// Errors that can be returned by the ICRC-2 `transfer_from` endpoint on a ledger canister.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum LedgerTransferFromError {
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
    /// The ledger is temporarily unavailable.
    TemporarilyUnavailable,
    /// Internal error
    InternalError(String),
}

/// Response after a successful deposit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct DepositResponse {
    /// The block index of the transfer on the token ledger.
    pub block_index: Nat,
}
