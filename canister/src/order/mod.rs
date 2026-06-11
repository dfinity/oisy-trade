mod book;
mod fees;
mod history;
#[cfg(test)]
mod tests;

pub use book::{
    Fill, MatchOrderError, MatchResult, MatchingOutput, OrderBook, OrderBookSnapshot, PriceLevel,
    RemovedOrder,
};
pub use fees::{BasisPoint, FeeRates, InvalidBasisPoint};
pub use history::OrderHistory;

use candid::{Nat, Principal};
pub use history::OrderRecord;
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use minicbor::{Decode, Encode};
use num_bigint::BigUint;
use std::borrow::Cow;
use std::fmt;
use std::num::{NonZeroU64, NonZeroU128};
use std::str::FromStr;

/// Selector for the base or quote token of a [`TradingPair`]. Resolved to a
/// concrete [`TokenId`] via [`TradingPair::token`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Decode, Encode)]
pub enum PairToken {
    #[n(0)]
    Base,
    #[n(1)]
    Quote,
}

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
    Canceled(#[n(0)] CanceledOrderInfo),
}

/// Fill information captured when an order transitions to `Canceled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct CanceledOrderInfo {
    /// Quantity that was still open on the book at the moment of cancel and will never be filled.
    #[n(0)]
    pub remaining_quantity: Quantity,
}

impl From<OrderStatus> for dex_types::OrderStatus {
    fn from(status: OrderStatus) -> Self {
        match status {
            OrderStatus::Pending => dex_types::OrderStatus::Pending,
            OrderStatus::Open => dex_types::OrderStatus::Open,
            OrderStatus::Filled => dex_types::OrderStatus::Filled,
            OrderStatus::Canceled(info) => dex_types::OrderStatus::Canceled(info.into()),
        }
    }
}

