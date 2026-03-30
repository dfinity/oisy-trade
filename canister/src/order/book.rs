use super::{Fill, MatchOrderError, MatchResult, Order, Price, Quantity};
use dex_types::Side;
use std::cmp::Reverse;
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
    pub fn new(tick_size: Price, lot_size: Quantity) -> Self {
        Self {
            tick_size,
            lot_size,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    pub fn tick_size(&self) -> Price {
        self.tick_size
    }

    pub fn lot_size(&self) -> Quantity {
        self.lot_size
    }

    /// Match an incoming order against the book.
    ///
    /// Validates tick size and lot size, then attempts to fill the order against
    /// the opposite side. If the order is fully filled, returns [`MatchResult::Filled`].
    /// Otherwise the remainder is inserted into the book as a resting order and
    /// [`MatchResult::Resting`] is returned.
    pub fn match_order(&mut self, mut order: Order) -> Result<MatchResult, MatchOrderError> {
        self.validate_order(&order)?;

        let mut fills = Vec::new();

        match order.side() {
            Side::Buy => self.match_buy(&mut order, &mut fills),
            Side::Sell => self.match_sell(&mut order, &mut fills),
        }

        if order.remaining_quantity().is_zero() {
            Ok(MatchResult::Filled { fills })
        } else {
            let resting_order_id = order.id();
            self.insert_order(order);
            Ok(MatchResult::Resting {
                fills,
                resting_order_id,
            })
        }
    }

    fn validate_order(&self, order: &Order) -> Result<(), MatchOrderError> {
        if order.price().get() % self.tick_size.get() != 0 {
            return Err(MatchOrderError::InvalidTickSize {
                price: order.price(),
                tick_size: self.tick_size,
            });
        }
        if order.remaining_quantity().get() % self.lot_size.get() != 0 {
            return Err(MatchOrderError::InvalidLotSize {
                quantity: order.remaining_quantity(),
                lot_size: self.lot_size,
            });
        }
        Ok(())
    }

    fn match_buy(&mut self, order: &mut Order, fills: &mut Vec<Fill>) {
        while !order.remaining_quantity().is_zero() {
            let Some((&ask_price, _)) = self.asks.first_key_value() else {
                break;
            };
            if ask_price > order.price() {
                break;
            }
            let queue = self.asks.get_mut(&ask_price).expect("price level must exist");
            Self::fill_against_queue(ask_price, queue, order, fills);
            if queue.is_empty() {
                self.asks.remove(&ask_price);
            }
        }
    }

    fn match_sell(&mut self, order: &mut Order, fills: &mut Vec<Fill>) {
        while !order.remaining_quantity().is_zero() {
            let Some((&Reverse(bid_price), _)) = self.bids.first_key_value() else {
                break;
            };
            if bid_price < order.price() {
                break;
            }
            let queue = self.bids.get_mut(&Reverse(bid_price)).expect("price level must exist");
            Self::fill_against_queue(bid_price, queue, order, fills);
            if queue.is_empty() {
                self.bids.remove(&Reverse(bid_price));
            }
        }
    }

    fn fill_against_queue(
        price: Price,
        queue: &mut VecDeque<Order>,
        order: &mut Order,
        fills: &mut Vec<Fill>,
    ) {
        while !order.remaining_quantity().is_zero() {
            let Some(resting) = queue.front_mut() else {
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
                queue.pop_front();
            }
        }
    }

    fn insert_order(&mut self, order: Order) {
        match order.side() {
            Side::Buy => self
                .bids
                .entry(Reverse(order.price()))
                .or_default()
                .push_back(order),
            Side::Sell => self
                .asks
                .entry(order.price())
                .or_default()
                .push_back(order),
        }
    }
}
