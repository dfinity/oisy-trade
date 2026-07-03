use super::{OrderBookId, OrderId, OrderSeq, Price, Quantity, Side};
use crate::ids::{CompositeId, Seq, SeqMarker};

#[derive(Debug, Clone, Copy)]
pub struct FillSeqMarker;

impl SeqMarker for FillSeqMarker {
    const NAME: &'static str = "FillSeq";
}

/// Sequence number identifying a [`Fill`] within a single order book.
pub type FillSeq = Seq<FillSeqMarker>;

/// Identity of a match in the order book.
pub type FillId = CompositeId<OrderBookId, FillSeq>;

/// Identity of a trade associated with a given order.
///
/// One fill touches the maker and the taker orders.
pub type TradeId = CompositeId<OrderId, FillSeq>;

impl TradeId {
    pub fn order_id(&self) -> OrderId {
        *self.first()
    }

    pub fn seq(&self) -> FillSeq {
        *self.second()
    }

    pub fn fill_id(&self) -> FillId {
        FillId::new(self.first().book_id(), *self.second())
    }

    pub fn first_of(order: OrderId) -> Self {
        Self::new(order, FillSeq::ZERO)
    }

    pub fn last_of(order: OrderId) -> Self {
        Self::new(order, FillSeq::new(u64::MAX))
    }
}

/// A single fill produced when an incoming order matches a resting order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fill {
    /// The per-book sequence of this match, minted by the order book.
    pub fill_seq: FillSeq,
    /// The sequence of the incoming (taker) order.
    pub taker_order_seq: OrderSeq,
    /// The side of the taker order.
    pub taker_side: Side,
    /// The limit price of the taker order.
    pub taker_price: Price,
    /// The sequence of the resting (maker) order that was matched.
    pub maker_order_seq: OrderSeq,
    /// The price at which the fill occurred (always the maker's price).
    pub maker_price: Price,
    /// The quantity filled.
    pub quantity: Quantity,
}

impl Fill {
    /// The amount of base tokens exchanged (same as quantity).
    pub fn base_amount(&self) -> &Quantity {
        &self.quantity
    }
}
