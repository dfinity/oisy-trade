use super::plan::{FillPlan, PlannedFill};
use super::{
    FeeRates, LotSize, Order, OrderBookId, OrderSeq, Price, Quantity, RestingOrder, Side, TickSize,
};
use crate::order::iter::OrderIter;
use minicbor::{Decode, Encode};
use std::cmp::Reverse;
use std::collections::btree_map;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::num::NonZeroU64;

/// Central limit order book for a single trading pair.
///
/// Bids are sorted by price descending (best bid = highest price).
/// Asks are sorted by price ascending (best ask = lowest price).
/// Within a price level, orders are matched in FIFO order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderBook {
    /// Immutable identifier assigned at registration time.
    id: OrderBookId,
    /// Per-book sequence counter for generating order IDs.
    next_seq: OrderSeq,
    /// Minimum price increment. All order prices must be a multiple of this value.
    tick_size: TickSize,
    /// Minimum order quantity. All order quantities must be a multiple of this value.
    lot_size: LotSize,
    /// Minimum order notional (quote smallest units). Enforced at the state layer.
    min_notional: Quantity,
    /// Maximum order notional (quote smallest units), if any. Enforced at the state layer.
    max_notional: Option<Quantity>,
    /// Maker/taker fee rates applied at fill-time.
    fee_rates: FeeRates,
    /// Orders awaiting matching, processed by the timer.
    pending_orders: VecDeque<Order>,
    /// Buy side, sorted by price descending (highest first) via [`Reverse<Price>`].
    bids: BTreeMap<Reverse<Price>, VecDeque<RestingOrder>>,
    /// Sell side, sorted by price ascending (lowest first).
    asks: BTreeMap<Price, VecDeque<RestingOrder>>,
    /// Index mapping order sequences to their location (side, price) for O(log n) lookup.
    resting_orders: BTreeMap<OrderSeq, (Side, Price)>,
    /// Sequences of orders that were fully filled since the last drain.
    filled_orders: BTreeSet<OrderSeq>,
}

impl OrderBook {
    /// Creates a new empty order book with the given constraints.
    pub fn new(
        id: OrderBookId,
        tick_size: TickSize,
        lot_size: LotSize,
        min_notional: Quantity,
        max_notional: Option<Quantity>,
        fee_rates: FeeRates,
    ) -> Self {
        Self {
            id,
            next_seq: OrderSeq::default(),
            tick_size,
            lot_size,
            min_notional,
            max_notional,
            fee_rates,
            pending_orders: VecDeque::new(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            resting_orders: BTreeMap::new(),
            filled_orders: BTreeSet::new(),
        }
    }

    pub fn id(&self) -> OrderBookId {
        self.id
    }

    pub fn next_seq(&self) -> OrderSeq {
        self.next_seq
    }

    pub fn is_empty(&self) -> bool {
        assert_eq!(
            self.bids.is_empty() && self.asks.is_empty(),
            self.resting_orders.is_empty(),
            "BUG: orders should be empty iff both bids and asks are empty"
        );
        self.resting_orders.is_empty() && self.pending_orders.is_empty()
    }

    pub fn tick_size(&self) -> TickSize {
        self.tick_size
    }

    pub fn lot_size(&self) -> LotSize {
        self.lot_size
    }

    pub fn min_notional(&self) -> Quantity {
        self.min_notional
    }

    pub fn max_notional(&self) -> Option<Quantity> {
        self.max_notional
    }

    pub fn fee_rates(&self) -> FeeRates {
        self.fee_rates
    }

    pub fn bids_iter(&self) -> OrderIter<'_, Reverse<Price>, RestingOrder> {
        OrderIter::new(&self.bids)
    }

