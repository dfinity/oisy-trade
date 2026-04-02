//! Candid types used by the candid interface of the DEX canister.

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(test)]
mod tests;

use candid::{CandidType, Nat, Principal};
use serde::{Deserialize, Serialize};

/// Unique identifier for an order, encoded as a hex string.
pub type OrderId = String;

/// Side of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub enum Side {
    /// Buy order (bid).
    Buy,
    /// Sell order (ask).
    Sell,
}

/// A trading pair identified by the base and quote token ledger principals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub struct TradingPair {
    /// The base token ledger canister principal.
    pub base: Principal,
    /// The quote token ledger canister principal.
    pub quote: Principal,
}

/// Request to place a new limit order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct LimitOrderRequest {
    /// The trading pair to place the order on.
    pub pair: TradingPair,
    /// Whether this is a buy or sell order.
    pub side: Side,
    /// Limit price in quote token units per base token unit.
    pub price: u64,
    /// Order quantity in base token units.
    pub quantity: u64,
}

/// Error returned when placing a limit order fails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum AddLimitOrderError {
    /// The requested trading pair is not registered.
    UnknownTradingPair,
    /// The price is not a positive multiple of the tick size.
    InvalidPrice {
        /// The rejected price.
        price: u64,
        /// The required tick size.
        tick_size: u64,
    },
    /// The quantity is not a positive multiple of the lot size.
    InvalidQuantity {
        /// The rejected quantity.
        quantity: u64,
        /// The required lot size.
        lot_size: u64,
    },
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
    /// The order is open and resting in the order book.
    Open,
    /// The order has been fully filled.
    Filled,
    /// The order has been canceled.
    Canceled,
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

/// Request to add a new trading pair to the DEX.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct AddTradingPairRequest {
    /// The base token of the pair (e.g. ckSOL).
    pub base: TokenId,
    /// The quote token of the pair (e.g. ckBTC).
    pub quote: TokenId,
    /// Minimum price increment. Must be greater than zero.
    pub tick_size: u64,
    /// Minimum order quantity. Must be greater than zero.
    pub lot_size: u64,
}

/// Error returned by the `add_trading_pair` endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum AddTradingPairError {
    /// The caller is not a controller of the canister.
    NotController,
    /// The base and quote tokens are the same.
    BaseEqualsQuote,
    /// The tick size must be greater than zero.
    InvalidTickSize,
    /// The lot size must be greater than zero.
    InvalidLotSize,
    /// A trading pair with the same base and quote tokens already exists.
    TradingPairAlreadyExists,
}
