use super::{
    FeeRates, OrderBookId, OrderId, OrderSeq, OrderUpdate, PairToken, Price, Quantity,
    RemovedOrder, Side, TradeLeg, TradeRecord,
};
use crate::Timestamp;
use crate::ids::{CompositeId, Seq, SeqMarker};
use crate::state::event;
use crate::user::UserId;
use minicbor::{Decode, Encode};
use std::collections::BTreeMap;
use std::num::NonZeroU64;

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
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Fill {
    /// The per-book sequence of this match, minted by the order book.
    #[n(0)]
    pub fill_seq: FillSeq,
    /// The sequence of the incoming (taker) order.
    #[n(1)]
    pub taker_order_seq: OrderSeq,
    /// The side of the taker order.
    #[n(2)]
    pub taker_side: Side,
    /// The limit price of the taker order.
    #[n(3)]
    pub taker_price: Price,
    /// The sequence of the resting (maker) order that was matched.
    #[n(4)]
    pub maker_order_seq: OrderSeq,
    /// The price at which the fill occurred (always the maker's price).
    #[n(5)]
    pub maker_price: Price,
    /// The quantity filled.
    #[n(6)]
    pub quantity: Quantity,
}

impl Fill {
    /// The amount of quote tokens exchanged:
    /// `maker_price × quantity / base_scale` (`base_scale = 10^base_decimals`).
    pub fn quote_amount(&self, base_scale: NonZeroU64) -> Quantity {
        self.maker_price
            .checked_mul_quantity_scaled(&self.quantity, base_scale)
            .expect("BUG: validation of order should prevent overflow")
    }

    /// The amount of base tokens exchanged (same as quantity).
    pub fn base_amount(&self) -> &Quantity {
        &self.quantity
    }
}

/// A single [`Fill`] together with the realized values derived from it, computed
/// once in settlement (the only point where both `fee_rates` and `base_scale`
/// are in scope) and reused to build both the [`event::BalanceOperation`]s and
/// the per-order scalar deltas, so the two can never diverge.
///
/// Produced during matching (pure heap) and carried in the paired
/// [`event::SettlingEvent`]; the settling phase is where its two orders'
/// owners are resolved and stamped via [`Self::resolve_owners`] before the
/// settlement is handed to [`crate::order::TradeHistory::append`]. It is
/// therefore CBOR-encoded into the persisted settling event with both owners
/// unresolved, and re-resolved on replay.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct FillSettlement {
    #[n(0)]
    fill: Fill,
    /// Quote notional `maker_price × quantity / base_scale` (the executed
    /// price; a buy taker's reservation surplus is excluded).
    #[n(1)]
    notional: Quantity,
    /// Fee charged to the taker order, in its receive token (base if the taker
    /// bought, quote if it sold).
    #[n(2)]
    taker_fee: Quantity,
    /// Fee charged to the maker order, in its receive token.
    #[n(3)]
    maker_fee: Quantity,
    /// Quote surplus released back to a buy taker that crossed below its limit;
    /// Zero for a sell taker or an exact-price fill.
    #[n(4)]
    surplus: Quantity,
    /// The book the fill executed in.
    #[n(5)]
    book_id: OrderBookId,
    /// The match timestamp, stamped onto both projected trade records.
    #[n(6)]
    timestamp: Timestamp,
    /// Owner of the taker order, resolved during the settling phase.
    #[n(7)]
    taker_user: Option<UserId>,
    /// Owner of the maker order, resolved during the settling phase.
    #[n(8)]
    maker_user: Option<UserId>,
}

