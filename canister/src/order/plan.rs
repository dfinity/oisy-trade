use super::{Order, OrderSeq, Quantity, RestingOrder};

/// A read-only plan of the fills an incoming order would make, produced by
/// [`OrderBook::plan_fills`] and replayed by [`OrderBook::apply_plan`].
///
/// The matching invariant `fill_qty = min(maker.remaining, taker.remaining)`
/// implies every maker the taker touches is fully consumed *except* possibly
/// the last one. The plan therefore records only:
///
/// - the fully-consumed makers, in execution order, by sequence;
/// - the optional last partial fill, as `(maker_seq, fill_qty)`;
/// - the taker's remaining quantity after the plan executes (zero ⇒ filled).
///
/// Build a plan by constructing it with [`FillPlan::new`] and calling
/// [`FillPlan::add_fill`] in execution order. The method enforces the
/// "at most one partial fill, always last" invariant and decrements
/// `taker_remaining` per fill.
///
/// [`OrderBook::plan_fills`]: super::book::OrderBook::plan_fills
/// [`OrderBook::apply_plan`]: super::book::OrderBook::apply_plan
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FillPlan {
    taker_order: OrderSeq,
    filled_makers: Vec<OrderSeq>,
    last_partial: Option<(OrderSeq, Quantity)>,
    taker_remaining: Quantity,
}

impl FillPlan {
    pub fn new(order: &Order) -> Self {
        Self {
            taker_order: order.id(),
            filled_makers: Vec::new(),
            last_partial: None,
            taker_remaining: *order.remaining_quantity(),
        }
    }

    /// Record a fill of `min(taker_remaining, resting.remaining_quantity())`
    /// against `resting`. If the fill empties the maker it is appended to
    /// `filled_makers`; otherwise it becomes the (final) partial fill and
    /// the plan is locked against further fills.
    ///
    /// Panics if a partial fill is already recorded (no fill may follow a
    /// partial), or if `resting.id()` equals the taker — a self-fill.
    pub fn add_fill(&mut self, resting: &RestingOrder) {
        assert!(
            self.last_partial.is_none(),
            "BUG: cannot add a fill after a partial fill"
        );
        let maker_seq = resting.id();
        assert_ne!(
            maker_seq, self.taker_order,
            "BUG: maker_seq equals taker_order — self-fill"
        );
        let maker_qty = *resting.remaining_quantity();
        let fill_qty = std::cmp::min(self.taker_remaining, maker_qty);
        self.taker_remaining = self
            .taker_remaining
            .checked_sub(fill_qty)
            .expect("BUG: fill_qty exceeds taker_remaining");
        // `fill_qty == min(taker_remaining, maker_qty)`, so the maker is fully
        // consumed exactly when `fill_qty == maker_qty` — no subtraction needed.
        if fill_qty == maker_qty {
            self.filled_makers.push(maker_seq);
        } else {
            assert_eq!(
                self.taker_remaining,
                Quantity::ZERO,
                "BUG: partial fill of maker can only happen if taker fully consumed"
            );
            self.last_partial = Some((maker_seq, fill_qty));
        }
    }

    pub fn taker_order(&self) -> OrderSeq {
        self.taker_order
    }

    pub fn filled_makers(&self) -> &[OrderSeq] {
        &self.filled_makers
    }

    pub fn last_partial(&self) -> Option<(OrderSeq, Quantity)> {
        self.last_partial
    }

    pub fn taker_remaining(&self) -> Quantity {
        self.taker_remaining
    }
}
