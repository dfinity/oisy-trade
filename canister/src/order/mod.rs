mod book;
#[cfg(test)]
mod tests;

pub use book::OrderBook;
use candid::Principal;
use dex_types::Side;

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
    pub fn new(principal: Principal) -> Self {
        Self(principal)
    }

    pub fn as_principal(&self) -> &Principal {
        &self.0
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

#[derive(Debug, PartialEq, Eq)]
pub enum MatchOrderError {
    /// Price is not a multiple of the tick size.
    InvalidTickSize { price: Price, tick_size: Price },
    /// Quantity is not a multiple of the lot size.
    InvalidLotSize {
        quantity: Quantity,
        lot_size: Quantity,
    },
}

/// A single fill produced when an incoming order matches a resting order.
#[derive(Debug, PartialEq, Eq)]
pub struct Fill {
    /// The ID of the resting (maker) order that was matched.
    pub maker_order_id: OrderId,
    /// The price at which the fill occurred (always the maker's price).
    pub price: Price,
    /// The quantity filled.
    pub quantity: Quantity,
}

/// The result of matching an incoming order against the book.
#[derive(Debug, PartialEq, Eq)]
pub enum MatchResult {
    /// The order was fully filled and does not rest in the book.
    Filled { fills: Vec<Fill> },
    /// The order was partially filled and the remainder is now resting in the book.
    PartiallyFilled {
        fills: Vec<Fill>,
        resting_order_id: OrderId,
    },
    /// No match was found; the order is resting in the book.
    Resting { resting_order_id: OrderId },
}

impl MatchResult {
    pub fn fills(&self) -> &[Fill] {
        match self {
            MatchResult::Filled { fills } | MatchResult::PartiallyFilled { fills, .. } => fills,
            MatchResult::Resting { .. } => &[],
        }
    }
}