impl FillSettlement {
    /// Compute the realized values of a single fill once. Owners are left
    /// unresolved; the settling phase stamps them via [`Self::resolve_owners`].
    pub fn new(
        fill: Fill,
        fee_rates: FeeRates,
        base_scale: NonZeroU64,
        book_id: OrderBookId,
        timestamp: Timestamp,
    ) -> Self {
        // Receive-side convention: buyer pays fee in base (the asset they
        // receive), seller in quote. Each side's rate is `taker` if they
        // were the taker, else `maker`.
        let (buyer_rate, seller_rate) = match fill.taker_side {
            Side::Buy => (fee_rates.taker, fee_rates.maker),
            Side::Sell => (fee_rates.maker, fee_rates.taker),
        };
        let notional = fill.quote_amount(base_scale);
        let quote_fee = seller_rate.mul_ceil(notional);
        let base_fee = buyer_rate.mul_ceil(fill.quantity);
        // The taker pays on the side it traded: base if it bought, quote if
        // it sold. The maker pays on the opposite side.
        let (taker_fee, maker_fee) = match fill.taker_side {
            Side::Buy => (base_fee, quote_fee),
            Side::Sell => (quote_fee, base_fee),
        };
        let surplus = if fill.taker_side == Side::Buy
            && let Some(diff) = fill.taker_price.checked_sub(fill.maker_price)
            && !diff.is_zero()
        {
            diff.checked_mul_quantity_scaled(&fill.quantity, base_scale)
                .expect("BUG: price_diff * quantity overflow — validated in validate_limit_order")
        } else {
            Quantity::ZERO
        };
        Self {
            fill,
            notional,
            taker_fee,
            maker_fee,
            surplus,
            book_id,
            timestamp,
            taker_user: None,
            maker_user: None,
        }
    }

    /// Push the (up to three) balance operations a single fill settles into `ops`.
    pub fn push_balance_operations(&self, ops: &mut Vec<event::BalanceOperation>) {
        let fill = &self.fill;
        let (buyer_seq, seller_seq) = match fill.taker_side {
            Side::Buy => (fill.taker_order_seq, fill.maker_order_seq),
            Side::Sell => (fill.maker_order_seq, fill.taker_order_seq),
        };
        let (quote_fee, base_fee) = match fill.taker_side {
            Side::Buy => (self.maker_fee, self.taker_fee),
            Side::Sell => (self.taker_fee, self.maker_fee),
        };
        ops.push(event::BalanceOperation::Transfer {
            from_order: buyer_seq,
            to_order: seller_seq,
            token: PairToken::Quote,
            amount: self.notional,
            fee: nonzero(quote_fee),
        });
        if !self.surplus.is_zero() {
            ops.push(event::BalanceOperation::Unreserve {
                order: fill.taker_order_seq,
                token: PairToken::Quote,
                amount: self.surplus,
            });
        }
        ops.push(event::BalanceOperation::Transfer {
            from_order: seller_seq,
            to_order: buyer_seq,
            token: PairToken::Base,
            amount: fill.quantity,
            fee: nonzero(base_fee),
        });
    }

    /// Update maker and taker orders based on this fill.
    pub fn accrue_fill(&self, updates: &mut BTreeMap<OrderSeq, OrderUpdate>) {
        for (order_seq, fee) in [
            (self.fill.maker_order_seq, self.maker_fee),
            (self.fill.taker_order_seq, self.taker_fee),
        ] {
            let update = updates.entry(order_seq).or_default();
            update.filled_delta = update
                .filled_delta
                .checked_add(self.fill.quantity)
                .expect("BUG: filled_delta overflow");
            update.quote_delta = update
                .quote_delta
                .checked_add(self.notional)
                .expect("BUG: quote_delta overflow");
            update.fee_delta = update
                .fee_delta
                .checked_add(fee)
                .expect("BUG: fee_delta overflow");
        }
    }

    /// The taker order's per-book sequence.
    pub fn taker_order_seq(&self) -> OrderSeq {
        self.fill.taker_order_seq
    }

    /// The maker order's per-book sequence.
    pub fn maker_order_seq(&self) -> OrderSeq {
        self.fill.maker_order_seq
    }

    /// Stamp the resolved owners of the two orders onto the settlement. Called
    /// in the settling phase after the single `OrderSeq -> UserId` resolution,
    /// before the settlement is handed to [`crate::order::TradeHistory::append`].
    pub fn resolve_owners(&mut self, taker_user: UserId, maker_user: UserId) {
        self.taker_user = Some(taker_user);
        self.maker_user = Some(maker_user);
    }

