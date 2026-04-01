use crate::order::{
    Order, OrderBook, OrderId, PendingOrder, Price, Quantity, TokenId, TokenMetadata, TradingPair,
};
use candid::{Nat, Principal};
use dex_types::OrderStatus;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};

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
    #[allow(dead_code)] //TODO: DEFI-2730 process pending orders on a timer
    tokens: BTreeMap<TokenId, TokenMetadata>,
    #[allow(dead_code)] //TODO: DEFI-2730 process pending orders on a timer
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

    pub fn add_limit_order(&mut self, pending: PendingOrder) -> OrderId {
        let order_id = self.next_order_id();
        self.pending_orders.push_back(pending.into_order(order_id));
        order_id
    }

    pub fn get_order_status(&self, order_id: OrderId) -> OrderStatus {
        if self.pending_orders.iter().any(|o| o.id() == order_id) {
            OrderStatus::Pending
        } else {
            OrderStatus::NotFound
        }
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

    pub fn add_trading_pair(
        &mut self,
        pair: TradingPair,
        tick_size: Price,
        lot_size: Quantity,
    ) -> Result<(), dex_types::AddTradingPairError> {
        if tick_size.is_zero() {
            return Err(dex_types::AddTradingPairError::InvalidTickSize);
        }
        if lot_size.is_zero() {
            return Err(dex_types::AddTradingPairError::InvalidLotSize);
        }
        use std::collections::btree_map::Entry;
        match self.order_books.entry(pair) {
            Entry::Occupied(_) => Err(dex_types::AddTradingPairError::TradingPairAlreadyExists),
            Entry::Vacant(entry) => {
                entry.insert(OrderBook::new(tick_size, lot_size));
                Ok(())
            }
        }
    }
}
