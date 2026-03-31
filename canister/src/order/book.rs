use super::{Order, OrderId, Price, Quantity, RestingOrder, Side};
use dex_types::OrderStatus;
use std::cmp::Reverse;
use std::collections::btree_map;
use std::collections::{BTreeMap, VecDeque};

/// Central limit order book for a single trading pair.
///
/// Bids are sorted by price descending (best bid = highest price).
/// Asks are sorted by price ascending (best ask = lowest price).
/// Within a price level, orders are matched in FIFO order.
#[derive(Debug)]
pub struct OrderBook {
    /// Minimum price increment. All order prices must be a multiple of this value.
    tick_size: Price,
    /// Minimum order quantity. All order quantities must be a multiple of this value.
    lot_size: Quantity,
    /// Buy side, sorted by price descending (highest first) via [`Reverse<Price>`].
    bids: BTreeMap<Reverse<Price>, VecDeque<RestingOrder>>,
    /// Sell side, sorted by price ascending (lowest first).
    asks: BTreeMap<Price, VecDeque<RestingOrder>>,
    /// Index mapping order IDs to their location (side, price) for O(log n) lookup.
    orders: BTreeMap<OrderId, (Side, Price)>,
    /// Orders awaiting matching, processed by the timer.
    pending_orders: VecDeque<Order>,
}

impl OrderBook {
    /// Creates a new empty order book with the given constraints.
    ///
    /// # Panics
    ///
    /// Panics if `tick_size` or `lot_size` is zero.
    pub fn new(tick_size: Price, lot_size: Quantity) -> Self {
        assert!(!tick_size.is_zero(), "tick_size must be non-zero");
        assert!(!lot_size.is_zero(), "lot_size must be non-zero");

        Self {
            tick_size,
            lot_size,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            orders: BTreeMap::new(),
            pending_orders: VecDeque::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        assert_eq!(
            self.bids.is_empty() && self.asks.is_empty(),
            self.orders.is_empty(),
            "BUG: orders should be empty iff both bids and asks are empty"
        );
        self.orders.is_empty() && self.pending_orders.is_empty()
    }

    pub fn tick_size(&self) -> Price {
        self.tick_size
    }

    pub fn lot_size(&self) -> Quantity {
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
        self.validate_order(&order)?;

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
                    let price = *entry.key();
                    fill_against_queue(price, entry, &mut order, &mut fills, &mut self.orders);
                }
            }
            Side::Sell => {
                while !order.remaining_quantity().is_zero() {
                    let Some(entry) = self.bids.first_entry() else {
                        break;
                    };
                    let Reverse(price) = *entry.key();
                    if price < order.price() {
                        break;
                    }
                    fill_against_queue(price, entry, &mut order, &mut fills, &mut self.orders);
                }
            }
        }

        if order.remaining_quantity().is_zero() {
            Ok(MatchResult::Filled { fills })
        } else {
            let resting_order_id = order.id();
            self.insert_order(order);
            if fills.is_empty() {
                Ok(MatchResult::Resting { resting_order_id })
            } else {
                Ok(MatchResult::PartiallyFilled {
                    fills,
                    resting_order_id,
                })
            }
        }
    }

    fn validate_order(&self, order: &Order) -> Result<(), MatchOrderError> {
        if order.price().is_zero() || !order.price().is_multiple_of(self.tick_size) {
            return Err(MatchOrderError::InvalidTickSize {
                price: order.price(),
                tick_size: self.tick_size,
            });
        }
        if order.remaining_quantity().is_zero()
            || !order.remaining_quantity().is_multiple_of(self.lot_size)
        {
            return Err(MatchOrderError::InvalidLotSize {
                quantity: order.remaining_quantity(),
                lot_size: self.lot_size,
            });
        }
        Ok(())
    }

    /// Look up a resting order by its ID.
    pub fn get_order(&self, order_id: OrderId) -> Option<Order> {
        let &(side, price) = self.orders.get(&order_id)?;
        let queue = match side {
            Side::Buy => self.bids.get(&Reverse(price))?,
            Side::Sell => self.asks.get(&price)?,
        };
        let resting = queue.iter().find(|o| o.id() == order_id)?;
        Some(resting.to_order(side, price))
    }

    /// Returns `true` if an order with the given ID is resting in the book.
    pub fn has_order(&self, order_id: &OrderId) -> bool {
        self.orders.contains_key(order_id)
    }

    /// Validate and enqueue an order for matching.
    pub fn add_pending_order(&mut self, order: Order) -> Result<(), MatchOrderError> {
        self.validate_order(&order)?;
        self.pending_orders.push_back(order);
        Ok(())
    }

    /// Drain the pending queue and match each order against the book.
    pub fn process_pending_orders(&mut self) {
        while let Some(order) = self.pending_orders.pop_front() {
            // Validation already happened in add_pending_order, so match_order
            // should not fail. If it does, we skip the order.
            match self.match_order(order) {
                Ok(_result) => {
                    // TODO: settle fills (credit/debit balances)
                }
                Err(_err) => {
                    // TODO: handle invalid orders (return funds to user)
                }
            }
        }
    }

    /// Returns the status of an order in this book, or `None` if not found.
    pub fn get_order_status(&self, order_id: OrderId) -> Option<OrderStatus> {
        if self.pending_orders.iter().any(|o| o.id() == order_id) {
            return Some(OrderStatus::Pending);
        }
        if self.has_order(&order_id) {
            return Some(OrderStatus::Open);
        }
        None
    }

    fn insert_order(&mut self, order: Order) {
        let side = order.side();
        let price = order.price();
        assert_eq!(self.orders.insert(order.id(), (side, price)), None);
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
}