    pub fn asks_iter(&self) -> OrderIter<'_, Price, RestingOrder> {
        OrderIter::new(&self.asks)
    }

    /// Returns the best (highest price) bid order, or `None` if the bid side is empty.
    pub fn best_bid(&self) -> Option<Order> {
        self.bids_iter()
            .next()
            .map(|(&Reverse(price), resting)| resting.to_order(Side::Buy, price))
    }

    /// Returns the best (lowest price) ask order, or `None` if the ask side is empty.
    pub fn best_ask(&self) -> Option<Order> {
        self.asks_iter()
            .next()
            .map(|(&price, resting)| resting.to_order(Side::Sell, price))
    }

    /// Match an incoming order against the book.
    ///
    /// Validates tick size, lot size, and rejects zero price/quantity, then attempts
    /// to fill the order against the opposite side. Returns:
    /// - [`MatchResult::Filled`] if the order is fully filled.
    /// - [`MatchResult::PartiallyFilled`] if partially filled with the remainder resting.
    /// - [`MatchResult::Resting`] if no match was found and the order rests as-is.
    pub(crate) fn match_order(&mut self, mut order: Order) -> Result<MatchResult, MatchOrderError> {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("book::match_order");
        self.validate_order(order.price(), order.remaining_quantity())?;

        let plan = self.plan_fills(&order);
        let mut fills = Vec::new();
        self.apply_plan(order.side(), &plan, &mut order, &mut fills);

        if order.remaining_quantity().is_zero() {
            self.filled_orders.insert(order.id());
            Ok(MatchResult::Filled { fills })
        } else {
            let resting_order_seq = order.id();
            self.insert_order(order);
            if fills.is_empty() {
                Ok(MatchResult::Resting { resting_order_seq })
            } else {
                Ok(MatchResult::PartiallyFilled {
                    fills,
                    resting_order_seq,
                })
            }
        }
    }

    /// Read-only walk of the crossing price levels, recording the fills the
    /// incoming `order` *would* make.
    ///
    /// Iterates best-first (asks ascending while `ask_price <= order.price()`;
    /// bids descending while `bid_price >= order.price()`) and, FIFO within
    /// each level, records one [`PlannedFill`] per maker it would touch,
    /// accumulating until the order is satisfied. Mutates no book state.
    /// Allocation-free until the first crossing maker is pushed.
    pub(crate) fn plan_fills(&self, order: &Order) -> FillPlan {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("book::plan_fills");
        let price = order.price();
        let mut fills = Vec::new();
        let mut remaining = *order.remaining_quantity();
        match order.side() {
            Side::Buy => {
                for (&ask_price, queue) in &self.asks {
                    if ask_price > price || remaining.is_zero() {
                        break;
                    }
                    Self::plan_level(ask_price, queue, &mut remaining, &mut fills);
                }
            }
            Side::Sell => {
                for (&Reverse(bid_price), queue) in &self.bids {
                    if bid_price < price || remaining.is_zero() {
                        break;
                    }
                    Self::plan_level(bid_price, queue, &mut remaining, &mut fills);
                }
            }
        }

        FillPlan {
            fills,
            fully_filled: remaining.is_zero(),
        }
    }

    /// Record FIFO fills against a single crossing price level, decrementing
    /// `remaining` as it goes.
    fn plan_level(
        maker_price: Price,
        queue: &VecDeque<RestingOrder>,
        remaining: &mut Quantity,
        fills: &mut Vec<PlannedFill>,
    ) {
        for resting in queue {
            if remaining.is_zero() {
                break;
            }
            let fill_qty = *std::cmp::min(&*remaining, resting.remaining_quantity());
            *remaining = remaining
                .checked_sub(fill_qty)
                .expect("BUG: fill_qty exceeds remaining");
            let maker_emptied = fill_qty == *resting.remaining_quantity();
            fills.push(PlannedFill {
                maker_seq: resting.id(),
                maker_price,
                fill_qty,
                maker_emptied,
            });
        }
    }

    /// Replay a [`FillPlan`] against the book, performing the mutations.
    ///
    /// For each [`PlannedFill`]: reduce the maker (the level cursor is held
    /// across consecutive fills at the same level and only re-acquired when
    /// `maker_price` changes), reduce the taker, push the [`Fill`], and on
    /// `maker_emptied` pop the maker, drop its `resting_orders` index entry,
    /// insert into `filled_orders`, and remove the level if its queue empties.
    fn apply_plan(
        &mut self,
        side: Side,
        plan: &FillPlan,
        order: &mut Order,
        fills: &mut Vec<Fill>,
    ) {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("book::apply_plan");
        let mut cursor: Option<(Price, &mut VecDeque<RestingOrder>)> = None;

        for planned in &plan.fills {
            // Re-acquire the level only when the price changes; otherwise hold
            // the cursor across consecutive fills at the same level.
            if cursor
                .as_ref()
                .is_none_or(|(cursor_price, _)| *cursor_price != planned.maker_price)
            {
                let queue = match side {
                    Side::Buy => self
                        .asks
                        .get_mut(&planned.maker_price)
                        .expect("BUG: planned ask level missing"),
                    Side::Sell => self
                        .bids
                        .get_mut(&Reverse(planned.maker_price))
                        .expect("BUG: planned bid level missing"),
                };
                cursor = Some((planned.maker_price, queue));
            }
            let queue = &mut cursor.as_mut().expect("cursor set above").1;

            let resting = queue.front_mut().expect("BUG: planned maker level empty");
            assert_eq!(
                resting.id(),
                planned.maker_seq,
                "BUG: plan/apply divergence — maker at level front does not match plan"
            );

            assert!(
                planned.fill_qty <= *order.remaining_quantity(),
                "BUG: plan/apply divergence — planned fill exceeds taker remaining quantity"
            );
            assert!(
                planned.fill_qty <= *resting.remaining_quantity(),
                "BUG: plan/apply divergence — planned fill exceeds maker remaining quantity"
            );
            order.reduce_quantity(&planned.fill_qty);
            resting.reduce_quantity(&planned.fill_qty);

            fills.push(Fill {
                taker_order_seq: order.id(),
                taker_side: order.side(),
                taker_price: order.price(),
                maker_order_seq: planned.maker_seq,
                maker_price: planned.maker_price,
                quantity: planned.fill_qty,
            });

            if planned.maker_emptied {
                assert!(
                    resting.remaining_quantity().is_zero(),
                    "BUG: plan/apply divergence — maker marked emptied but has remaining quantity"
                );
                let filled = queue.pop_front().expect("front exists");
                self.resting_orders.remove(&filled.id()).expect(
                    "BUG: plan/apply divergence — emptied maker missing from resting_orders index",
                );
                self.filled_orders.insert(filled.id());
                if queue.is_empty() {
                    cursor = None;
                    match side {
                        Side::Buy => {
                            self.asks.remove(&planned.maker_price);
                        }
                        Side::Sell => {
                            self.bids.remove(&Reverse(planned.maker_price));
                        }
                    }
                }
            }
        }
    }

    pub fn validate_order(&self, price: Price, quantity: &Quantity) -> Result<(), MatchOrderError> {
        if price.is_zero() || !price.is_multiple_of(self.tick_size) {
            return Err(MatchOrderError::InvalidTickSize {
                price,
                tick_size: self.tick_size,
            });
        }
        if quantity.is_zero() || !quantity.is_multiple_of(self.lot_size) {
            return Err(MatchOrderError::InvalidLotSize {
                quantity: *quantity,
                lot_size: self.lot_size,
            });
        }
        Ok(())
    }

    /// Check that `notional` (the scaled `price × quantity`, in quote smallest
    /// units) lies within the book's `[min_notional, max_notional]` bounds.
    pub fn check_notional(&self, notional: &Quantity) -> Result<(), NotionalError> {
        if *notional < self.min_notional || self.max_notional.is_some_and(|max| *notional > max) {
            return Err(NotionalError {
                notional: *notional,
                min: self.min_notional,
                max: self.max_notional,
            });
        }
        Ok(())
    }

    /// Enqueue an order for matching.
    pub(crate) fn add_pending_order(&mut self, order: Order) {
        assert!(
            self.validate_order(order.price, order.remaining_quantity())
                .is_ok(),
            "BUG: order is invalid"
        );
        assert_eq!(order.id(), self.next_seq, "BUG: order seq mismatch");
        self.pending_orders.push_back(order);
        self.next_seq.increment();
    }

    /// Match exactly the given pending-order sequences, in order, against
    /// the book.
    pub(crate) fn process_pending_orders(&mut self, expected_seqs: &[OrderSeq]) -> MatchingOutput {
        let mut all_fills = Vec::new();
        let mut resting_order_seqs = BTreeSet::new();
        for expected_seq in expected_seqs {
            let order = self
                .pending_orders
                .pop_front()
                .expect("BUG: fewer pending orders than expected sequences");
            assert_eq!(
                order.id(),
                *expected_seq,
                "BUG: pending order seq mismatch at the head of the queue"
            );
            match self.match_order(order) {
                Ok(result) => {
                    if let Some(resting_order_seq) = result.resting_order_seq() {
                        resting_order_seqs.insert(resting_order_seq);
                    }
                    for fill in result.into_fills() {
                        debug_assert!(!all_fills.contains(&fill), "BUG: duplicate fill {fill:?}");
                        all_fills.push(fill);
                    }
                }
                Err(err) => {
                    panic!(
                        "BUG: failed to match order: {:?}. Order was validated when inserted; \
                        and, although matching happens asynchronously afterwards, \
                        the tick/lot size are not mutable",
                        err
                    );
                }
            }
        }
        // An order may rest and then get fully filled within the same batch.
        // Remove those from resting so each order appears in exactly one set.
        let resting_orders = &resting_order_seqs - &self.filled_orders;
        debug_assert!(
            resting_orders.is_disjoint(&self.filled_orders),
            "BUG: resting and filled sets overlap"
        );
        let filled_orders = std::mem::take(&mut self.filled_orders);
        MatchingOutput {
            fills: all_fills,
            resting_orders,
            filled_orders,
        }
    }

    fn insert_order(&mut self, order: Order) {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("book::insert_order");
        let side = order.side();
        let price = order.price();
        assert_eq!(self.resting_orders.insert(order.id(), (side, price)), None);
        let resting = RestingOrder::from(order);
        match side {
            Side::Buy => self
                .bids
                .entry(Reverse(price))
                .or_default()
                .push_back(resting),
            Side::Sell => self.asks.entry(price).or_default().push_back(resting),
        }
    }

    /// Remove the order with the given sequence from the book.
    ///
    /// Looks in the resting book first (O(log(num_resting_orders))) and
    /// then in the pending orders (O(num_pending_orders)).
    pub(crate) fn remove_order(&mut self, seq: OrderSeq) -> Option<RemovedOrder> {
        if let Some((side, price)) = self.resting_orders.remove(&seq) {
            let remaining_quantity = match side {
                Side::Buy => remove_from_level(self.bids.entry(Reverse(price)), seq),
                Side::Sell => remove_from_level(self.asks.entry(price), seq),
            }
            .expect("BUG: resting_orders index inconsistent with bids/asks");
            return Some(RemovedOrder {
                side,
                price,
                remaining_quantity,
            });
        }
        let pos = self.pending_orders.iter().position(|o| o.id() == seq)?;
        let order = self.pending_orders.remove(pos).expect("position is valid");
        Some(RemovedOrder {
            side: order.side(),
            price: order.price(),
            remaining_quantity: *order.remaining_quantity(),
        })
    }

    pub fn pending_orders_len(&self) -> usize {
        self.pending_orders.len()
    }

    /// FIFO sequence numbers of the orders currently waiting to be matched.
    pub fn pending_order_seqs(&self) -> impl Iterator<Item = OrderSeq> + '_ {
        self.pending_orders.iter().map(|order| order.id())
    }

    pub fn bids_len(&self) -> usize {
        self.bids.len()
    }

    pub fn asks_len(&self) -> usize {
        self.asks.len()
    }

    pub fn resting_orders_len(&self) -> usize {
        self.resting_orders.len()
    }

    /// Iterate over bid price levels (highest price first), up to `limit` levels.
    /// Each level aggregates the remaining quantities of all resting orders at that price.
    pub fn bid_levels(&self, limit: usize) -> impl Iterator<Item = (Price, Quantity)> + '_ {
        self.bids
            .iter()
            .take(limit)
            .map(|(Reverse(price), queue)| (*price, sum_remaining(queue)))
    }

    /// Iterate over ask price levels (lowest price first), up to `limit` levels.
    /// Each level aggregates the remaining quantities of all resting orders at that price.
    pub fn ask_levels(&self, limit: usize) -> impl Iterator<Item = (Price, Quantity)> + '_ {
        self.asks
            .iter()
            .take(limit)
            .map(|(price, queue)| (*price, sum_remaining(queue)))
    }
}

