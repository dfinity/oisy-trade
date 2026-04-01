//! Candid types used by the candid interface of the DEX canister.

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(test)]
mod tests;

use candid::{CandidType, Principal};
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
    /// Ledger canister ID of the base asset.
    pub base_asset: Principal,
    /// Ledger canister ID of the quote asset.
    pub quote_asset: Principal,
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
