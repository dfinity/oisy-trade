use crate::order::{Order, OrderId, PendingOrder};
use dex_types::{OrderStatus, Token};
use std::cell::RefCell;
use std::collections::{BTreeSet, VecDeque};

thread_local! {
    static STATE: RefCell<Option<State>> = RefCell::default();
}

pub fn with_state<R>(f: impl FnOnce(&State) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("State not initialized!")))
}

pub fn with_state_mut<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(s.borrow_mut().as_mut().expect("State not initialized!")))
}

pub fn init_state() {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        assert!(state.is_none(), "State already initialized!");
        *state = Some(State::default());
    });
}

#[derive(Debug, Default)]
pub struct State {
    next_order_id: OrderId,
    pending_orders: VecDeque<Order>,
    supported_tokens: BTreeSet<Token>,
}

impl State {
    pub fn next_order_id(&mut self) -> OrderId {
        let id = self.next_order_id;
        self.next_order_id.increment();
        id
    }

    pub fn add_limit_order(&mut self, pending: PendingOrder) -> OrderId {
        let order_id = self.next_order_id();
        self.pending_orders.push_back(pending.into_order(order_id));
        order_id
    }

    pub fn add_supported_token(&mut self, token: Token) {
        self.supported_tokens.insert(token);
    }

    pub fn get_order_status(&self, order_id: OrderId) -> OrderStatus {
        if self.pending_orders.iter().any(|o| o.id == order_id) {
            OrderStatus::Pending
        } else {
            OrderStatus::NotFound
        }
    }

    pub fn get_supported_tokens(&self) -> Vec<Token> {
        self.supported_tokens.iter().cloned().collect()
    }
}
