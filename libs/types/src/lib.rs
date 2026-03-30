//! Candid types used by the candid interface of the DEX canister.

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

#[cfg(test)]
mod tests;

use candid::CandidType;
use serde::{Deserialize, Serialize};

/// Unique identifier for an order.
pub type OrderId = u64;

/// Side of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, CandidType)]
pub enum Side {
    /// Buy order (bid).
    Buy,
    /// Sell order (ask).
    Sell,
}

/// Request to place a new limit order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct LimitOrderRequest {
    /// Whether this is a buy or sell order.
    pub side: Side,
    /// Limit price in quote token units per base token unit.
    pub price: u64,
    /// Order quantity in base token units.
    pub quantity: u64,
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
    /// The order is open and resting in the order book.
    Open,
    /// The order has been fully filled.
    Filled,
    /// The order has been canceled.
    Cancelled,
}
