mod book;
#[cfg(test)]
mod tests;

pub use book::{Fill, MatchOrderError, MatchResult, OrderBook};
use candid::Principal;
use std::num::NonZeroU64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

impl From<dex_types::Side> for Side {
    fn from(side: dex_types::Side) -> Self {
        match side {
            dex_types::Side::Buy => Side::Buy,
            dex_types::Side::Sell => Side::Sell,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderId(u64);

impl OrderId {
    pub const ZERO: Self = Self(0);

    pub fn increment(&mut self) {
        self.0 = self.0.checked_add(1).expect("OrderId overflow");
    }
}

impl From<u64> for OrderId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<OrderId> for u64 {
    fn from(id: OrderId) -> Self {
        id.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TokenId(Principal);

impl TokenId {
    pub const fn new(principal: Principal) -> Self {
        Self(principal)
    }

    pub fn as_principal(&self) -> &Principal {
        &self.0
    }
}

impl From<dex_types::TokenId> for TokenId {
    fn from(value: dex_types::TokenId) -> Self {
        Self(value.ledger_id)
    }
}

#[derive(Debug, Clone)]
pub struct TokenMetadata {
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradingPair {
    pub base: TokenId,
    pub quote: TokenId,
}

impl From<dex_types::TradingPair> for TradingPair {
    fn from(pair: dex_types::TradingPair) -> Self {
        Self {
            base: TokenId::new(pair.base),
            quote: TokenId::new(pair.quote),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(u64);

impl Price {
    pub const ZERO: Self = Self(0);

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn is_multiple_of(self, tick_size: TickSize) -> bool {
        self.0.is_multiple_of(tick_size.get())
    }
}

/// Minimum price increment for a trading pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TickSize(NonZeroU64);

impl TickSize {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

impl From<u64> for Price {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<Price> for u64 {
    fn from(price: Price) -> Self {
        price.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quantity(u64);

impl Quantity {
    pub const ZERO: Self = Self(0);

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn is_multiple_of(self, other: Self) -> bool {
        self.0.is_multiple_of(other.0)
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }
}

impl From<u64> for Quantity {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<Quantity> for u64 {
    fn from(quantity: Quantity) -> Self {
        quantity.0
    }
}

#[derive(Debug)]
pub struct PendingOrder {
    pub side: Side,
    pub price: Price,
    pub quantity: Quantity,
}

impl PendingOrder {
    pub fn into_order(self, id: OrderId) -> Order {
        Order {
            id,
            side: self.side,
            price: self.price,
            remaining_quantity: self.quantity,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Order {
    id: OrderId,
    side: Side,
    price: Price,
    remaining_quantity: Quantity,
}

impl Order {
    pub fn id(&self) -> OrderId {
        self.id
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn price(&self) -> Price {
        self.price
    }

    pub fn remaining_quantity(&self) -> Quantity {
        self.remaining_quantity
    }

    pub fn reduce_quantity(&mut self, amount: Quantity) {
        self.remaining_quantity = self
            .remaining_quantity
            .checked_sub(amount)
            .expect("cannot reduce quantity below zero");
    }
}

/// An order resting in the order book. Only carries the ID and remaining
/// quantity — side and price are implicit from the book's structure.
#[derive(Debug, PartialEq, Eq)]
pub struct RestingOrder {
    id: OrderId,
    remaining_quantity: Quantity,
}

impl From<Order> for RestingOrder {
    fn from(order: Order) -> Self {
        Self {
            id: order.id,
            remaining_quantity: order.remaining_quantity,
        }
    }
}

impl RestingOrder {
    /// Reconstruct a full [`Order`] by combining the resting order with its
    /// side and price (which are stored in the book's index, not on the order itself).
    pub fn to_order(&self, side: Side, price: Price) -> Order {
        Order {
            id: self.id,
            side,
            price,
            remaining_quantity: self.remaining_quantity,
        }
    }

    pub fn id(&self) -> OrderId {
        self.id
    }

    pub fn remaining_quantity(&self) -> Quantity {
        self.remaining_quantity
    }

    pub fn reduce_quantity(&mut self, amount: Quantity) {
        self.remaining_quantity = self
            .remaining_quantity
            .checked_sub(amount)
            .expect("cannot reduce quantity below zero");
    }
}