/// Sum the remaining quantities of every resting order at a price level.
/// Saturates to [`Quantity::MAX`] on overflow so a query can never trap.
/// Overflow is practically unreachable — it would require aggregating
/// resting orders whose combined size exceeds 2^256 - 1.
fn sum_remaining(queue: &VecDeque<RestingOrder>) -> Quantity {
    queue.iter().fold(Quantity::ZERO, |acc, order| {
        acc.checked_add(*order.remaining_quantity())
            .unwrap_or(Quantity::MAX)
    })
}

/// An order removed from the book via [`OrderBook::remove_order`].
///
/// Carries enough information for the caller to refund the reserved balance:
/// `side` determines the token (quote for Buy, base for Sell) and
/// `price × remaining_quantity` (or just `remaining_quantity` for Sell) is
/// the amount to unreserve.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "RemovedOrder must be applied to order_history via `record_cancel_limit_order`; \
              dropping it leaves the order book and order_history out of sync"]
pub struct RemovedOrder {
    pub side: Side,
    pub price: Price,
    pub remaining_quantity: Quantity,
}

fn remove_from_level<K: Ord>(
    entry: btree_map::Entry<'_, K, VecDeque<RestingOrder>>,
    seq: OrderSeq,
) -> Option<Quantity> {
    let btree_map::Entry::Occupied(mut occupied) = entry else {
        return None;
    };
    let queue = occupied.get_mut();
    let pos = queue.iter().position(|o| o.id() == seq)?;
    let removed = queue.remove(pos).expect("position is valid");
    if queue.is_empty() {
        occupied.remove();
    }
    Some(*removed.remaining_quantity())
}

