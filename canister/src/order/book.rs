use super::{Order, OrderId, Price, Quantity, Side};
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
    bids: BTreeMap<Reverse<Price>, VecDeque<Order>>,
    /// Sell side, sorted by price ascending (lowest first).
    asks: BTreeMap<Price, VecDeque<Order>>,
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
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }

    pub fn tick_size(&self) -> Price {
        self.tick_size
    }

    pub fn lot_size(&self) -> Quantity {
        self.lot_size
    }

    /// Returns the best (highest price) bid order, or `None` if the bid side is empty.
    pub fn best_bid(&self) -> Option<&Order> {
        self.bids.first_key_value().and_then(|(_, q)| q.front())
    }

    /// Returns the best (lowest price) ask order, or `None` if the ask side is empty.
    pub fn best_ask(&self) -> Option<&Order> {
        self.asks.first_key_value().and_then(|(_, q)| q.front())
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
                    fill_against_queue(price, entry, &mut order, &mut fills);
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
                    fill_against_queue(price, entry, &mut order, &mut fills);
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

    fn insert_order(&mut self, order: Order) {
        match order.side() {
            Side::Buy => self
                .bids
                .entry(Reverse(order.price()))
                .or_default()
                .push_back(order),
            Side::Sell => self.asks.entry(order.price()).or_default().push_back(order),
        }
    }
}

fn fill_against_queue<K: Ord>(
    price: Price,
    mut entry: btree_map::OccupiedEntry<'_, K, VecDeque<Order>>,
    order: &mut Order,
    fills: &mut Vec<Fill>,
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
            resting_orders.pop_front();
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
