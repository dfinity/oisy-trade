use super::{OrderId, Price, Quantity, Side, TradingPair};
use candid::Principal;
use dex_types::OrderStatus;
use std::collections::BTreeMap;

/// Record of an order from submission through terminal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRecord {
    pub owner: Principal,
    pub pair: TradingPair,
    pub side: Side,
    pub price: Price,
    pub quantity: Quantity,
    pub status: OrderStatus,
}

/// Tracks all orders from submission to terminal state.
///
/// Wraps the underlying storage to provide a seam for future trimming
/// or migration to stable memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderHistory {
    orders: BTreeMap<OrderId, OrderRecord>,
}

impl OrderHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new order record. Panics if the order ID already exists.
    pub fn insert_once(&mut self, id: OrderId, record: OrderRecord) {
        assert_eq!(
            self.orders.insert(id, record),
            None,
            "BUG: duplicate order ID {id}"
        );
    }

    pub fn get(&self, id: &OrderId) -> Option<&OrderRecord> {
        self.orders.get(id)
    }

    pub fn get_status_mut(&mut self, id: &OrderId) -> Option<&mut OrderStatus> {
        self.orders.get_mut(id).map(|r| &mut r.status)
    }

    /// Returns the status of the given order, or [`OrderStatus::NotFound`] if absent.
    pub fn get_status(&self, id: &OrderId) -> OrderStatus {
        self.orders
            .get(id)
            .map(|r| r.status.clone())
            .unwrap_or(OrderStatus::NotFound)
    }
}