/// Output of [`OrderBook::process_pending_orders`]: the fills produced,
/// orders that began resting in the book, and orders that were fully filled.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[must_use = "MatchingOutput must be applied to order_history via `record_matching_event`; \
              dropping it leaves the order book and order_history out of sync"]
pub struct MatchingOutput {
    /// Fills executed during this matching round, in execution order.
    #[n(0)]
    pub fills: Vec<Fill>,
    /// Orders that were not fully filled and are now resting in the book.
    #[n(1)]
    pub resting_orders: BTreeSet<OrderSeq>,
    /// Orders that were fully filled and removed from the book.
    #[n(2)]
    pub filled_orders: BTreeSet<OrderSeq>,
}

/// The result of matching an incoming order against the book.
#[derive(Debug, PartialEq, Eq)]
pub enum MatchResult {
    /// The order was fully filled and does not rest in the book.
    Filled { fills: Vec<Fill> },
    /// The order was partially filled and the remainder is now resting in the book.
    PartiallyFilled {
        fills: Vec<Fill>,
        resting_order_seq: OrderSeq,
    },
    /// No match was found; the order is resting in the book.
    Resting { resting_order_seq: OrderSeq },
}

impl MatchResult {
    pub fn fills(&self) -> &[Fill] {
        match self {
            MatchResult::Filled { fills } | MatchResult::PartiallyFilled { fills, .. } => fills,
            MatchResult::Resting { .. } => &[],
        }
    }

