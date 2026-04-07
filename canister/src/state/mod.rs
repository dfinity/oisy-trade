#[cfg(test)]
mod tests;

use crate::Runtime;
use crate::Task;
use crate::balance::Balance;
use crate::order::{
    LotSize, MatchOrderError, OrderBook, OrderBookId, OrderId, PendingOrder, TickSize, TokenId,
    TokenMetadata, TradingPair,
};
use candid::{Nat, Principal};
use dex_types::{OrderStatus, TradingPairInfo};
use dex_types_internal::{InitArg, Mode};
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

pub fn init_state(init_arg: InitArg) {
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        assert!(state.is_none(), "State already initialized!");
        *state = Some(State::try_from(init_arg).expect("Failed to initialize state"));
    });
}

#[derive(Debug)]
pub struct State {
    mode: Mode,
    next_book_id: OrderBookId,
    #[allow(dead_code)] //TODO DEFI-2744: add trading pairs
    tokens: BTreeMap<TokenId, TokenMetadata>,
    trading_pairs: BTreeMap<TradingPair, OrderBookId>,
    order_books: BTreeMap<OrderBookId, OrderBook>,
    // TODO(DEFI-2746): Add support for subaccounts.
    balances: BTreeMap<Principal, BTreeMap<TokenId, Balance>>,
    active_tasks: BTreeSet<Task>,
}

impl TryFrom<InitArg> for State {
    type Error = String;

    fn try_from(init_arg: InitArg) -> Result<Self, Self::Error> {
        Ok(Self {
            mode: init_arg.mode,
            next_book_id: OrderBookId::default(),
            tokens: BTreeMap::default(),
            trading_pairs: BTreeMap::default(),
            order_books: BTreeMap::default(),
            balances: BTreeMap::default(),
            active_tasks: BTreeSet::default(),
        })
    }
}

impl State {
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    pub fn assert_caller_is_allowed(&self, runtime: &impl Runtime) {
        if let Mode::RestrictedTo(ref allowed) = self.mode {
            let caller = runtime.msg_caller();
            if runtime.is_controller(&caller) {
                return;
            }
            if !allowed.contains(&caller) {
                panic!(
                    "Caller {} is not allowed to call this endpoint in restricted mode",
                    caller
                );
            }
        }
    }

    fn next_book_id(&mut self) -> OrderBookId {
        let id = self.next_book_id;
        self.next_book_id.increment();
        id
    }

    pub fn add_limit_order(
        &mut self,
        user: Principal,
        pair: TradingPair,
        pending: PendingOrder,
    ) -> Result<OrderId, AddLimitOrderError> {
        use crate::order::Side;

        let book_id = self
            .trading_pairs
            .get(&pair)
            .ok_or(AddLimitOrderError::UnknownTradingPair)?;
        let book = self
            .order_books
            .get_mut(book_id)
            .expect("BUG: trading pair registered but order book missing");

        book.validate_order(pending.price, pending.quantity)
            .map_err(AddLimitOrderError::InvalidOrder)?;

        let (token, required) = match pending.side {
            Side::Buy => (
                pair.quote,
                Nat::from(pending.price.get()) * Nat::from(pending.quantity.get()),
            ),
            Side::Sell => (pair.base, Nat::from(pending.quantity.get())),
        };
        self.balances
            .entry(user)
            .or_default()
            .entry(token)
            .or_default()
            .reserve(required)
            .map_err(|e| AddLimitOrderError::InsufficientBalance {
                token,
                available: e.available,
                required: e.required,
            })?;

        let order_id = book
            .add_pending_order(pending)
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
        let book = self.order_books.get(&order_id.book_id());
        match book {
            Some(book) => book
                .get_order_status(order_id.seq())
                .unwrap_or(OrderStatus::NotFound),
            None => OrderStatus::NotFound,
        }
    }

    /// Register a new trading pair with a new order book.
    pub fn add_trading_pair(
        &mut self,
        pair: TradingPair,
        tick_size: TickSize,
        lot_size: LotSize,
    ) -> Result<(), dex_types::AddTradingPairError> {
        if self.trading_pairs.contains_key(&pair) {
            return Err(dex_types::AddTradingPairError::TradingPairAlreadyExists);
        }
        let book_id = self.next_book_id();
        let book = OrderBook::new(book_id, tick_size, lot_size);
        assert_eq!(self.trading_pairs.insert(pair, book_id), None);
        assert_eq!(self.order_books.insert(book_id, book), None);
        Ok(())
    }

    pub fn get_trading_pairs(&self) -> Vec<TradingPairInfo> {
        self.trading_pairs
            .iter()
            .map(|(pair, book_id)| {
                let book = self
                    .order_books
                    .get(book_id)
                    .expect("BUG: trading pair registered but order book missing");
                TradingPairInfo {
                    base_asset: dex_types::TokenId::from(pair.base),
                    quote_asset: dex_types::TokenId::from(pair.quote),
                    tick_size: book.tick_size().get(),
                    lot_size: book.lot_size().get(),
                }
            })
            .collect()
    }

    pub fn deposit(&mut self, user: Principal, token_id: TokenId, amount: Nat) {
        self.balances
            .entry(user)
            .or_default()
            .entry(token_id)
            .or_default()
            .deposit(amount);
    }

    pub fn get_balance(&self, user: Principal, token_id: TokenId) -> dex_types::Balance {
        self.balances
            .get(&user)
            .and_then(|tokens| tokens.get(&token_id))
            .map(dex_types::Balance::from)
            .unwrap_or_else(|| Balance::zero().into())
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
    InsufficientBalance {
        token: TokenId,
        available: Nat,
        required: Nat,
    },
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
            AddLimitOrderError::InsufficientBalance {
                token,
                available,
                required,
            } => dex_types::AddLimitOrderError::InsufficientBalance {
                token: token.into(),
                available,
                required,
            },
        }
    }
}
