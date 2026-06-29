use super::ids::book_scoped_id;
use super::{FillSeq, OrderId};
use ic_stable_structures::Storable;
use ic_stable_structures::storable::Bound;
use std::borrow::Cow;

#[cfg(test)]
mod tests;

book_scoped_id! {
    /// Identity of a match: the order book it happened in and the per-book
    /// [`FillSeq`] the book minted for it. Mirrors [`OrderId`] — opaque outside
    /// the canister as a 32-character hex string (8 bytes book + 8 bytes seq) —
    /// and is derivable from any [`TradeId`] by dropping its `OrderSeq`.
    pub struct FillId(FillSeq);
    error FillIdParseError = "invalid fill ID: expected 32-character hex string";
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
pub struct TradeId {
    order: OrderId,
    seq: FillSeq,
}

/// 16 bytes of `OrderId` + 8 bytes of `seq`, both big-endian.
const TRADE_ID_LEN: usize = 16 + 8;

impl TradeId {
    pub fn new(order: OrderId, seq: FillSeq) -> Self {
        Self { order, seq }
    }

    pub fn order_id(&self) -> OrderId {
        self.order
    }

    pub fn seq(&self) -> FillSeq {
        self.seq
    }

    /// The id of the match this trade is one side of — the owning order's book
    /// paired with the shared `FillSeq`, dropping the `OrderSeq`.
    pub fn fill_id(&self) -> FillId {
        FillId::new(self.order.book_id(), self.seq)
    }
}

impl Storable for TradeId {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; TRADE_ID_LEN];
        buf[..16].copy_from_slice(&self.order.to_bytes());
        buf[16..].copy_from_slice(&self.seq.get().to_be_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        let bytes: &[u8] = bytes.as_ref();
        assert_eq!(
            bytes.len(),
            TRADE_ID_LEN,
            "TradeId must decode from exactly {TRADE_ID_LEN} bytes"
        );
        let order = OrderId::from_bytes(Cow::Borrowed(&bytes[..16]));
        let seq = FillSeq::new(u64::from_be_bytes(
            bytes[16..].try_into().expect("8-byte slice"),
        ));
        Self { order, seq }
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: TRADE_ID_LEN as u32,
        is_fixed_size: true,
    };
}