    pub fn resting_order_seq(&self) -> Option<OrderSeq> {
        match self {
            MatchResult::PartiallyFilled {
                resting_order_seq, ..
            }
            | MatchResult::Resting { resting_order_seq } => Some(*resting_order_seq),
            MatchResult::Filled { .. } => None,
        }
    }

    pub fn into_fills(self) -> Vec<Fill> {
        match self {
            MatchResult::Filled { fills } | MatchResult::PartiallyFilled { fills, .. } => fills,
            MatchResult::Resting { .. } => Vec::new(),
        }
    }
}

/// A single fill produced when an incoming order matches a resting order.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Fill {
    /// The sequence of the incoming (taker) order.
    #[n(0)]
    pub taker_order_seq: OrderSeq,
    /// The side of the taker order.
    #[n(1)]
    pub taker_side: Side,
    /// The limit price of the taker order.
    #[n(2)]
    pub taker_price: Price,
    /// The sequence of the resting (maker) order that was matched.
    #[n(3)]
    pub maker_order_seq: OrderSeq,
    /// The price at which the fill occurred (always the maker's price).
    #[n(4)]
    pub maker_price: Price,
    /// The quantity filled.
    #[n(5)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchOrderError {
    /// Price is not a positive multiple of the tick size.
    InvalidTickSize { price: Price, tick_size: TickSize },
    /// Quantity is not a positive multiple of the lot size.
    InvalidLotSize {
        quantity: Quantity,
        lot_size: LotSize,
    },
}

/// The order notional lies outside the book's `[min_notional, max_notional]` bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotionalError {
    pub notional: Quantity,
    pub min: Quantity,
    pub max: Option<Quantity>,
}

