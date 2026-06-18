use super::book::{Fill, OrderBook};
use super::{Order, OrderSeq, Price, Quantity, RestingOrder, Side};
use std::cmp::Reverse;
use std::collections::VecDeque;

impl OrderBook {
    /// Read-only walk of the crossing price levels, recording the fills an
    /// incoming order of `side`/`price`/`quantity` *would* make.
    ///
    /// Iterates best-first (asks ascending while `ask_price <= price`; bids
    /// descending while `bid_price >= price`) and, FIFO within each level,
    /// records one [`PlannedFill`] per maker it would touch, accumulating until
    /// the order is satisfied. Mutates no book state.
    pub(crate) fn plan_fills(&self, side: Side, price: Price, quantity: Quantity) -> FillPlan {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("book::plan_fills");
        let mut fills = Vec::new();
        let remaining = match side {
            Side::Buy => self.plan_against_asks(price, quantity, &mut fills),
            Side::Sell => self.plan_against_bids(price, quantity, &mut fills),
        };

        FillPlan {
            fills,
            fully_filled: remaining.is_zero(),
        }
    }

    /// Walk the ask side (ascending) for a buy taker, recording fills while the
    /// ask price crosses (`ask_price <= price`). Returns the unfilled remainder.
    /// Allocation-free until the first crossing maker is found.
    fn plan_against_asks(
        &self,
        price: Price,
        quantity: Quantity,
        fills: &mut Vec<PlannedFill>,
    ) -> Quantity {
        let mut remaining = quantity;
        for (&ask_price, queue) in &self.asks {
            if ask_price > price || remaining.is_zero() {
                break;
            }
            Self::plan_level(ask_price, queue, &mut remaining, fills);
        }
        remaining
    }

    /// Walk the bid side (descending) for a sell taker, recording fills while the
    /// bid price crosses (`bid_price >= price`). Returns the unfilled remainder.
    /// Allocation-free until the first crossing maker is found.
    fn plan_against_bids(
        &self,
        price: Price,
        quantity: Quantity,
        fills: &mut Vec<PlannedFill>,
    ) -> Quantity {
        let mut remaining = quantity;
        for (&Reverse(bid_price), queue) in &self.bids {
            if bid_price < price || remaining.is_zero() {
                break;
            }
            Self::plan_level(bid_price, queue, &mut remaining, fills);
        }
        remaining
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
    pub(crate) fn apply_plan(
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
                assert!(self.resting_orders.remove(&filled.id()).is_some());
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
}

/// A read-only plan of the fills an incoming order would make, produced by
/// [`OrderBook::plan_fills`] and replayed by [`OrderBook::apply_plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FillPlan {
    /// The fills, in execution order (best price first, FIFO within a level).
    pub fills: Vec<PlannedFill>,
    /// Whether the order's full quantity is satisfied by `fills`.
    pub fully_filled: bool,
}

/// A single fill an incoming order would make against one resting maker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlannedFill {
    /// Sequence of the resting (maker) order to fill against.
    pub maker_seq: OrderSeq,
    /// Price of the maker's level (the fill executes at this price).
    pub maker_price: Price,
    /// Quantity to fill against this maker.
    pub fill_qty: Quantity,
    /// Whether this fill empties the maker (it is removed from the book).
    pub maker_emptied: bool,
}
