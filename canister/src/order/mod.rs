mod book;
mod history;
#[cfg(test)]
mod tests;

pub use book::{Fill, MatchOrderError, MatchResult, MatchingOutput, OrderBook};
use candid::{Nat, Principal};
pub use history::OrderRecord;
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use num_bigint::BigUint;
use std::borrow::Cow;
use std::fmt;
use std::num::NonZeroU64;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, minicbor::Encode, minicbor::Decode)]
pub enum Side {
    #[n(0)]
    Buy,
    #[n(1)]
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

impl From<Side> for dex_types::Side {
    fn from(side: Side) -> Self {
        match side {
            Side::Buy => dex_types::Side::Buy,
            Side::Sell => dex_types::Side::Sell,
        }
    }
}

/// Lifecycle state persisted with each [`OrderRecord`]. Mirrors the four real
/// states of [`dex_types::OrderStatus`]; the public `NotFound` variant is
/// synthesized at the canister boundary when no record exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub enum OrderStatus {
    #[n(0)]
    Pending,
    #[n(1)]
    Open,
    #[n(2)]
    Filled,
    #[n(3)]
    Canceled,
}

impl From<OrderStatus> for dex_types::OrderStatus {
    fn from(status: OrderStatus) -> Self {
        match status {
            OrderStatus::Pending => dex_types::OrderStatus::Pending,
            OrderStatus::Open => dex_types::OrderStatus::Open,
            OrderStatus::Filled => dex_types::OrderStatus::Filled,
            OrderStatus::Canceled => dex_types::OrderStatus::Canceled,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct OrderBookId(#[n(0)] u64);

impl OrderBookId {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);

    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub fn increment(&mut self) {
        self.0 = self.0.checked_add(1).expect("OrderBookId overflow");
    }
}

/// Sequence number identifying an order within a single order book.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct OrderSeq(#[n(0)] u64);

impl OrderSeq {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);

    pub const fn new(seq: u64) -> Self {
        Self(seq)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub fn increment(&mut self) {
        self.0 = self.0.checked_add(1).expect("OrderSeq overflow");
    }
}

/// Unique order identifier encoding the order book ID and a per-book sequence number.
///
/// Represented as an opaque 32-character hex string (16 bytes: 8 for book ID, 8 for sequence) to the outside.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, minicbor::Encode, minicbor::Decode,
)]
pub struct OrderId {
    #[n(0)]
    book_id: OrderBookId,
    #[n(1)]
    seq: OrderSeq,
}

impl OrderId {
    pub fn new(book_id: OrderBookId, seq: OrderSeq) -> Self {
        Self { book_id, seq }
    }

    pub fn book_id(&self) -> OrderBookId {
        self.book_id
    }

    pub fn seq(&self) -> OrderSeq {
        self.seq
    }

    pub fn into_parts(self) -> (OrderBookId, OrderSeq) {
        (self.book_id, self.seq)
    }
}

impl Storable for OrderId {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let (book, seq) = self.into_parts();
        let mut buf = [0u8; 16];
        buf[..8].copy_from_slice(&book.get().to_be_bytes());
        buf[8..].copy_from_slice(&seq.get().to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(bytes.len(), 16, "OrderId must decode from exactly 16 bytes");
        let book = u64::from_be_bytes(bytes[..8].try_into().expect("8-byte slice"));
        let seq = u64::from_be_bytes(bytes[8..].try_into().expect("8-byte slice"));
        OrderId::new(OrderBookId::new(book), OrderSeq::new(seq))
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 16,
        is_fixed_size: true,
    };
}

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}{:016x}", self.book_id.0, self.seq.0)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct OrderIdParseError;

impl fmt::Display for OrderIdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid order ID: expected 32-character hex string")
    }
}

impl FromStr for OrderId {
    type Err = OrderIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 32 || !s.is_ascii() {
            return Err(OrderIdParseError);
        }
        let book_id = u64::from_str_radix(&s[..16], 16).map_err(|_| OrderIdParseError)?;
        let seq = u64::from_str_radix(&s[16..], 16).map_err(|_| OrderIdParseError)?;
        Ok(Self {
            book_id: OrderBookId(book_id),
            seq: OrderSeq(seq),
        })
    }
}

impl From<OrderId> for String {
    fn from(id: OrderId) -> Self {
        id.to_string()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, minicbor::Encode, minicbor::Decode,
)]
pub struct TokenId(#[cbor(n(0), with = "icrc_cbor::principal")] Principal);

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

#[derive(Debug, Clone, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct TokenMetadata {
    #[n(0)]
    pub symbol: String,
    #[n(1)]
    pub decimals: u8,
}

impl From<dex_types::TokenMetadata> for TokenMetadata {
    fn from(value: dex_types::TokenMetadata) -> Self {
        Self {
            symbol: value.symbol,
            decimals: value.decimals,
        }
    }
}

