use super::{LotSize, Order, OrderBookId, OrderSeq, Price, Quantity, RestingOrder, Side, TickSize};
use minicbor::{Decode, Encode};
use std::cmp::Reverse;
use std::collections::btree_map;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
    pub fn new(id: OrderBookId, tick_size: TickSize, lot_size: LotSize) -> Self {
        Self {
            id,
            next_seq: OrderSeq::default(),
            tick_size,
            lot_size,
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

    /// Returns the best (highest price) bid order, or `None` if the bid side is empty.
    pub fn best_bid(&self) -> Option<Order> {
        let (&Reverse(price), queue) = self.bids.first_key_value()?;
        let resting = queue.front()?;
        Some(resting.to_order(Side::Buy, price))
    }

    /// Returns the best (lowest price) ask order, or `None` if the ask side is empty.
    pub fn best_ask(&self) -> Option<Order> {
        let (&price, queue) = self.asks.first_key_value()?;
        let resting = queue.front()?;
        Some(resting.to_order(Side::Sell, price))
    }

    /// Match an incoming order against the book.
    ///
    /// Validates tick size, lot size, and rejects zero price/quantity, then attempts
    /// to fill the order against the opposite side. Returns:
    /// - [`MatchResult::Filled`] if the order is fully filled.
    /// - [`MatchResult::PartiallyFilled`] if partially filled with the remainder resting.
    /// - [`MatchResult::Resting`] if no match was found and the order rests as-is.
    pub fn match_order(&mut self, mut order: Order) -> Result<MatchResult, MatchOrderError> {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("book::match_order");
        self.validate_order(order.price(), order.remaining_quantity())?;

        let mut fills = Vec::new();

        match order.side() {
            Side::Buy => {
                while !order.remaining_quantity().is_zero() {
                    let Some(entry) = self.asks.first_entry() else {
                        break;
                    };
                    if *entry.key() > order.price() {
                        break;
                    }
                    let maker_price = *entry.key();
                    fill_against_queue(
                        maker_price,
                        entry,
                        &mut order,
                        &mut fills,
                        &mut self.resting_orders,
                        &mut self.filled_orders,
                    );
                }
            }
            Side::Sell => {
                while !order.remaining_quantity().is_zero() {
                    let Some(entry) = self.bids.first_entry() else {
                        break;
                    };
                    let Reverse(maker_price) = *entry.key();
                    if maker_price < order.price() {
                        break;
                    }
                    fill_against_queue(
                        maker_price,
                        entry,
                        &mut order,
                        &mut fills,
                        &mut self.resting_orders,
                        &mut self.filled_orders,
                    );
                }
            }
        }

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

    /// Enqueue an order for matching.
    pub fn add_pending_order(&mut self, order: Order) {
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
    pub fn process_pending_orders(&mut self, expected_seqs: &[OrderSeq]) -> MatchingOutput {
        // TODO DEFI-2743: chunk matching orders to avoid hitting the instruction limit.
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

fn sum_remaining(queue: &VecDeque<RestingOrder>) -> Quantity {
    queue
        .iter()
        .try_fold(Quantity::ZERO, |acc, order| {
            acc.checked_add(*order.remaining_quantity())
        })
        .expect("BUG: aggregate quantity at a price level overflowed u256")
}

fn fill_against_queue<K: Ord>(
    maker_price: Price,
    mut entry: btree_map::OccupiedEntry<'_, K, VecDeque<RestingOrder>>,
    order: &mut Order,
    fills: &mut Vec<Fill>,
    orders_index: &mut BTreeMap<OrderSeq, (Side, Price)>,
    filled_orders: &mut BTreeSet<OrderSeq>,
) {
    #[cfg(feature = "canbench-rs")]
    let _p = canbench_rs::bench_scope("book::fill_against_queue");
    let resting_orders = entry.get_mut();
    while !order.remaining_quantity().is_zero() && !resting_orders.is_empty() {
        let Some(resting) = resting_orders.front_mut() else {
            break;
        };
        let fill_qty = *std::cmp::min(order.remaining_quantity(), resting.remaining_quantity());

        order.reduce_quantity(&fill_qty);
        resting.reduce_quantity(&fill_qty);

        fills.push(Fill {
            taker_order_seq: order.id(),
            taker_side: order.side(),
            taker_price: order.price(),
            maker_order_seq: resting.id(),
            maker_price,
            quantity: fill_qty,
        });

        if resting.remaining_quantity().is_zero() {
            let filled = resting_orders.pop_front().expect("front exists");
            assert!(orders_index.remove(&filled.id()).is_some());
            filled_orders.insert(filled.id());
        }
    }
    if resting_orders.is_empty() {
        entry.remove();
    }
}

/// Output of [`OrderBook::process_pending_orders`]: the fills produced,
/// orders that began resting in the book, and orders that were fully filled.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
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
    /// The amount of quote tokens exchanged (maker_price × quantity).
    pub fn quote_amount(&self) -> Quantity {
        self.quantity
            .checked_mul_u64(self.maker_price.0)
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
            pending_orders,
            bids,
            asks,
            resting_orders,
            filled_orders,
        }
    }
}
