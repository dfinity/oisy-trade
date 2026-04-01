mod book;
#[cfg(test)]
mod tests;

pub use book::{Fill, MatchOrderError, MatchResult, OrderBook};
use candid::Principal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
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

impl From<TokenId> for dex_types::TokenId {
    fn from(value: TokenId) -> Self {
        Self { ledger_id: value.0 }
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

    pub fn is_multiple_of(self, other: Self) -> bool {
        self.0.is_multiple_of(other.0)
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

#[derive(Debug)]
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
