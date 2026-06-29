use super::{
    FeeRates, FillSeq, OrderSeq, OrderUpdate, PairToken, Price, Quantity, RemovedOrder, Side,
};
use crate::state::event;
use minicbor::{Decode, Encode};
use std::collections::BTreeMap;
use std::num::NonZeroU64;

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
pub struct FillSettlement {
    fill: Fill,
    /// Quote notional `maker_price × quantity / base_scale` (the executed
    /// price; a buy taker's reservation surplus is excluded).
    notional: Quantity,
    /// Fee charged to the taker order, in its receive token (base if the taker
    /// bought, quote if it sold).
    taker_fee: Quantity,
    /// Fee charged to the maker order, in its receive token.
    maker_fee: Quantity,
    /// Quote surplus released back to a buy taker that crossed below its limit;
    /// Zero for a sell taker or an exact-price fill.
    surplus: Quantity,
}

impl FillSettlement {
    /// Compute the realized values of a single fill once.
    pub fn new(fill: Fill, fee_rates: FeeRates, base_scale: NonZeroU64) -> Self {
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

/// Collapse a zero-quantity fee to `None`. Keeps `Some(_)` reserved for
/// "fee was actually charged" so callers (audit log, apply path,
/// `/metrics`) can distinguish "no fee on this fill" from "fee of zero
/// charged".
fn nonzero(q: Quantity) -> Option<Quantity> {
    if q.is_zero() { None } else { Some(q) }
}
