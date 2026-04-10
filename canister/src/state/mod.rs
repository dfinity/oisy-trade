pub mod event;

#[cfg(test)]
mod tests;

use crate::Runtime;
use crate::Task;
use crate::balance::Balance;
use crate::order::{
    Fill, LotSize, MatchOrderError, OrderBook, OrderBookId, OrderId, PendingOrder, Quantity, Side,
    TickSize, TokenId, TokenMetadata, TradingPair,
};
use candid::Principal;
use dex_types::OrderStatus;
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

pub fn init_state(state: State) {
    STATE.with(|s| {
        let mut current = s.borrow_mut();
        assert!(current.is_none(), "State already initialized!");
        *current = Some(state);
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    mode: Mode,
    next_book_id: OrderBookId,
    tokens: BTreeMap<TokenId, TokenMetadata>,
    trading_pairs: BTreeMap<TradingPair, OrderBookId>,
    order_books: BTreeMap<OrderBookId, OrderBook>,
    // TODO(DEFI-2746): Add support for subaccounts.
    balances: BTreeMap<Principal, BTreeMap<TokenId, Balance>>,
    // TODO DEFI-2752: Keep track of filled orders.
    order_owners: BTreeMap<OrderId, Principal>,
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
            order_owners: BTreeMap::default(),
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

        book.validate_order(pending.price, &pending.quantity)
            .map_err(AddLimitOrderError::InvalidOrder)?;

        let (token, required) = match pending.side {
            Side::Buy => (pair.quote, pending.price.mul_quantity(&pending.quantity)),
            Side::Sell => (pair.base, pending.quantity.clone()),
        };
        match self
            .balances
            .get_mut(&user)
            .and_then(|tokens| tokens.get_mut(&token))
        {
            Some(balance) => {
                balance
                    .reserve(required)
                    .map_err(|e| AddLimitOrderError::InsufficientBalance {
                        token,
                        available: e.available,
                        required: e.required,
                    })?;
            }
            None => {
                return Err(AddLimitOrderError::InsufficientBalance {
                    token,
                    available: Quantity::ZERO,
                    required,
                });
            }
        }

        let order_id = book
            .add_pending_order(pending)
            .map_err(AddLimitOrderError::InvalidOrder)?;
        assert_eq!(self.order_owners.insert(order_id, user), None);
        Ok(order_id)
    }

    pub fn process_pending_orders(&mut self) {
        // TODO DEFI-2743: chunk matching orders to avoid hitting the instruction limit.
        let pairs: Vec<(TradingPair, OrderBookId)> = self
            .trading_pairs
            .iter()
            .map(|(pair, &book_id)| (pair.clone(), book_id))
            .collect();
        for (pair, book_id) in pairs {
            let fills = {
                #[cfg(feature = "canbench-rs")]
                let _p = canbench_rs::bench_scope("matching");
                self.order_books
                    .get_mut(&book_id)
                    .expect("BUG: trading pair registered but order book missing")
                    .process_pending_orders()
            };
            {
                #[cfg(feature = "canbench-rs")]
                let _p = canbench_rs::bench_scope("settling");
                for fill in &fills {
                    self.settle_fill(book_id, &pair, fill);
                }
            }
        }
    }

    fn settle_fill(&mut self, book_id: OrderBookId, pair: &TradingPair, fill: &Fill) {
        let taker = *self
            .order_owners
            .get(&OrderId::new(book_id, fill.taker_order_seq))
            .expect("BUG: taker not found in order_owners");
        let maker = *self
            .order_owners
            .get(&OrderId::new(book_id, fill.maker_order_seq))
            .expect("BUG: maker not found in order_owners");

        let (buyer, seller) = match fill.taker_side {
            Side::Buy => (taker, maker),
            Side::Sell => (maker, taker),
        };

        let quote_amount = fill.maker_price.mul_quantity(&fill.quantity);
        let base_amount = fill.quantity.clone();

        // Buyer: pay quote, receive base
        self.balance_mut(buyer, pair.quote)
            .debit_reserved(quote_amount.clone());
        self.balance_mut(buyer, pair.base)
            .deposit(base_amount.clone());

        // Seller: pay base, receive quote
        self.balance_mut(seller, pair.base)
            .debit_reserved(base_amount);
        self.balance_mut(seller, pair.quote).deposit(quote_amount);

        // Unreserve buy-taker surplus (price improvement):
        // the buyer reserved `taker_price * quantity` of quote tokens but filled at
        // the lower or equal `maker_price`, so the difference must move back to free.
        //
        // Sell takers have no surplus because they reserve base quantity only,
        // which is price-independent. They see the price improvement in the deposit
        // of quote tokens (maker_price * quantity instead of taker_price * quantity, where
        // in the case of sell maker_price >= taker_price).
        if fill.taker_side == Side::Buy
            && let Some(price_diff) = fill.taker_price.checked_sub(fill.maker_price)
            && !price_diff.is_zero()
        {
            let surplus = price_diff.mul_quantity(&fill.quantity);
            self.balance_mut(taker, pair.quote).unreserve(surplus);
        }
    }

    fn balance_mut(&mut self, user: Principal, token: TokenId) -> &mut Balance {
        self.balances
            .entry(user)
            .or_default()
            .entry(token)
            .or_default()
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
    ///
    /// Also validates and stores the token metadata for both the base and quote
    /// tokens. If a token is already registered with different metadata, returns
    /// [`AddTradingPairError::InconsistentTokenMetadata`].
    pub fn add_trading_pair(
        &mut self,
        pair: TradingPair,
        base_metadata: TokenMetadata,
        quote_metadata: TokenMetadata,
        tick_size: TickSize,
        lot_size: LotSize,
    ) -> Result<(), dex_types::AddTradingPairError> {
        self.check_token_metadata_consistency(pair.base, &base_metadata)?;
        self.check_token_metadata_consistency(pair.quote, &quote_metadata)?;
        if self.trading_pairs.contains_key(&pair) {
            return Err(dex_types::AddTradingPairError::TradingPairAlreadyExists);
        }
        self.tokens.entry(pair.base).or_insert(base_metadata);
        self.tokens.entry(pair.quote).or_insert(quote_metadata);
        let book_id = self.next_book_id();
        let book = OrderBook::new(book_id, tick_size, lot_size);
        assert_eq!(self.trading_pairs.insert(pair, book_id), None);
        assert_eq!(self.order_books.insert(book_id, book), None);
        Ok(())
    }

    fn check_token_metadata_consistency(
        &self,
        token_id: TokenId,
        submitted: &TokenMetadata,
    ) -> Result<(), dex_types::AddTradingPairError> {
        if let Some(existing) = self.tokens.get(&token_id)
            && existing != submitted
        {
            return Err(dex_types::AddTradingPairError::InconsistentTokenMetadata {
                token: token_id.into(),
                expected: existing.clone().into(),
                submitted: submitted.clone().into(),
            });
        }
        Ok(())
    }

    pub fn trading_pairs(&self) -> &BTreeMap<TradingPair, OrderBookId> {
        &self.trading_pairs
    }

    pub fn order_book(&self, id: &OrderBookId) -> Option<&OrderBook> {
        self.order_books.get(id)
    }

    pub fn token_metadata(&self, token_id: &TokenId) -> Option<&TokenMetadata> {
        self.tokens.get(token_id)
    }

    pub fn deposit(&mut self, user: Principal, token_id: TokenId, amount: Quantity) {
        self.balances
            .entry(user)
            .or_default()
            .entry(token_id)
            .or_default()
            .deposit(amount);
    }

    pub fn get_balance(&self, user: &Principal, token_id: &TokenId) -> Balance {
        self.balances
            .get(user)
            .and_then(|tokens| tokens.get(token_id))
            .cloned()
            .unwrap_or_default()
    }

    /// Set of currently active tasks to avoid parallel execution.
    pub fn active_tasks_mut(&mut self) -> &mut BTreeSet<Task> {
        &mut self.active_tasks
    }

    pub fn get_order_book(&self, trading_pair: &TradingPair) -> Option<&OrderBook> {
        self.trading_pairs
            .get(trading_pair)
            .and_then(|book_id| self.order_books.get(book_id))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddLimitOrderError {
    UnknownTradingPair,
    InvalidOrder(MatchOrderError),
    InsufficientBalance {
        token: TokenId,
        available: Quantity,
        required: Quantity,
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
                quantity: quantity.into(),
                lot_size: lot_size.get(),
            },
            AddLimitOrderError::InsufficientBalance {
                token,
                available,
                required,
            } => dex_types::AddLimitOrderError::InsufficientBalance {
                token: dex_types::TokenId::from(token),
                available: available.into(),
                required: required.into(),
            },
        }
    }
}
