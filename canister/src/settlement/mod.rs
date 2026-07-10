use crate::Timestamp;
use crate::order::{
    FeeRates, Fill, FillSeq, MatchingOutput, OrderBookId, OrderId, OrderSeq, OrderStatus,
    OrderUpdate, PairToken, Price, Quantity, RemovedOrder, Side, TradeId, TradeLeg, TradeRecord,
};
use crate::state::event;
use minicbor::{Decode, Encode};
use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};

#[cfg(test)]
mod tests;

/// The full set of stable-memory state changes a matching round settles into:
/// the per-order record updates (fill deltas plus terminal status) applied in
/// one pass, and the balance operations and lean per-fill [`FillEvent`]s
/// partitioned into bounded [`SettlementBatch`]es, each enqueued as its own
/// `SettlingEvent`. Built once from a [`MatchingOutput`] during the matching
/// phase; consumed to update `order_history` and enqueue the settling events.
pub struct MatchSettlement {
    pub order_updates: BTreeMap<OrderSeq, OrderUpdate>,
    pub settling_batches: Vec<SettlementBatch>,
}

/// A bounded slice of a matching round's settlement: at most
/// `max_settlement_units_per_event` settlement units, carrying the balance
/// operations of those units together with the fills that produced them. A fill
/// and the balance operations it generated are always kept in the same batch so
/// that every order seq the fill references appears in that event's operations:
/// `record_settling_event` resolves each seq's owner, side, and price only for
/// seqs its `resolve_op_orders` cache was built from — the event's own balance
/// operations — so co-locating them is what keeps the fill's taker and maker
/// seqs resolvable from that cache (an absent seq would panic the resolution
/// `.expect(...)`).
#[derive(Default)]
pub struct SettlementBatch {
    pub balance_operations: Vec<event::BalanceOperation>,
    pub fills: Vec<FillEvent>,
}

