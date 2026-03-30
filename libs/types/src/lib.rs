//! Candid types used by the candid interface of the DEX canister.

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(test)]
mod tests;

use candid::{CandidType, Nat};
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

/// A token that can be used in the DEX, either as a base or quote asset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, CandidType)]
pub struct Token {
    /// The token name.
    pub name: String,
    /// The token symbol.
    pub symbol: String,
    /// The number of decimal places used by the token.
    pub decimals: u8,
    /// The ledger canister ID associated with this token.
    pub ledger_id: candid::Principal,
    /// The fee charged for transferring this token.
    pub fee: Nat,
}
