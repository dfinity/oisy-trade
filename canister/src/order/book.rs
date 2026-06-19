use super::plan::FillPlan;
use super::queue::{OrderQueue, OrderQueueIter};
use super::{
    FeeRates, LotSize, Order, OrderBookId, OrderSeq, Price, Quantity, RestingOrder, Side, TickSize,
};
use minicbor::{Decode, Encode};
use std::cmp::Reverse;
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
    bids: OrderQueue<Reverse<Price>, RestingOrder>,
    /// Sell side, sorted by price ascending (lowest first).
    asks: OrderQueue<Price, RestingOrder>,
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
            bids: OrderQueue::new(),
            asks: OrderQueue::new(),
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

    fn bids_iter(&self) -> OrderQueueIter<'_, Reverse<Price>, RestingOrder> {
        self.bids.iter()
    }

    fn asks_iter(&self) -> OrderQueueIter<'_, Price, RestingOrder> {
        self.asks.iter()
    }

    pub(crate) fn asks_pop_front(&mut self) -> Option<(Price, RestingOrder)> {
        let (price, resting) = self.asks.pop_front()?;
        self.resting_orders
            .remove(&resting.id())
            .expect("BUG: popped order missing from resting_orders index");
        Some((price, resting))
    }

    pub(crate) fn bids_pop_front(&mut self) -> Option<(Price, RestingOrder)> {
        let (Reverse(price), resting) = self.bids.pop_front()?;
        self.resting_orders
            .remove(&resting.id())
            .expect("BUG: popped order missing from resting_orders index");
        Some((price, resting))
    }

    pub(crate) fn asks_front_mut(&mut self) -> Option<(Price, &mut RestingOrder)> {
        self.asks.front_mut()
    }

    pub(crate) fn bids_front_mut(&mut self) -> Option<(Price, &mut RestingOrder)> {
        let (Reverse(price), resting) = self.bids.front_mut()?;
        Some((price, resting))
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
        let fills = self.apply_plan(plan, &mut order);

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

    /// Read-only walk of the crossing makers, building a [`FillPlan`] of the
    /// fills the incoming `order` would make.
    pub(crate) fn plan_fills(&self, order: &Order) -> FillPlan {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("book::plan_fills");
        let taker_price = order.price();
        let mut plan = FillPlan::new(order);

        match order.side() {
            Side::Buy => {
                for (&maker_price, resting) in self.asks_iter() {
                    if maker_price > taker_price || plan.taker_remaining().is_zero() {
                        break;
                    }
                    plan.add_fill(resting);
                }
            }
            Side::Sell => {
                for (&Reverse(maker_price), resting) in self.bids_iter() {
                    if maker_price < taker_price || plan.taker_remaining().is_zero() {
                        break;
                    }
                    plan.add_fill(resting);
                }
            }
        }

        plan
    }

    /// Replay a [`FillPlan`] against the book, performing the mutations.
    ///
    /// Fully consumes each maker listed in `plan.filled_makers()` from the
    /// front of its level, then partially fills the maker named in
    /// `plan.last_partial()` (if any). Asserts the taker's remaining quantity
    /// matches `plan.taker_remaining()` at the end.
    fn apply_plan(&mut self, plan: FillPlan, taker_order: &mut Order) -> Vec<Fill> {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("book::apply_plan");
        assert_eq!(
            taker_order.id(),
            plan.taker_order(),
            "BUG: plan/apply divergence — taker order mismatch"
        );
        let side = taker_order.side();
        let taker_price = taker_order.price();
        let mut fills = Vec::new();

        for &maker_seq in plan.filled_makers() {
            let (maker_price, filled) = match side {
                Side::Buy => self.asks_pop_front(),
                Side::Sell => self.bids_pop_front(),
            }
            .expect("BUG: plan/apply divergence — no maker level to pop");
            assert_eq!(
                filled.id(),
                maker_seq,
                "BUG: plan/apply divergence — maker at level front does not match plan"
            );
            let full_qty = *filled.remaining_quantity();
            taker_order.reduce_quantity(&full_qty);
            fills.push(Fill {
                taker_order_seq: taker_order.id(),
                taker_side: side,
                taker_price,
                maker_order_seq: maker_seq,
                maker_price,
                quantity: full_qty,
            });
            self.filled_orders.insert(filled.id());
        }

        if let Some((maker_seq, fill_qty)) = plan.last_partial() {
            let (maker_price, resting) = match side {
                Side::Buy => self.asks_front_mut(),
                Side::Sell => self.bids_front_mut(),
            }
            .expect("BUG: plan/apply divergence — no maker level for partial fill");
            assert_eq!(
                resting.id(),
                maker_seq,
                "BUG: plan/apply divergence — partial-fill maker not at level front"
            );
            resting.reduce_quantity(&fill_qty);
            assert_ne!(
                *resting.remaining_quantity(),
                Quantity::ZERO,
                "BUG: plan/apply divergence - partial-fill maker cannot be fully filled"
            );
            taker_order.reduce_quantity(&fill_qty);
            fills.push(Fill {
                taker_order_seq: taker_order.id(),
                taker_side: side,
                taker_price,
                maker_order_seq: maker_seq,
                maker_price,
                quantity: fill_qty,
            });
        }

        assert_eq!(
            *taker_order.remaining_quantity(),
            plan.taker_remaining(),
            "BUG: plan/apply divergence — taker remaining mismatch"
        );
        fills
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
            Side::Buy => self.bids.push_back(Reverse(price), resting),
            Side::Sell => self.asks.push_back(price, resting),
        }
    }

    /// Remove the order with the given sequence from the book.
    ///
    /// Looks in the resting book first (O(log(num_resting_orders))) and
    /// then in the pending orders (O(num_pending_orders)).
    pub(crate) fn remove_order(&mut self, seq: OrderSeq) -> Option<RemovedOrder> {
        if let Some((side, price)) = self.resting_orders.remove(&seq) {
            let removed = match side {
                Side::Buy => self.bids.remove(Reverse(price), |o| o.id() == seq),
                Side::Sell => self.asks.remove(price, |o| o.id() == seq),
            }
            .expect("BUG: resting_orders index inconsistent with bids/asks");
            return Some(RemovedOrder {
                side,
                price,
                remaining_quantity: *removed.remaining_quantity(),
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
            .levels()
            .take(limit)
            .map(|(Reverse(price), queue)| (*price, sum_remaining(queue)))
    }

    /// Iterate over ask price levels (lowest price first), up to `limit` levels.
    /// Each level aggregates the remaining quantities of all resting orders at that price.
    pub fn ask_levels(&self, limit: usize) -> impl Iterator<Item = (Price, Quantity)> + '_ {
        self.asks
            .levels()
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
                .levels()
                .map(|(Reverse(price), orders)| PriceLevel {
                    price: *price,
                    orders: orders.iter().cloned().collect(),
                })
                .collect(),
            asks: book
                .asks
                .levels()
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
            bids: OrderQueue::from_levels(bids),
            asks: OrderQueue::from_levels(asks),
            resting_orders,
            filled_orders,
        }
    }
}