fn fill_against_queue<K: Ord>(
    price: Price,
    mut entry: btree_map::OccupiedEntry<'_, K, VecDeque<RestingOrder>>,
    order: &mut Order,
    fills: &mut Vec<Fill>,
    orders_index: &mut BTreeMap<OrderId, (Side, Price)>,
) {
    let resting_orders = entry.get_mut();
    while !order.remaining_quantity().is_zero() && !resting_orders.is_empty() {
        let Some(resting) = resting_orders.front_mut() else {
            break;
        };
        let fill_qty = order.remaining_quantity().min(resting.remaining_quantity());

        order.reduce_quantity(fill_qty);
        resting.reduce_quantity(fill_qty);

        fills.push(Fill {
            maker_order_id: resting.id(),
            price,
            quantity: fill_qty,
        });

        if resting.remaining_quantity().is_zero() {
            let filled = resting_orders.pop_front().expect("front exists");
            assert!(orders_index.remove(&filled.id()).is_some());
        }
    }
    if resting_orders.is_empty() {
        entry.remove();
    }
}

/// The result of matching an incoming order against the book.
#[derive(Debug, PartialEq, Eq)]
pub enum MatchResult {
    /// The order was fully filled and does not rest in the book.
    Filled { fills: Vec<Fill> },
    /// The order was partially filled and the remainder is now resting in the book.
    PartiallyFilled {
        fills: Vec<Fill>,
        resting_order_id: OrderId,
    },
    /// No match was found; the order is resting in the book.
    Resting { resting_order_id: OrderId },
}

impl MatchResult {
    pub fn fills(&self) -> &[Fill] {
        match self {
            MatchResult::Filled { fills } | MatchResult::PartiallyFilled { fills, .. } => fills,
            MatchResult::Resting { .. } => &[],
        }
    }
}

/// A single fill produced when an incoming order matches a resting order.
#[derive(Debug, PartialEq, Eq)]
pub struct Fill {
    /// The ID of the resting (maker) order that was matched.
    pub maker_order_id: OrderId,
    /// The price at which the fill occurred (always the maker's price).
    pub price: Price,
    /// The quantity filled.
    pub quantity: Quantity,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MatchOrderError {
    /// Price is not a positive multiple of the tick size.
    InvalidTickSize { price: Price, tick_size: Price },
    /// Quantity is not a positive multiple of the lot size.
    InvalidLotSize {
        quantity: Quantity,
        lot_size: Quantity,
    },
}
