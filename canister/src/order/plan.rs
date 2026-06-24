use super::{Order, OrderSeq, Quantity, RestingOrder, TimeInForce};

/// A read-only, validated plan of the fills an incoming order will make,
/// produced by [`FillPlanBuilder::build`] and replayed by
/// [`OrderBook::apply_plan`].
///
/// The matching invariant `fill_qty = min(maker.remaining, taker.remaining)`
/// implies every maker the taker touches is fully consumed *except* possibly
/// the last one. The plan therefore records only:
///
/// - the fully-consumed makers, in execution order, by sequence;
/// - the optional last partial fill, as `(maker_seq, fill_qty)`;
/// - the taker's remaining quantity after the plan executes (zero ⇒ filled).
///
/// A `FillPlan` exists only once it has passed [`FillPlanBuilder::build`], so
/// holding one is proof the order may be executed against the book.
///
/// [`OrderBook::apply_plan`]: super::book::OrderBook::apply_plan
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FillPlan {
    taker_order: OrderSeq,
    filled_makers: Vec<OrderSeq>,
    last_partial: Option<(OrderSeq, Quantity)>,
    taker_remaining: Quantity,
}

impl FillPlan {
    /// Start accumulating fills for `order`. The builder borrows `order`, so
    /// the plan can only ever be built — and validated — for that order.
    pub fn builder(order: &Order) -> FillPlanBuilder<'_> {
        FillPlanBuilder {
            order,
            filled_makers: Vec::new(),
            last_partial: None,
            taker_remaining: *order.remaining_quantity(),
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

/// Accumulator for a [`FillPlan`], built during the read-only walk of the
/// crossing makers in [`OrderBook::plan_fills`].
///
/// The builder borrows the incoming [`Order`] and has no access to the book,
/// so neither accumulating fills nor validating the plan can mutate the book.
/// Call [`FillPlanBuilder::add_fill`] in execution order, then
/// [`FillPlanBuilder::build`] to validate the plan against the order's
/// time-in-force.
///
/// [`OrderBook::plan_fills`]: super::book::OrderBook::plan_fills
pub(crate) struct FillPlanBuilder<'a> {
    order: &'a Order,
    filled_makers: Vec<OrderSeq>,
    last_partial: Option<(OrderSeq, Quantity)>,
    taker_remaining: Quantity,
}

impl FillPlanBuilder<'_> {
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
            maker_seq,
            self.order.id(),
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

    pub fn taker_remaining(&self) -> Quantity {
        self.taker_remaining
    }

    /// Validate the accumulated fills against the order's time-in-force and
    /// produce either an executable [`FillPlan`] or a [`PlanOutcome::Kill`]
    /// decision.
    ///
    /// A fill-or-kill order that cannot fully fill (any taker quantity left
    /// over) is killed; every other order yields an executable plan. The
    /// builder has no access to the book, so this decision provably cannot
    /// mutate it.
    pub fn build(self) -> PlanOutcome {
        match self.order.time_in_force() {
            TimeInForce::GoodTilCanceled => PlanOutcome::Execute(self.into_plan()),
            TimeInForce::FillOrKill => {
                if !self.taker_remaining.is_zero() {
                    return PlanOutcome::Kill;
                }
                PlanOutcome::Execute(self.into_plan())
            }
        }
    }

    fn into_plan(self) -> FillPlan {
        FillPlan {
            taker_order: self.order.id(),
            filled_makers: self.filled_makers,
            last_partial: self.last_partial,
            taker_remaining: self.taker_remaining,
        }
    }
}

/// Outcome of validating a [`FillPlanBuilder`]: either an executable plan to
/// replay against the book, or a decision to kill the order without touching
/// the book.
pub(crate) enum PlanOutcome {
    /// The plan is valid for the order; replay it via [`OrderBook::apply_plan`].
    ///
    /// [`OrderBook::apply_plan`]: super::book::OrderBook::apply_plan
    Execute(FillPlan),
    /// A fill-or-kill order that could not fully fill. The book must not be
    /// touched; the caller releases the placement reservation.
    Kill,
}
