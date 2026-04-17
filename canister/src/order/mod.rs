mod book;
mod history;
#[cfg(test)]
mod tests;

pub use book::{Fill, MatchOrderError, MatchResult, MatchingOutput, OrderBook};
use candid::{Nat, Principal};
pub use history::{OrderHistory, OrderRecord};
use num_bigint::BigUint;
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
        quantity
            .checked_mul_u64(self.0)
            .expect("BUG: price * quantity overflow")
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

/// A 256-bit unsigned quantity represented as `(high, low)` pair of `u128`.
///
/// Stack-allocated and `Copy`. In practice `high` is almost always zero
/// (single token amounts fit in `u128`); only intermediate products like
/// `price × quantity` may use the high limb.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Quantity {
    high: u128,
    low: u128,
}

impl Quantity {
    pub const ZERO: Self = Self { high: 0, low: 0 };
    pub const MAX: Self = Self {
        high: u128::MAX,
        low: u128::MAX,
    };

    pub const fn new(high: u128, low: u128) -> Self {
        Self { high, low }
    }

    pub const fn from_u128(value: u128) -> Self {
        Self {
            high: 0,
            low: value,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.high == 0 && self.low == 0
    }

    pub fn is_multiple_of(&self, lot_size: LotSize) -> bool {
        // For lot sizes that fit in u64, only need to check low limb
        // (if high == 0, which is the common case).
        let divisor = lot_size.get() as u128;
        if self.high == 0 {
            self.low.is_multiple_of(divisor)
        } else {
            // Full u256 % u64: use the identity (high * 2^128 + low) % d
            let high_rem = self.high % divisor;
            // 2^128 mod d
            let shift_rem = (u128::MAX % divisor + 1) % divisor;
            let combined = (high_rem.wrapping_mul(shift_rem) + self.low % divisor) % divisor;
            combined == 0
        }
    }

    pub fn checked_sub(&self, other: &Self) -> Option<Self> {
        #[cfg(feature = "canbench-rs")]
        let _q = canbench_rs::bench_scope("qty");
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("qty::checked_sub");
        let (low, borrow) = self.low.overflowing_sub(other.low);
        let high = self.high.checked_sub(other.high + borrow as u128)?;
        Some(Self { high, low })
    }

    /// Convert to `Nat` for Candid serialization.
    pub fn to_nat(&self) -> Nat {
        if self.high == 0 {
            Nat::from(self.low)
        } else {
            let mut bytes = [0u8; 32];
            bytes[..16].copy_from_slice(&self.high.to_be_bytes());
            bytes[16..].copy_from_slice(&self.low.to_be_bytes());
            Nat(BigUint::from_bytes_be(&bytes))
        }
    }
}

impl Ord for Quantity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.high.cmp(&other.high).then(self.low.cmp(&other.low))
    }
}

impl PartialOrd for Quantity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl From<u64> for Quantity {
    fn from(value: u64) -> Self {
        Self {
            high: 0,
            low: value as u128,
        }
    }
}

/// Error returned when a `Nat` value exceeds the 256-bit capacity of `Quantity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantityOverflowError;

impl std::fmt::Display for QuantityOverflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "value exceeds u256 capacity")
    }
}

impl TryFrom<Nat> for Quantity {
    type Error = QuantityOverflowError;

    fn try_from(value: Nat) -> Result<Self, Self::Error> {
        let bytes = value.0.to_bytes_be();
        if bytes.len() <= 16 {
            let mut buf = [0u8; 16];
            buf[16 - bytes.len()..].copy_from_slice(&bytes);
            Ok(Self {
                high: 0,
                low: u128::from_be_bytes(buf),
            })
        } else if bytes.len() <= 32 {
            let mut low_buf = [0u8; 16];
            let mut high_buf = [0u8; 16];
            let high_len = bytes.len() - 16;
            high_buf[16 - high_len..].copy_from_slice(&bytes[..high_len]);
            low_buf.copy_from_slice(&bytes[high_len..]);
            Ok(Self {
                high: u128::from_be_bytes(high_buf),
                low: u128::from_be_bytes(low_buf),
            })
        } else {
            Err(QuantityOverflowError)
        }
    }
}

