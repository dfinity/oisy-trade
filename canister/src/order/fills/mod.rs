use super::ids::{Composite, parse_hex};
use super::{FillSeq, OrderBookId, OrderId};
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use std::borrow::Cow;

#[cfg(test)]
mod tests;

/// Identity of a match: the order book it happened in and the per-book
/// [`FillSeq`] the book minted for it. Mirrors [`OrderId`] — opaque outside
/// the canister as a 32-character hex string (8 bytes book + 8 bytes seq) —
/// and is derivable from any [`TradeId`] by dropping its `OrderSeq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FillId(Composite<OrderBookId, FillSeq>);

impl FillId {
    pub fn new(book_id: OrderBookId, seq: FillSeq) -> Self {
        Self(Composite::new(book_id, seq))
    }

    pub fn book_id(&self) -> OrderBookId {
        self.0.first()
    }

    pub fn seq(&self) -> FillSeq {
        self.0.second()
    }
}

impl std::fmt::Display for FillId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// The string passed to [`FillId`]'s [`FromStr`](std::str::FromStr) was not a
/// 32-character hex string.
#[derive(Debug, PartialEq, Eq)]
pub struct FillIdParseError;

impl std::fmt::Display for FillIdParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid fill ID: expected 32-character hex string")
    }
}

impl std::str::FromStr for FillId {
    type Err = FillIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_hex(s).map(Self).ok_or(FillIdParseError)
    }
}

impl From<FillId> for String {
    fn from(id: FillId) -> Self {
        id.to_string()
    }
}

impl Storable for FillId {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.into_bytes())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0.into_bytes()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Self(Composite::from_bytes(bytes))
    }

    const BOUND: Bound = <Composite<OrderBookId, FillSeq> as Storable>::BOUND;
}

/// Primary store key and per-side identity: the owning [`OrderId`] followed by
/// the match's per-book [`FillSeq`]. A range scan over an `order` prefix yields
/// that order's trades in `seq` order; reversed, newest-first. Mirrors
/// [`OrderId`]: opaque outside the canister as a 48-character hex string (16
/// bytes of `OrderId` + 8 bytes of `seq`). The match's [`FillId`] is derivable
/// via [`TradeId::fill_id`].
///
/// Both fields are fixed-width big-endian, so the derived field-wise `Ord`
/// matches the [`Storable`] byte order that `StableBTreeMap` relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradeId(Composite<OrderId, FillSeq>);

impl TradeId {
    pub fn new(order: OrderId, seq: FillSeq) -> Self {
        Self(Composite::new(order, seq))
    }

    pub fn order_id(&self) -> OrderId {
        self.0.first()
    }

    pub fn seq(&self) -> FillSeq {
        self.0.second()
    }

    /// The id of the match this trade is one side of — the owning order's book
    /// paired with the shared `FillSeq`, dropping the `OrderSeq`.
    pub fn fill_id(&self) -> FillId {
        FillId::new(self.order_id().book_id(), self.seq())
    }
}

impl std::fmt::Display for TradeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// The string passed to [`TradeId`]'s [`FromStr`](std::str::FromStr) was not a
/// 48-character hex string.
#[derive(Debug, PartialEq, Eq)]
pub struct TradeIdParseError;

impl std::fmt::Display for TradeIdParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid trade ID: expected 48-character hex string")
    }
}

impl std::str::FromStr for TradeId {
    type Err = TradeIdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_hex(s).map(Self).ok_or(TradeIdParseError)
    }
}

impl From<TradeId> for String {
    fn from(id: TradeId) -> Self {
        id.to_string()
    }
}

impl Storable for TradeId {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(self.0.into_bytes())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.0.into_bytes()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Self(Composite::from_bytes(bytes))
    }

    const BOUND: Bound = <Composite<OrderId, FillSeq> as Storable>::BOUND;
}