impl From<TokenMetadata> for dex_types::TokenMetadata {
    fn from(value: TokenMetadata) -> Self {
        Self {
            symbol: value.symbol,
            decimals: value.decimals,
        }
    }
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

impl From<TradingPair> for dex_types::TradingPair {
    fn from(value: TradingPair) -> Self {
        dex_types::TradingPair {
            base: value.base.0,
            quote: value.quote.0,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, minicbor::Encode, minicbor::Decode,
)]
pub struct Price(#[n(0)] u64);

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

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn mul_quantity(self, quantity: &Quantity) -> Quantity {
        quantity * self.0
    }
}

/// Minimum price increment for a trading pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, minicbor::Encode, minicbor::Decode)]
pub struct TickSize(#[cbor(n(0), with = "crate::cbor::non_zero_u64")] NonZeroU64);

impl TickSize {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

impl From<TickSize> for u64 {
    fn from(tick_size: TickSize) -> Self {
        tick_size.get()
    }
}

/// Minimum order quantity for a trading pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, minicbor::Encode, minicbor::Decode)]
pub struct LotSize(#[cbor(n(0), with = "crate::cbor::non_zero_u64")] NonZeroU64);

impl LotSize {
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

impl From<LotSize> for u64 {
    fn from(lot_size: LotSize) -> Self {
        lot_size.get()
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, minicbor::Encode, minicbor::Decode)]
pub struct Quantity(#[cbor(n(0), with = "icrc_cbor::nat")] Nat);

impl Default for Quantity {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Quantity {
    pub const ZERO: Self = Self(Nat(BigUint::ZERO));

    pub fn is_zero(&self) -> bool {
        self == &Self::ZERO
    }

    pub fn is_multiple_of(&self, lot_size: LotSize) -> bool {
        self.as_big_uint() % lot_size.get() == BigUint::ZERO
    }

    pub fn checked_sub(&self, other: &Self) -> Option<Self> {
        bench_scopes!("qty", "qty::checked_sub");
        if self >= other {
            Some(Quantity(self.0.clone() - other.0.clone()))
        } else {
            None
        }
    }

    fn as_big_uint(&self) -> &BigUint {
        &self.0.0
    }
}

impl From<u64> for Quantity {
    fn from(value: u64) -> Self {
        Self(Nat::from(value))
    }
}

impl From<Nat> for Quantity {
    fn from(value: Nat) -> Self {
        Self(value)
    }
}

impl From<Quantity> for Nat {
    fn from(quantity: Quantity) -> Self {
        quantity.0
    }
}

impl std::ops::Add for Quantity {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        bench_scopes!("qty", "qty::add");
        Quantity(self.0 + rhs.0)
    }
}

impl std::ops::AddAssign for Quantity {
    fn add_assign(&mut self, rhs: Self) {
        bench_scopes!("qty", "qty::add_assign");
        self.0 += rhs.0;
    }
}

impl std::ops::Mul for Quantity {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        bench_scopes!("qty", "qty::mul");
        Quantity(self.0 * rhs.0)
    }
}

impl std::ops::Mul<u64> for &Quantity {
    type Output = Quantity;

    fn mul(self, rhs: u64) -> Self::Output {
        bench_scopes!("qty", "qty::mul_u64");
        Quantity(Nat(self.as_big_uint() * rhs))
    }
}

#[derive(Debug)]
pub struct PendingOrder {
    pub side: Side,
    pub price: Price,
    pub quantity: Quantity,
}

impl From<dex_types::LimitOrderRequest> for PendingOrder {
    fn from(request: dex_types::LimitOrderRequest) -> Self {
        Self {
            side: Side::from(request.side),
            price: Price::from(request.price),
            quantity: Quantity::from(request.quantity),
        }
    }
}

impl PendingOrder {
    pub fn into_order(self, id: OrderSeq) -> Order {
        Order {
            id,
            side: self.side,
            price: self.price,
            remaining_quantity: self.quantity,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    id: OrderSeq,
    side: Side,
    price: Price,
    remaining_quantity: Quantity,
}

impl Order {
    pub fn id(&self) -> OrderSeq {
        self.id
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn price(&self) -> Price {
        self.price
    }

    pub fn remaining_quantity(&self) -> &Quantity {
        &self.remaining_quantity
    }

    pub fn reduce_quantity(&mut self, amount: &Quantity) {
        self.remaining_quantity = self
            .remaining_quantity
            .checked_sub(amount)
            .expect("cannot reduce quantity below zero");
    }
}

/// An order resting in the order book. Only carries the ID and remaining
/// quantity — side and price are implicit from the book's structure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestingOrder {
    id: OrderSeq,
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
            remaining_quantity: self.remaining_quantity.clone(),
        }
    }

    pub fn id(&self) -> OrderSeq {
        self.id
    }

    pub fn remaining_quantity(&self) -> &Quantity {
        &self.remaining_quantity
    }

    pub fn reduce_quantity(&mut self, amount: &Quantity) {
        self.remaining_quantity = self
            .remaining_quantity
            .checked_sub(amount)
            .expect("cannot reduce quantity below zero");
    }
}
