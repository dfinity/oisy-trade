use crate::Task;
use crate::order::{
    MatchOrderError, OrderBook, OrderId, PendingOrder, TokenId, TokenMetadata, TradingPair,
};
use dex_types::OrderStatus;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

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
    tokens: BTreeMap<TokenId, TokenMetadata>,
    order_books: BTreeMap<TradingPair, OrderBook>,
    active_tasks: BTreeSet<Task>,
}

impl State {
    pub fn next_order_id(&mut self) -> OrderId {
        let id = self.next_order_id;
        self.next_order_id.increment();
        id
    }

    pub fn add_limit_order(
        &mut self,
        pair: TradingPair,
        pending: PendingOrder,
    ) -> Result<OrderId, AddLimitOrderError> {
        // TODO DEFI-2723: ensure the user has enough balance
        // TODO DEFI-2723: only update ID if order is valid.
        let order_id = self.next_order_id();
        let order = pending.into_order(order_id);
        let book = self
            .order_books
            .get_mut(&pair)
            .ok_or(AddLimitOrderError::UnknownTradingPair)?;
        book.add_pending_order(order)
            .map_err(AddLimitOrderError::InvalidOrder)?;
        Ok(order_id)
    }

    pub fn process_pending_orders(&mut self) {
        // TODO DEFI-2743: chunk matching orders to avoid hitting the instruction limit.
        for book in self.order_books.values_mut() {
            book.process_pending_orders();
        }
    }

    pub fn get_order_status(&self, order_id: OrderId) -> OrderStatus {
        for book in self.order_books.values() {
            if let Some(status) = book.get_order_status(order_id) {
                return status;
            }
        }
        OrderStatus::NotFound
    }

    /// Register a new trading pair with the given order book.
    pub fn add_order_book(&mut self, pair: TradingPair, book: OrderBook) {
        assert!(
            self.order_books.insert(pair, book).is_none(),
            "ERROR: order book already exists for this pair"
        );
    }

    /// Set of currently active tasks to avoid parallel execution.
    pub fn active_tasks_mut(&mut self) -> &mut BTreeSet<Task> {
        &mut self.active_tasks
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddLimitOrderError {
    UnknownTradingPair,
    InvalidOrder(MatchOrderError),
}

impl From<AddLimitOrderError> for dex_types::AddLimitOrderError {
    fn from(err: AddLimitOrderError) -> Self {
        match err {
            AddLimitOrderError::UnknownTradingPair => {
                dex_types::AddLimitOrderError::UnknownTradingPair
            }
            AddLimitOrderError::InvalidOrder(MatchOrderError::InvalidTickSize {
                price,
                tick_size,
            }) => dex_types::AddLimitOrderError::InvalidPrice {
                price: price.get(),
                tick_size: tick_size.get(),
            },
            AddLimitOrderError::InvalidOrder(MatchOrderError::InvalidLotSize {
                quantity,
                lot_size,
            }) => dex_types::AddLimitOrderError::InvalidQuantity {
                quantity: quantity.get(),
                lot_size: lot_size.get(),
            },
        }
    }
}