impl From<CanceledOrderInfo> for dex_types::CanceledOrderInfo {
    fn from(info: CanceledOrderInfo) -> Self {
        dex_types::CanceledOrderInfo {
            remaining_quantity: info.remaining_quantity.into(),
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
    pub const ZERO: Self = Self {
        book_id: OrderBookId::ZERO,
        seq: OrderSeq::ZERO,
    };

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

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, minicbor::Encode, minicbor::Decode,
)]
pub struct TradingPair {
    #[n(0)]
    pub base: TokenId,
    #[n(1)]
    pub quote: TokenId,
}

impl TradingPair {
    pub fn token(&self, side: &PairToken) -> TokenId {
        match side {
            PairToken::Base => self.base,
            PairToken::Quote => self.quote,
        }
    }
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
pub struct Price(#[cbor(n(0), with = "crate::cbor::u128_codec")] u128);

impl Price {
    pub const ZERO: Self = Self(0);

    pub fn new(value: u128) -> Self {
        Self(value)
    }

    pub fn get(self) -> u128 {
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

    /// Quote-token amount owed for `quantity` base units at this price:
    /// `price × quantity / base_scale`, where `base_scale = 10^base_decimals`.
    ///
    /// Exact (zero remainder) by the pair-creation invariant: `price` is a
    /// multiple of the tick, `quantity` a multiple of the lot, and
    /// `tick × lot` a multiple of `base_scale`. Returns `None` only if the
    /// intermediate `price × quantity` overflows 256 bits.
    pub fn checked_mul_quantity_scaled(
        self,
        quantity: &Quantity,
        base_scale: NonZeroU64,
    ) -> Option<Quantity> {
        let (quote, remainder) = quantity
            .checked_mul_u128(self.0)?
            .checked_div_rem_u64(base_scale.get())?;
        assert_eq!(
            remainder, 0,
            "BUG: settlement not exact — pair invariant violated"
        );
        Some(quote)
    }
}

/// Minimum price increment for a trading pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, minicbor::Encode, minicbor::Decode)]
pub struct TickSize(#[cbor(n(0), with = "crate::cbor::non_zero_u128")] NonZeroU128);

impl TickSize {
    pub const fn new(value: NonZeroU128) -> Self {
        Self(value)
    }

    pub fn get(self) -> u128 {
        self.0.get()
    }
}

impl From<TickSize> for u128 {
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

impl From<u128> for Price {
    fn from(value: u128) -> Self {
        Self(value)
    }
}

impl From<Price> for u128 {
    fn from(price: Price) -> Self {
        price.0
    }
}

impl From<Price> for Nat {
    fn from(price: Price) -> Self {
        Nat::from(price.get())
    }
}

impl From<TickSize> for Nat {
    fn from(tick_size: TickSize) -> Self {
        Nat::from(tick_size.get())
    }
}

impl From<LotSize> for Nat {
    fn from(lot_size: LotSize) -> Self {
        Nat::from(lot_size.get())
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

    pub(crate) fn high(&self) -> u128 {
        self.high
    }

    pub(crate) fn low(&self) -> u128 {
        self.low
    }

    pub const fn is_zero(&self) -> bool {
        self.high == 0 && self.low == 0
    }

    pub fn is_multiple_of(&self, lot_size: LotSize) -> bool {
        let divisor = lot_size.get();
        // Fast path for the common small-quantity case (`high == 0`):
        // a single `u128 % u64` is ~250 instructions cheaper than the
        // long-division below, and validation paths place dense calls.
        if self.high == 0 {
            return self.low.is_multiple_of(divisor as u128);
        }
        let (_, remainder) = (*self)
            .checked_div_rem_u64(divisor)
            .expect("LotSize is NonZeroU64");
        remainder == 0
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        bench_scopes!("qty", "qty::checked_sub");
        if other.is_zero() {
            return Some(self);
        }
        let (low, borrow) = self.low.overflowing_sub(other.low);
        let high = self
            .high
            .checked_sub(other.high.checked_add(borrow as u128)?)?;
        Some(Self { high, low })
    }

    /// Serialize as a 32-byte big-endian representation.
    pub fn to_be_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[..16].copy_from_slice(&self.high.to_be_bytes());
        bytes[16..].copy_from_slice(&self.low.to_be_bytes());
        bytes
    }

    /// Deserialize from a big-endian byte slice (up to 32 bytes).
    ///
    /// Returns `None` if the slice is longer than 32 bytes.
    pub fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > 32 {
            return None;
        }
        let mut buf = [0u8; 32];
        buf[32 - bytes.len()..].copy_from_slice(bytes);
        Some(Self {
            high: u128::from_be_bytes(buf[..16].try_into().unwrap()),
            low: u128::from_be_bytes(buf[16..].try_into().unwrap()),
        })
    }

    /// Convert to `Nat` for Candid serialization.
    pub fn to_nat(&self) -> Nat {
        if self.high == 0 {
            Nat::from(self.low)
        } else {
            Nat(BigUint::from_bytes_be(&self.to_be_bytes()))
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

impl From<u128> for Quantity {
    fn from(value: u128) -> Self {
        Self::from_u128(value)
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
        if value.0.bits() > 256 {
            return Err(QuantityOverflowError);
        }
        Self::from_be_bytes(&value.0.to_bytes_be()).ok_or(QuantityOverflowError)
    }
}

impl From<Quantity> for Nat {
    fn from(quantity: Quantity) -> Self {
        quantity.to_nat()
    }
}

impl Quantity {
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        bench_scopes!("qty", "qty::add");
        let (low, carry) = self.low.overflowing_add(rhs.low);
        let high = self
            .high
            .checked_add(rhs.high)?
            .checked_add(carry as u128)?;
        Some(Self { high, low })
    }

    /// Multiply this u256 by a `u128`, checked for overflow past 256 bits.
    ///
    /// Schoolbook multiplication on 64-bit limbs (`self` = four limbs, `rhs` =
    /// two). Each accumulation `r[i+j] + a[i]·b[j] + carry` stays within `u128`
    /// because `(2^64−1)² + 2·(2^64−1) = 2^128 − 1`. Limbs 4 and 5 must be zero
    /// for the product to fit a u256.
    pub fn checked_mul_u128(self, rhs: u128) -> Option<Self> {
        bench_scopes!("qty", "qty::mul_u128");
        const MASK: u128 = u64::MAX as u128;
        let a = [
            self.low & MASK,
            self.low >> 64,
            self.high & MASK,
            self.high >> 64,
        ];
        let b = [rhs & MASK, rhs >> 64];
        let mut r = [0u128; 6];
        for i in 0..4 {
            let mut carry = 0u128;
            for j in 0..2 {
                let cur = r[i + j] + a[i] * b[j] + carry;
                r[i + j] = cur & MASK;
                carry = cur >> 64;
            }
            let mut k = i + 2;
            while carry != 0 {
                if k >= 6 {
                    return None;
                }
                let cur = r[k] + carry;
                r[k] = cur & MASK;
                carry = cur >> 64;
                k += 1;
            }
        }
        if r[4] != 0 || r[5] != 0 {
            return None;
        }
        Some(Self {
            high: r[2] | (r[3] << 64),
            low: r[0] | (r[1] << 64),
        })
    }

    pub fn checked_mul_u64(self, rhs: u64) -> Option<Self> {
        bench_scopes!("qty", "qty::mul_u64");
        // We want (high * 2^128 + low) * rhs, checked for overflow.
        // low * rhs can overflow u128, so split low into two 64-bit halves
        // and multiply each by rhs (u64 × u64 → u128, no overflow).
        let rhs = rhs as u128;
        let low_lo = self.low & 0xFFFF_FFFF_FFFF_FFFF;
        let low_hi = self.low >> 64;
        let prod_lo = low_lo * rhs;
        let prod_hi = low_hi * rhs;
        // Reassemble the low limb: prod_hi is shifted left by 64 bits.
        let (low, carry) = prod_lo.overflowing_add(prod_hi << 64);
        // Build the high limb: high * rhs + overflow from low multiplication.
        let high = self
            .high
            .checked_mul(rhs)?
            .checked_add(prod_hi >> 64)? // upper bits of prod_hi that spilled past bit 128
            .checked_add(carry as u128)?; // carry from the low addition
        Some(Self { high, low })
    }

    /// Integer-divide `self` by a u64 divisor, returning `(quotient, remainder)`.
    /// Returns `None` if `divisor` is zero.
    ///
    /// Schoolbook long division on `self` split into three chunks:
    /// the full `high` limb (top 128 bits) and the two 64-bit halves
    /// of `low`. Write `self = high · 2^128 + low_hi · 2^64 + low_lo`
    /// and divide chunk-by-chunk, carrying each remainder into the
    /// next step.
    ///
    /// **Step 1.** Divide the high limb: `high = q1 · d + r1`.
    /// Substituting back and grouping the settled term:
    ///
    /// ```text
    /// self = q1 · d · 2^128  +  r1 · 2^128 + low_hi · 2^64 + low_lo
    ///        └── settled ──┘    └────── still to divide ──────┘
    /// ```
    ///
    /// **Step 2.** Factor a `2^64` out of the leftover's top two
    /// terms (since `2^128 = 2^64 · 2^64`) — the leftover now has
    /// the same shape as the original, one 64-bit digit shorter:
    ///
    /// ```text
    /// r1 · 2^128 + low_hi · 2^64 + low_lo
    ///     = (r1 · 2^64 + low_hi) · 2^64 + low_lo
    /// ```
    ///
    /// The new leading 128-bit chunk `(r1 · 2^64 + low_hi)` is
    /// step 2's dividend: `r1 · 2^64 + low_hi = q2 · d + r2`.
    ///
    /// **Step 3.** Same factoring again leaves `r2 · 2^64 + low_lo`
    /// as the final dividend: `r2 · 2^64 + low_lo = q3 · d + r3`.
    ///
    /// ```text
    /// quotient  = q1 · 2^128 + q2 · 2^64 + q3
    /// remainder = r3
    /// ```
    ///
    /// Every dividend fits in u128: each `rᵢ < d ≤ 2^64`, leaving
    /// room for the `· 2^64` shift before the next 64-bit chunk is
    /// OR'd in. The final remainder is `< divisor ≤ u64::MAX`, so
    /// it always fits in u64.
    pub fn checked_div_rem_u64(self, divisor: u64) -> Option<(Self, u64)> {
        bench_scopes!("qty", "qty::div_rem_u64");
        if divisor == 0 {
            return None;
        }
        let d = divisor as u128;

        // Fast path for the common small-quantity case (`high == 0`): the
        // value fits in a u128, so a single native `/` and `%` replace the
        // three-chunk long division below. Real-world notionals/quantities
        // are well under 2^128, so `mul_ceil` (the fee path) hits this on
        // every production fill.
        if self.high == 0 {
            return Some((Self::from_u128(self.low / d), (self.low % d) as u64));
        }

        // Step 1: divide the high limb. r1 < d ≤ u64::MAX < 2^64.
        let q1 = self.high / d;
        let r1 = self.high % d;

        // Step 2: dividend is `r1 · 2^64 + low_hi`, fits in u128 since r1 < 2^64.
        let low_hi = self.low >> 64;
        let low_lo = self.low & 0xFFFF_FFFF_FFFF_FFFF;
        let dividend2 = (r1 << 64) | low_hi;
        let q2 = dividend2 / d;
        let r2 = dividend2 % d;

        // Step 3: dividend is `r2 · 2^64 + low_lo`. Same overflow argument.
        let dividend3 = (r2 << 64) | low_lo;
        let q3 = dividend3 / d;
        let r3 = dividend3 % d;

        // q2 < 2^64 and q3 < 2^64, so they combine into the low u128.
        let low_out = (q2 << 64) | q3;
        Some((
            Self {
                high: q1,
                low: low_out,
            },
            r3 as u64,
        ))
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
            let buf = self.to_be_bytes();
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
        Self::from_be_bytes(bytes)
            .ok_or_else(|| minicbor::decode::Error::message("Quantity exceeds 256 bits"))
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
            price: Price::from(
                u128::try_from(&request.price.0).map_err(|_| QuantityOverflowError)?,
            ),
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

#[derive(Clone, Debug, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct Order {
    #[n(0)]
    id: OrderSeq,
    #[n(1)]
    side: Side,
    #[n(2)]
    price: Price,
    #[n(3)]
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
            .checked_sub(*amount)
            .expect("cannot reduce quantity below zero");
    }
}

/// An order resting in the order book. Only carries the ID and remaining
/// quantity — side and price are implicit from the book's structure.
#[derive(Clone, Debug, PartialEq, Eq, minicbor::Encode, minicbor::Decode)]
pub struct RestingOrder {
    #[n(0)]
    id: OrderSeq,
    #[n(1)]
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
            .checked_sub(*amount)
            .expect("cannot reduce quantity below zero");
    }
}
