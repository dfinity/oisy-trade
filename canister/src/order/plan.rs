use super::{OrderSeq, Price, Quantity};

/// A read-only plan of the fills an incoming order would make, produced by
/// [`OrderBook::plan_fills`] and replayed by [`OrderBook::apply_plan`].
///
/// [`OrderBook::plan_fills`]: super::book::OrderBook::plan_fills
/// [`OrderBook::apply_plan`]: super::book::OrderBook::apply_plan
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