impl From<Quantity> for Nat {
    fn from(quantity: Quantity) -> Self {
        quantity.to_nat()
    }
}

impl Quantity {
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        #[cfg(feature = "canbench-rs")]
        let _q = canbench_rs::bench_scope("qty");
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("qty::add");
        let (low, carry) = self.low.overflowing_add(rhs.low);
        let high = self
            .high
            .checked_add(rhs.high)?
            .checked_add(carry as u128)?;
        Some(Self { high, low })
    }

    pub fn checked_mul_u64(self, rhs: u64) -> Option<Self> {
        #[cfg(feature = "canbench-rs")]
        let _q = canbench_rs::bench_scope("qty");
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("qty::mul_u64");
        let rhs = rhs as u128;
        let low_lo = self.low & 0xFFFF_FFFF_FFFF_FFFF;
        let low_hi = self.low >> 64;
        let prod_lo = low_lo * rhs;
        let prod_hi = low_hi * rhs;
        let (low, carry) = prod_lo.overflowing_add(prod_hi << 64);
        let high = self
            .high
            .checked_mul(rhs)?
            .checked_add(prod_hi >> 64)?
            .checked_add(carry as u128)?;
        Some(Self { high, low })
    }
}

/// CBOR encoding of large numbers:
/// - Values ≤ u64::MAX: encoded as a CBOR unsigned integer (1–9 bytes).
/// - Values > u64::MAX: encoded as Tag 2 (PosBignum) + big-endian byte string.
impl<C> minicbor::Encode<C> for Quantity {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        if self.high == 0 && self.low <= u64::MAX as u128 {
            e.u64(self.low as u64)?;
        } else {
            let mut buf = [0u8; 32];
            buf[..16].copy_from_slice(&self.high.to_be_bytes());
            buf[16..].copy_from_slice(&self.low.to_be_bytes());
            let start = buf.iter().position(|&b| b != 0).unwrap_or(buf.len());
            e.tag(minicbor::data::Tag::PosBignum)?
                .bytes(&buf[start..])?;
        }
        Ok(())
    }
}

impl<'b, C> minicbor::Decode<'b, C> for Quantity {
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        // Try decoding as a plain CBOR unsigned integer first.
        let pos = d.position();
        match d.u64() {
            Ok(n) => return Ok(Self::from(n)),
            Err(e) if e.is_type_mismatch() => d.set_position(pos),
            Err(e) => return Err(e),
        }
        // Otherwise expect Tag 2 (PosBignum) + byte string.
        let tag = d.tag()?;
        if tag != minicbor::data::Tag::PosBignum {
            return Err(minicbor::decode::Error::message(
                "expected u64 or Tag::PosBignum for Quantity",
            ));
        }
        let bytes = d.bytes()?;
        if bytes.len() > 32 {
            return Err(minicbor::decode::Error::message(
                "Quantity exceeds 256 bits",
            ));
        }
        let mut buf = [0u8; 32];
        buf[32 - bytes.len()..].copy_from_slice(bytes);
        let high = u128::from_be_bytes(buf[..16].try_into().unwrap());
        let low = u128::from_be_bytes(buf[16..].try_into().unwrap());
        Ok(Self { high, low })
    }
}

#[derive(Debug)]
pub struct PendingOrder {
    pub side: Side,
    pub price: Price,
    pub quantity: Quantity,
}

impl TryFrom<dex_types::LimitOrderRequest> for PendingOrder {
    type Error = QuantityOverflowError;

    fn try_from(request: dex_types::LimitOrderRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            side: Side::from(request.side),
            price: Price::from(request.price),
            quantity: Quantity::try_from(request.quantity)?,
        })
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
            remaining_quantity: self.remaining_quantity,
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