impl SettlementBatch {
    fn add_unit(&mut self, unit: SettlementUnit) {
        self.balance_operations.extend(unit.balance_operations);
        if let Some(fill) = unit.fill {
            self.fills.push(fill);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.balance_operations.is_empty() && self.fills.is_empty()
    }
}

/// The atomic, indivisible piece of settlement work the packer counts and cuts
/// settling batches from: a fill (its `Some(fill_event())` together with its 2–3
/// balance operations) or an order killed/expired during matching — a marketable
/// or FOK order dropped as `MatchResult::Killed` on the matching walk — releasing
/// its reservation via its single `Unreserve` operation with `None`. User
/// cancellations run through a separate path and are not settled here. A unit is
/// never split across batches, which keeps every fill co-located with the
/// operations that reference its taker and maker seqs.
struct SettlementUnit {
    balance_operations: Vec<event::BalanceOperation>,
    fill: Option<FillEvent>,
}

impl MatchSettlement {
    /// Build the complete set of state changes from a matching round's
    /// [`MatchingOutput`]: for each fill, the balance operations, the per-order
    /// fill deltas, and the lean [`FillEvent`]; for each expired order, the
    /// reservation-releasing balance operation; and, overlaid onto the same
    /// `order_updates` map, the terminal status of every resting, filled, and
    /// expired order.
    ///
    /// The settling batches are bounded on the number of **settlement units** —
    /// each a fill (with its balance operations) or a removed order (with its
    /// `Unreserve` operation). A unit's per-message cost is dominated by its
    /// writes against the balance stable map, which drive the instruction cost
    /// of [`crate::state::State::record_settling_event`], so bounding the number
    /// of units per event is what bounds that cost — and, unlike a fill-based
    /// bound, it also covers the `Unreserve` operations emitted by killed and
    /// expired orders, which carry no fill yet still write to the balance map.
    pub fn from_matching(
        output: MatchingOutput,
        fee_rates: FeeRates,
        base_scale: NonZeroU64,
        max_settlement_units_per_event: NonZeroU32,
    ) -> Self {
        let MatchingOutput {
            fills,
            resting_orders,
            filled_orders,
            expired_orders,
        } = output;
        let mut order_updates = BTreeMap::new();

        let mut units: Vec<SettlementUnit> = Vec::with_capacity(fills.len() + expired_orders.len());
        for fill in fills {
            let settlement = FillSettlement::new(fill, fee_rates, base_scale);
            let mut balance_operations = Vec::with_capacity(3);
            settlement.push_balance_operations(&mut balance_operations);
            settlement.accrue_fill(&mut order_updates);
            units.push(SettlementUnit {
                balance_operations,
                fill: Some(settlement.fill_event()),
            });
        }
        for (seq, removed) in &expired_orders {
            let mut balance_operations = Vec::with_capacity(1);
            RemovedOrderSettlement::new(*seq, removed, base_scale)
                .push_balance_operations(&mut balance_operations);
            units.push(SettlementUnit {
                balance_operations,
                fill: None,
            });
        }

        let settling_batches = pack_settlement_units(units, max_settlement_units_per_event);

        for seq in &resting_orders {
            order_updates.entry(*seq).or_default().status = Some(OrderStatus::Open);
        }
        for seq in &filled_orders {
            order_updates.entry(*seq).or_default().status = Some(OrderStatus::Filled);
        }
        for seq in expired_orders.keys() {
            order_updates.entry(*seq).or_default().status = Some(OrderStatus::Expired);
        }
        Self {
            order_updates,
            settling_batches,
        }
    }
}

/// Cut a stream of [`SettlementUnit`]s into [`SettlementBatch`]es holding at
/// most `max_settlement_units_per_event` whole units each. A running batch
/// accumulates units; once it already holds the cap, it is flushed and a fresh
/// batch started before the next unit is added. A unit is atomic and counted as
/// one, so an event never exceeds the cap and a unit is never split across
/// batches, keeping every fill co-located with the operations that reference its
/// taker and maker seqs.
fn pack_settlement_units(
    units: Vec<SettlementUnit>,
    max_settlement_units_per_event: NonZeroU32,
) -> Vec<SettlementBatch> {
    let cap = max_settlement_units_per_event.get() as usize;
    let mut batches: Vec<SettlementBatch> = Vec::new();
    let mut current = SettlementBatch::default();
    let mut units_in_current = 0usize;
    for unit in units {
        if units_in_current == cap {
            batches.push(std::mem::take(&mut current));
            units_in_current = 0;
        }
        current.add_unit(unit);
        units_in_current += 1;
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

/// A single [`Fill`] together with the realized values derived from it, computed
/// once in the matching phase (the only point where both `fee_rates` and
/// `base_scale` are in scope) and used to project both the
/// [`event::BalanceOperation`]s and the per-order scalar deltas, so the two can
/// never diverge.
///
/// This is a matching-phase, heap-only helper: it is never CBOR-encoded into the
/// event log. The settling event carries the lean [`FillEvent`] instead, and the
/// settling phase recovers side/price from the order records and recomputes the
/// realized values to rebuild the two side-projected [`TradeRecord`]s.
#[derive(Debug)]
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
    /// Snapshot of the book's fee rates at match time, carried onto the lean
    /// [`FillEvent`] so settling can recompute the fees off the pinned rates.
    fee_rates: FeeRates,
}

impl FillSettlement {
    /// Compute the realized values of a single fill once.
    pub fn new(fill: Fill, fee_rates: FeeRates, base_scale: NonZeroU64) -> Self {
        let (notional, taker_fee, maker_fee) = fees(
            fill.maker_price,
            fill.quantity,
            fill.taker_side,
            fee_rates,
            base_scale,
        );
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
            fee_rates,
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

    /// The lean, normalized record persisted on the settling event: the fill's
    /// identity, quantity, and the fee-rate snapshot — everything else is
    /// recovered or recomputed in the settling phase.
    pub fn fill_event(&self) -> FillEvent {
        FillEvent {
            fill_seq: self.fill.fill_seq,
            taker_order_seq: self.fill.taker_order_seq,
            maker_order_seq: self.fill.maker_order_seq,
            quantity: self.fill.quantity,
            fee_rates: self.fee_rates,
        }
    }
}

/// The lean, normalized per-fill record carried on the settling event and the only
/// fill data persisted in the event log. It stores just what cannot be recovered
/// otherwise: the fill's identity, the matched `quantity`, and a snapshot of the
/// book's `fee_rates` at match time.
///
/// The fill's execution price (the maker price) and its taker `side` are NOT
/// stored: they are recovered in the settling phase from the two referenced order
/// records. `fee_rates` is snapshotted here rather than recovered because the rate
/// lives on the book and is mutable — it is the one fee input pinned by neither the
/// fill identity nor the orders.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct FillEvent {
    #[n(0)]
    pub fill_seq: FillSeq,
    #[n(1)]
    pub taker_order_seq: OrderSeq,
    #[n(2)]
    pub maker_order_seq: OrderSeq,
    #[n(3)]
    pub quantity: Quantity,
    #[n(4)]
    pub fee_rates: FeeRates,
}

impl FillEvent {
    /// Rebuild the two side-projected [`TradeRecord`]s — the taker leg and the
    /// maker leg — from this lean record, the side/price recovered from the order
    /// records, and the freshly recomputed `notional` and fees. Each leg is keyed
    /// by its [`TradeId`] `(OrderId, FillSeq)`, shares the match's `fill_seq`, and
    /// self-describes one order's view of the execution without ever referencing
    /// the counterparty. Consumed by [`crate::order::TradeHistory::append`].
    pub fn trade_legs(
        &self,
        book_id: OrderBookId,
        taker_side: Side,
        maker_price: Price,
        base_scale: NonZeroU64,
        timestamp: Timestamp,
    ) -> [TradeLeg; 2] {
        let (notional, taker_fee, maker_fee) = fees(
            maker_price,
            self.quantity,
            taker_side,
            self.fee_rates,
            base_scale,
        );
        let maker_side = match taker_side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        let taker_id = TradeId::new(OrderId::new(book_id, self.taker_order_seq), self.fill_seq);
        let taker_leg = TradeRecord {
            side: taker_side,
            price: maker_price,
            quantity: self.quantity,
            notional,
            fee: taker_fee,
            fee_token: fee_token(taker_side),
            is_maker: false,
            timestamp,
        };
        let maker_id = TradeId::new(OrderId::new(book_id, self.maker_order_seq), self.fill_seq);
        let maker_leg = TradeRecord {
            side: maker_side,
            price: maker_price,
            quantity: self.quantity,
            notional,
            fee: maker_fee,
            fee_token: fee_token(maker_side),
            is_maker: true,
            timestamp,
        };
        [(taker_id, taker_leg), (maker_id, maker_leg)]
    }
}

/// The fill's `(notional, taker_fee, maker_fee)`: the quote notional and the fee
/// charged to each of the fill's two orders, each in its receive token. Shared by
/// the matching-phase [`FillSettlement::new`] and the settling-phase
/// [`FillEvent::trade_legs`] recompute, so the balance ops and the persisted trade
/// legs can never diverge, and the notional (the costliest arithmetic) is computed
/// once per fill at each call site.
fn fees(
    maker_price: Price,
    quantity: Quantity,
    taker_side: Side,
    fee_rates: FeeRates,
    base_scale: NonZeroU64,
) -> (Quantity, Quantity, Quantity) {
    let (buyer_rate, seller_rate) = match taker_side {
        Side::Buy => (fee_rates.taker, fee_rates.maker),
        Side::Sell => (fee_rates.maker, fee_rates.taker),
    };
    let notional = maker_price
        .checked_mul_quantity_scaled(&quantity, base_scale)
        .expect("BUG: validation of order should prevent overflow");
    let quote_fee = seller_rate.mul_ceil(notional);
    let base_fee = buyer_rate.mul_ceil(quantity);
    let (taker_fee, maker_fee) = match taker_side {
        Side::Buy => (base_fee, quote_fee),
        Side::Sell => (quote_fee, base_fee),
    };
    (notional, taker_fee, maker_fee)
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

/// Collapse a zero-quantity fee to `None`. Returns `Some(_)` only when a
/// positive fee was charged; a zero fee maps to `None`, so callers (audit
/// log, apply path, `/metrics`) see "no fee on this fill" and "fee of zero
/// charged" as one and the same — the two are deliberately not
/// distinguished.
fn nonzero(q: Quantity) -> Option<Quantity> {
    if q.is_zero() { None } else { Some(q) }
}