/// CBOR-encoded view of [`OrderBook`] used for pre/post-upgrade persistence.
/// The derived `resting_orders` index is intentionally omitted and rebuilt
/// from `bids` + `asks` in `From<OrderBookSnapshot> for OrderBook`.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct OrderBookSnapshot {
    #[n(0)]
    pub id: OrderBookId,
    #[n(1)]
    pub next_seq: OrderSeq,
    #[n(2)]
    pub tick_size: TickSize,
    #[n(3)]
    pub lot_size: LotSize,
    #[n(4)]
    pub pending_orders: Vec<Order>,
    /// Bid side, stored with the natural (un-reversed) price. Converted back
    /// to a `BTreeMap<Reverse<Price>, …>` on restore.
    #[n(5)]
    pub bids: Vec<PriceLevel>,
    #[n(6)]
    pub asks: Vec<PriceLevel>,
    #[n(7)]
    pub filled_orders: Vec<OrderSeq>,
    #[n(8)]
    pub fee_rates: FeeRates,
    #[n(9)]
    pub min_notional: Quantity,
    #[n(10)]
    pub max_notional: Option<Quantity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct PriceLevel {
    #[n(0)]
    pub price: Price,
    #[n(1)]
    pub orders: Vec<RestingOrder>,
}

impl From<&OrderBook> for OrderBookSnapshot {
    fn from(book: &OrderBook) -> Self {
        Self {
            id: book.id,
            next_seq: book.next_seq,
            tick_size: book.tick_size,
            lot_size: book.lot_size,
            pending_orders: book.pending_orders.iter().cloned().collect(),
            bids: book
                .bids
                .iter()
                .map(|(Reverse(price), orders)| PriceLevel {
                    price: *price,
                    orders: orders.iter().cloned().collect(),
                })
                .collect(),
            asks: book
                .asks
                .iter()
                .map(|(price, orders)| PriceLevel {
                    price: *price,
                    orders: orders.iter().cloned().collect(),
                })
                .collect(),
            filled_orders: book.filled_orders.iter().copied().collect(),
            fee_rates: book.fee_rates,
            min_notional: book.min_notional,
            max_notional: book.max_notional,
        }
    }
}

impl From<OrderBookSnapshot> for OrderBook {
    fn from(snapshot: OrderBookSnapshot) -> Self {
        let pending_orders: VecDeque<Order> = snapshot.pending_orders.into_iter().collect();
        let mut bids: BTreeMap<Reverse<Price>, VecDeque<RestingOrder>> = BTreeMap::new();
        let mut asks: BTreeMap<Price, VecDeque<RestingOrder>> = BTreeMap::new();
        let mut resting_orders: BTreeMap<OrderSeq, (Side, Price)> = BTreeMap::new();

        for level in snapshot.bids {
            let PriceLevel { price, orders } = level;
            for order in &orders {
                assert!(
                    resting_orders
                        .insert(order.id(), (Side::Buy, price))
                        .is_none(),
                    "invalid order book snapshot: duplicate resting order sequence {:?}",
                    order.id()
                );
            }
            assert!(
                bids.insert(Reverse(price), VecDeque::from(orders))
                    .is_none(),
                "invalid order book snapshot: duplicate bid price level {:?}",
                price
            );
        }
        for level in snapshot.asks {
            let PriceLevel { price, orders } = level;
            for order in &orders {
                assert!(
                    resting_orders
                        .insert(order.id(), (Side::Sell, price))
                        .is_none(),
                    "invalid order book snapshot: duplicate resting order sequence {:?}",
                    order.id()
                );
            }
            assert!(
                asks.insert(price, VecDeque::from(orders)).is_none(),
                "invalid order book snapshot: duplicate ask price level {:?}",
                price
            );
        }

        let filled_orders = snapshot.filled_orders.into_iter().collect();
        Self {
            id: snapshot.id,
            next_seq: snapshot.next_seq,
            tick_size: snapshot.tick_size,
            lot_size: snapshot.lot_size,
            min_notional: snapshot.min_notional,
            max_notional: snapshot.max_notional,
            fee_rates: snapshot.fee_rates,
            pending_orders,
            bids,
            asks,
            resting_orders,
            filled_orders,
        }
    }
}
