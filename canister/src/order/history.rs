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
    /// Creates an empty order history.
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

    /// Returns the record for the given order, or `None` if absent.
    pub fn get(&self, id: &OrderId) -> Option<&OrderRecord> {
        self.orders.get(id)
    }

    /// Returns a mutable reference to the status of the given order, or `None` if absent.
    ///
    /// # Example
    ///
    /// Change the status of an order from Pending to Filled:
    /// ```
    /// # use dex_canister::order::{OrderHistory, OrderRecord, OrderId, OrderBookId, OrderSeq, Price, Quantity, Side, TradingPair, TokenId};
    /// # use dex_types::OrderStatus;
    /// # use candid::Principal;
    /// let mut history = OrderHistory::new();
    /// let id = OrderId::new(OrderBookId::ZERO, OrderSeq::new(0));
    /// let pair = TradingPair {
    ///     base: TokenId::new(Principal::anonymous()),
    ///     quote: TokenId::new(Principal::anonymous()),
    /// };
    /// history.insert_once(id, OrderRecord {
    ///     owner: Principal::anonymous(),
    ///     pair,
    ///     side: Side::Buy,
    ///     price: Price::new(100),
    ///     quantity: Quantity::from(1_000_000u64),
    ///     status: OrderStatus::Pending,
    /// });
    ///
    /// *history.get_status_mut(&id).unwrap() = OrderStatus::Filled;
    /// assert_eq!(history.get_status(&id), OrderStatus::Filled);
    /// ```
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
