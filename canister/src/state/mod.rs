use crate::order::{Order, OrderId, PendingOrder};
use dex_types::OrderStatus;
use std::cell::RefCell;
use std::collections::VecDeque;

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(&s.borrow()))
}

pub fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

#[derive(Debug, Default)]
pub struct State {
    next_order_id: OrderId,
    pending_orders: VecDeque<Order>,
}

impl State {
    pub fn add_limit_order(&mut self, pending: PendingOrder) -> OrderId {
        let order_id = self.next_order_id.increment();
        self.pending_orders
            .push_back(Order::from_pending(pending, order_id));
        order_id
    }

    pub fn get_order_status(&self, order_id: OrderId) -> OrderStatus {
        if self.pending_orders.iter().any(|o| o.id == order_id) {
            OrderStatus::Pending
        } else {
            OrderStatus::NotFound
        }
    }
}
