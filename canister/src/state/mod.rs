pub mod audit;
pub mod event;
mod map;

pub use map::TradingPairMap;

#[cfg(test)]
mod tests;

use crate::Runtime;
use crate::Task;
use crate::balance::{Balance, TokenBalance};
use crate::order::{
    Fill, LotSize, MatchOrderError, Order, OrderBook, OrderBookId, OrderHistory, OrderId,
    OrderRecord, PendingOrder, Quantity, Side, TickSize, TokenId, TokenMetadata, TradingPair,
};
use candid::{Nat, Principal};
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
    trading_pairs: TradingPairMap,
    order_books: BTreeMap<OrderBookId, OrderBook>,
    // TODO(DEFI-2746): Add support for subaccounts.
    pub(crate) balances: TokenBalance,
    order_history: OrderHistory,
    active_tasks: BTreeSet<Task>,
    /// Cached ledger transfer fees, learned from `BadFee` responses.
    /// Starts at 0 for unknown tokens; updated on the first withdrawal attempt.
    ledger_fee_cache: BTreeMap<TokenId, Nat>,
}

impl TryFrom<InitArg> for State {
    type Error = String;

    fn try_from(init_arg: InitArg) -> Result<Self, Self::Error> {
        Ok(Self {
            mode: init_arg.mode,
            next_book_id: OrderBookId::default(),
            tokens: BTreeMap::default(),
            trading_pairs: TradingPairMap::default(),
            order_books: BTreeMap::default(),
            balances: TokenBalance::default(),
            order_history: OrderHistory::new(),
            active_tasks: BTreeSet::default(),
            ledger_fee_cache: BTreeMap::default(),
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

    pub fn validate_limit_order(
        &self,
        user: Principal,
        pair: TradingPair,
        pending: PendingOrder,
    ) -> Result<(OrderId, Order), AddLimitOrderError> {
        let book_id = *self
            .trading_pairs
            .get_book_id(&pair)
            .ok_or(AddLimitOrderError::UnknownTradingPair)?;
        let book = self
            .order_books
            .get(&book_id)
            .expect("BUG: trading pair registered but order book missing");

        book.validate_order(pending.price, &pending.quantity)
            .map_err(AddLimitOrderError::InvalidOrder)?;

        let (token, required) = match pending.side {
            Side::Buy => (pair.quote, pending.price.mul_quantity(&pending.quantity)),
            Side::Sell => (pair.base, pending.quantity.clone()),
        };
        let free = self.balances.get_free(&user, &token);
        if free < required {
            return Err(AddLimitOrderError::InsufficientBalance {
                token,
                available: free,
                required,
            });
        }

        let order_id = OrderId::new(book_id, book.next_seq());
        let order = pending.into_order(order_id.seq());
        Ok((order_id, order))
    }

    pub fn record_limit_order(&mut self, user: Principal, book_id: OrderBookId, order: Order) {
        let pair = self
            .trading_pairs
            .get_pair(&book_id)
            .expect("BUG: unknown trading pair");
        let book = self
            .order_books
            .get_mut(&book_id)
            .expect("BUG: order book missing");

        let (token, required) = match order.side() {
            Side::Buy => (
                pair.quote,
                order.price().mul_quantity(order.remaining_quantity()),
            ),
            Side::Sell => (pair.base, order.remaining_quantity().clone()),
        };
        self.balances
            .reserve(&user, &token, required)
            .expect("BUG: insufficient balance for validated order");

        let order_id = OrderId::new(book_id, order.id());
        self.order_history.insert_once(
            order_id,
            OrderRecord {
                owner: user,
                pair: pair.clone(),
                side: order.side(),
                price: order.price(),
                quantity: order.remaining_quantity().clone(),
                status: OrderStatus::Pending,
            },
        );
        book.add_pending_order(order);
    }

    pub fn process_pending_orders(&mut self) {
        // TODO DEFI-2743: chunk matching orders to avoid hitting the instruction limit.
        let pairs: Vec<(TradingPair, OrderBookId)> = self
            .trading_pairs
            .iter()
            .map(|(pair, book_id)| (pair.clone(), *book_id))
            .collect();
        for (pair, book_id) in pairs {
            let output = {
                #[cfg(feature = "canbench-rs")]
                let _p = canbench_rs::bench_scope("matching");
                let book = self
                    .order_books
                    .get_mut(&book_id)
                    .expect("BUG: trading pair registered but order book missing");

                book.process_pending_orders()
            };

            {
                #[cfg(feature = "canbench-rs")]
                let _p = canbench_rs::bench_scope("settling");
                for fill in &output.fills {
                    self.settle_fill(book_id, &pair, fill);
                }
            }
            {
                #[cfg(feature = "canbench-rs")]
                let _p = canbench_rs::bench_scope("status");
                for seq in &output.resting_orders {
                    let order_id = OrderId::new(book_id, *seq);
                    *self
                        .order_history
                        .get_status_mut(&order_id)
                        .expect("BUG: resting order not found in order_history") =
                        OrderStatus::Open;
                }
                for seq in &output.filled_orders {
                    let order_id = OrderId::new(book_id, *seq);
                    *self
                        .order_history
                        .get_status_mut(&order_id)
                        .expect("BUG: filled order not found in order_history") =
                        OrderStatus::Filled;
                }
            }
        }
    }

    pub(crate) fn settle_fill(&mut self, book_id: OrderBookId, pair: &TradingPair, fill: &Fill) {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("state::settle_fill");
        let taker = {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("state::order_history_get");
            self.order_history
                .get(&OrderId::new(book_id, fill.taker_order_seq))
                .expect("BUG: taker not found in order_history")
                .owner
        };
        let maker = {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("state::order_history_get");
            self.order_history
                .get(&OrderId::new(book_id, fill.maker_order_seq))
                .expect("BUG: maker not found in order_history")
                .owner
        };

        let (buyer, seller) = match fill.taker_side {
            Side::Buy => (taker, maker),
            Side::Sell => (maker, taker),
        };

        let quote_amount = fill.quote_amount();
        let base_amount = fill.base_amount().clone();

        // Quote side: buyer pays reserved, seller receives free
        {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("state::balance_mut");
            let quote = self.balances.token_mut(&pair.quote);
            quote.transfer(&buyer, &seller, quote_amount);
            // Unreserve buy-taker surplus (price improvement)
            if fill.taker_side == Side::Buy
                && let Some(price_diff) = fill.taker_price.checked_sub(fill.maker_price)
                && !price_diff.is_zero()
            {
                let surplus = price_diff.mul_quantity(&fill.quantity);
                quote.unreserve(&taker, surplus);
            }
        }

        // Base side: seller pays reserved, buyer receives free
        {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("state::balance_mut");
            self.balances
                .token_mut(&pair.base)
                .transfer(&seller, &buyer, base_amount);
        }
    }

    pub fn get_order_status(&self, order_id: OrderId) -> OrderStatus {
        self.order_history.get_status(&order_id)
    }

    pub fn next_book_id(&self) -> OrderBookId {
        self.next_book_id
    }

    pub fn has_trading_pair(&self, pair: &TradingPair) -> bool {
        self.trading_pairs.contains(pair)
    }

    pub fn record_trading_pair(
        &mut self,
        book_id: OrderBookId,
        pair: TradingPair,
        base_metadata: TokenMetadata,
        quote_metadata: TokenMetadata,
        tick_size: TickSize,
        lot_size: LotSize,
    ) {
        self.record_token(pair.base, base_metadata);
        self.record_token(pair.quote, quote_metadata);
        assert_eq!(book_id, self.next_book_id, "BUG: order book ID mismatch");
        let book = OrderBook::new(book_id, tick_size, lot_size);
        self.trading_pairs.insert(pair, book_id);
        assert_eq!(self.order_books.insert(book_id, book), None);
        self.next_book_id.increment();
    }

    pub fn record_token(&mut self, token_id: TokenId, metadata: TokenMetadata) {
        self.tokens
            .entry(token_id)
            .and_modify(|existing| assert_eq!(existing, &metadata))
            .or_insert(metadata);
    }

    pub fn check_token_metadata_consistency(
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

    pub fn trading_pairs(&self) -> &TradingPairMap {
        &self.trading_pairs
    }

    pub fn order_book(&self, id: &OrderBookId) -> Option<&OrderBook> {
        self.order_books.get(id)
    }

    pub fn is_known_token(&self, token_id: &TokenId) -> bool {
        self.tokens.contains_key(token_id)
    }

    pub fn token_metadata(&self, token_id: &TokenId) -> Option<&TokenMetadata> {
        self.tokens.get(token_id)
    }

    pub fn withdraw(
        &mut self,
        user: Principal,
        token_id: TokenId,
        amount: Quantity,
    ) -> Result<(), crate::balance::InsufficientBalanceError> {
        self.balances.withdraw(&user, &token_id, amount)
    }

    pub fn deposit(&mut self, user: Principal, token_id: TokenId, amount: Quantity) {
        self.balances.deposit(user, token_id, amount);
    }

    pub fn get_cached_ledger_fee(&self, token_id: &TokenId) -> Nat {
        self.ledger_fee_cache
            .get(token_id)
            .cloned()
            .unwrap_or(Nat::from(0u64))
    }

    pub fn set_cached_ledger_fee(&mut self, token_id: TokenId, fee: Nat) {
        self.ledger_fee_cache.insert(token_id, fee);
    }

    pub fn get_balance(&self, user: &Principal, token_id: &TokenId) -> Balance {
        self.balances
            .get_balance(user, token_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Set of currently active tasks to avoid parallel execution.
    pub fn active_tasks_mut(&mut self) -> &mut BTreeSet<Task> {
        &mut self.active_tasks
    }

    pub fn get_order_book(&self, trading_pair: &TradingPair) -> Option<&OrderBook> {
        self.trading_pairs
            .get_book_id(trading_pair)
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
