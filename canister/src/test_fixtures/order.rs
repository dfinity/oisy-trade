use crate::Timestamp;
use crate::order::{self, PendingOrder, Price, Quantity, Side, TimeInForce, TradingPair};
use crate::state::{self, StableMemoryOptions};
use candid::Principal;

pub struct PlaceOrder<'a> {
    user: Principal,
    pair: &'a TradingPair,
    side: Side,
    price: u128,
    quantity: Quantity,
    time_in_force: TimeInForce,
}

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
    pub fn fill_or_kill(self) -> Self {
        self.time_in_force(TimeInForce::FillOrKill)
    }

    pub fn time_in_force(mut self, time_in_force: TimeInForce) -> Self {
        self.time_in_force = time_in_force;
        self
    }

    /// Deposit just enough of the appropriate token to cover the order's
    /// reservation, validate the resulting limit order, and record it. Returns
    /// the assigned `OrderId`.
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
            .validate_limit_order(
                state.lookup_account(self.user).as_ref(),
                self.pair.clone(),
                pending,
            )
            .expect("order: validate_limit_order failed");
        state.record_limit_order(
            self.user,
            order_id.book_id(),
            order,
            None,
            Timestamp::EPOCH,
            StableMemoryOptions::Write,
        );
        order_id
    }
}
