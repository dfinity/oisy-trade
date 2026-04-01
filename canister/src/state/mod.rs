use crate::order::{
    MatchOrderError, OrderBook, OrderId, PendingOrder, TokenId, TokenMetadata, TradingPair,
};
use candid::{Nat, Principal};
use dex_types::{OrderStatus, TradingPairInfo};
use std::cell::RefCell;
use std::collections::BTreeMap;

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
    #[allow(dead_code)] //TODO DEFI-2744: add trading pairs
    tokens: BTreeMap<TokenId, TokenMetadata>,
    order_books: BTreeMap<TradingPair, OrderBook>,
    // TODO(DEFI-2746): Add support for subaccounts.
    balances: BTreeMap<Principal, BTreeMap<TokenId, Nat>>,
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

    #[cfg(test)]
    pub fn add_trading_pair(&mut self, pair: TradingPair, order_book: OrderBook) {
        self.order_books.insert(pair, order_book);
    }

    pub fn get_trading_pairs(&self) -> Vec<TradingPairInfo> {
        self.order_books
            .iter()
            .map(|(pair, book)| TradingPairInfo {
                base_asset: dex_types::TokenId::from(pair.base),
                quote_asset: dex_types::TokenId::from(pair.quote),
                tick_size: book.tick_size().get(),
                lot_size: book.lot_size().get(),
            })
            .collect()
    }

    pub fn deposit(&mut self, user: Principal, token_id: TokenId, amount: Nat) {
        let balance = self
            .balances
            .entry(user)
            .or_default()
            .entry(token_id)
            .or_insert_with(|| Nat::from(0u64));
        *balance += amount;
    }

    pub fn get_balance(&self, user: Principal, token_id: TokenId) -> Nat {
        self.balances
            .get(&user)
            .and_then(|tokens| tokens.get(&token_id))
            .cloned()
            .unwrap_or(Nat::from(0u64))
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