    /// Build the two side-projected [`TradeRecord`]s — the taker leg and the maker
    /// leg — from this fill's single computed settlement, each keyed by its
    /// [`TradeId`] `(OrderId, FillSeq)` and stamped with the match `timestamp`.
    /// The two legs share the match's `fill_seq`; each record self-describes one
    /// order's view of the execution and never references the counterparty.
    /// Consumed by [`crate::order::TradeHistory::append`], which pairs each leg
    /// with its owner stamped via [`Self::resolve_owners`].
    pub(crate) fn trade_legs(self) -> [(TradeLeg, UserId); 2] {
        let fill = &self.fill;
        let book_id = self.book_id;
        let timestamp = self.timestamp;
        let taker_user = self
            .taker_user
            .expect("BUG: taker owner not resolved before trade projection");
        let maker_user = self
            .maker_user
            .expect("BUG: maker owner not resolved before trade projection");
        let taker_side = fill.taker_side;
        let maker_side = match taker_side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        let taker_id = TradeId::new(OrderId::new(book_id, fill.taker_order_seq), fill.fill_seq);
        let taker_leg = TradeRecord {
            side: taker_side,
            price: fill.maker_price,
            quantity: fill.quantity,
            notional: self.notional,
            fee: self.taker_fee,
            fee_token: fee_token(taker_side),
            is_maker: false,
            timestamp,
        };
        let maker_id = TradeId::new(OrderId::new(book_id, fill.maker_order_seq), fill.fill_seq);
        let maker_leg = TradeRecord {
            side: maker_side,
            price: fill.maker_price,
            quantity: fill.quantity,
            notional: self.notional,
            fee: self.maker_fee,
            fee_token: fee_token(maker_side),
            is_maker: true,
            timestamp,
        };
        [
            ((taker_id, taker_leg), taker_user),
            ((maker_id, maker_leg), maker_user),
        ]
    }
}

/// The settlement of a removed order (canceled or killed): the placement
/// reservation released back to its owner, computed where `base_scale` is in
/// scope so the matcher stays scale-agnostic.
pub struct RemovedOrderSettlement {
    order_seq: OrderSeq,
    token: PairToken,
    amount: Quantity,
}

impl RemovedOrderSettlement {
    /// Compute the reservation released by removing an order.
    pub fn new(order_seq: OrderSeq, removed: &RemovedOrder, base_scale: NonZeroU64) -> Self {
        let (token, amount) = match removed.side {
            Side::Buy => (
                PairToken::Quote,
                removed
                    .price
                    .checked_mul_quantity_scaled(&removed.remaining_quantity, base_scale)
                    .expect("BUG: price * remaining overflow — validated at placement"),
            ),
            Side::Sell => (PairToken::Base, removed.remaining_quantity),
        };
        Self {
            order_seq,
            token,
            amount,
        }
    }

    /// Push the single unreserve operation that releases the reservation.
    pub fn push_balance_operations(&self, ops: &mut Vec<event::BalanceOperation>) {
        ops.push(event::BalanceOperation::Unreserve {
            order: self.order_seq,
            token: self.token,
            amount: self.amount,
        });
    }
}

/// The token a fill's fee is charged in, per the receive-side convention: a
/// buyer is charged in the base token it receives, a seller in quote.
fn fee_token(side: Side) -> PairToken {
    match side {
        Side::Buy => PairToken::Base,
        Side::Sell => PairToken::Quote,
    }
}

/// Collapse a zero-quantity fee to `None`. Keeps `Some(_)` reserved for
/// "fee was actually charged" so callers (audit log, apply path,
/// `/metrics`) can distinguish "no fee on this fill" from "fee of zero
/// charged".
fn nonzero(q: Quantity) -> Option<Quantity> {
    if q.is_zero() { None } else { Some(q) }
}
