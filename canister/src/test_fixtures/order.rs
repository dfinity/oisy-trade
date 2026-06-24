use crate::Timestamp;
use crate::order::{self, PendingOrder, Price, Quantity, Side, TimeInForce, TradingPair};
use crate::state::{self, StableMemoryOptions};
use candid::Principal;

/// Builder for placing a limit order in tests. Construct with [`order`], tune
/// the time-in-force if needed (defaults to GTC), then call
/// [`PlaceOrder::place`] to deposit the reservation, validate, and record it.
pub struct PlaceOrder<'a> {
    user: Principal,
    pair: &'a TradingPair,
    side: Side,
    price: u128,
    quantity: Quantity,
    time_in_force: TimeInForce,
}

/// Start building a limit order placement. Defaults to good-til-canceled; call
/// [`PlaceOrder::fill_or_kill`] or [`PlaceOrder::time_in_force`] to change it.
pub fn order(
    user: Principal,
    pair: &TradingPair,
    side: Side,
    price: u128,
    quantity: impl Into<Quantity>,
) -> PlaceOrder<'_> {
    PlaceOrder {
        user,
        pair,
        side,
        price,
        quantity: quantity.into(),
        time_in_force: TimeInForce::GoodTilCanceled,
    }
}

impl<'a> PlaceOrder<'a> {
    /// Make this a fill-or-kill order.
    pub fn fill_or_kill(self) -> Self {
        self.time_in_force(TimeInForce::FillOrKill)
    }

    /// Set the time-in-force for this order.
    pub fn time_in_force(mut self, time_in_force: TimeInForce) -> Self {
        self.time_in_force = time_in_force;
        self
    }

    /// Deposit just enough of the appropriate token to cover the order's
    /// reservation, validate the resulting limit order, and record it. Returns
    /// the assigned `OrderId`. Each call funds the user from zero, so distinct
    /// users get distinct, isolated balances.
    pub fn place<MH, MB>(self, state: &mut state::State<MH, MB>) -> order::OrderId
    where
        MH: ic_stable_structures::Memory,
        MB: ic_stable_structures::Memory,
    {
        let pending = PendingOrder {
            side: self.side,
            price: Price::new(self.price),
            quantity: self.quantity,
            time_in_force: self.time_in_force,
        };
        let (token, amount) = match self.side {
            Side::Buy => (
                self.pair.quote,
                pending
                    .price
                    .checked_mul_quantity_scaled(
                        &pending.quantity,
                        state.base_scale(&self.pair.base),
                    )
                    .expect("order: price × quantity overflow"),
            ),
            Side::Sell => (self.pair.base, pending.quantity),
        };
        state.deposit(self.user, token, amount, StableMemoryOptions::Write);
        let (order_id, order) = state
            .validate_limit_order(self.user, self.pair.clone(), pending)
            .expect("order: validate_limit_order failed");
        state.record_limit_order(
            self.user,
            order_id.book_id(),
            order,
            Timestamp::EPOCH,
            StableMemoryOptions::Write,
        );
        order_id
    }
}
