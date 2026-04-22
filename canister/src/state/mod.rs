pub mod audit;
pub mod event;
mod map;
pub mod snapshot;

pub use map::TradingPairMap;
pub use snapshot::StateSnapshot;

#[cfg(test)]
mod tests;

use crate::Runtime;
use crate::Task;
use crate::balance::{Balance, TokenBalance};
use crate::order::{
    Fill, LotSize, MatchOrderError, MatchingOutput, Order, OrderBook, OrderBookId, OrderHistory,
    OrderId, OrderRecord, OrderSeq, OrderStatus, PendingOrder, Quantity, Side, TickSize, TokenId,
    TokenMetadata, TradingPair,
};
use crate::storage::VMem;
use candid::{Nat, Principal};
use dex_types_internal::{InitArg, Mode};
use ic_stable_structures::Memory;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

thread_local! {
    static STATE: RefCell<Option<State<VMem, VMem>>> = RefCell::default();
}

pub fn with_state<R>(f: impl FnOnce(&State<VMem, VMem>) -> R) -> R {
    STATE.with(|s| f(s.borrow().as_ref().expect("State not initialized!")))
}

pub fn with_state_mut<R>(f: impl FnOnce(&mut State<VMem, VMem>) -> R) -> R {
    STATE.with(|s| f(s.borrow_mut().as_mut().expect("State not initialized!")))
}

pub fn init_state(state: State<VMem, VMem>) {
    STATE.with(|s| {
        let mut current = s.borrow_mut();
        assert!(current.is_none(), "State already initialized!");
        *current = Some(state);
    });
}

/// Clears the thread-local state. Used by benchmarks (and tests) to simulate a
/// canister restart between `pre_upgrade` and `post_upgrade` calls so that
/// `init_state` can be invoked a second time without tripping its assertion.
#[cfg(any(test, feature = "canbench-rs"))]
pub fn reset_state() {
    STATE.with(|s| *s.borrow_mut() = None);
}

/// Controls whether a state mutation propagates to stable-memory-backed
/// structures. Normal execution uses [`StableMemoryOptions::Write`]; the
/// `post_upgrade` replay uses [`StableMemoryOptions::Skip`] because stable
/// storage already holds the post-mutation values — re-inserting them
/// would either duplicate entries or overwrite newer states with the
/// values the event carries at submission time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StableMemoryOptions {
    Write,
    Skip,
}

#[derive(Debug)]
pub struct State<MH: Memory, MB: Memory> {
    mode: Mode,
    next_book_id: OrderBookId,
    tokens: BTreeMap<TokenId, TokenMetadata>,
    trading_pairs: TradingPairMap,
    order_books: BTreeMap<OrderBookId, OrderBook>,
    // TODO(DEFI-2746): Add support for subaccounts.
    balances: TokenBalance<MB>,
    order_history: OrderHistory<MH>,
    active_tasks: BTreeSet<Task>,
    /// Cached ledger transfer fees, learned from `BadFee` responses.
    /// Starts at 0 for unknown tokens; updated on the first withdrawal attempt.
    ledger_fee_cache: BTreeMap<TokenId, Nat>,
    /// Matching outputs awaiting their `SettlingEvent`, keyed by book.
    /// Populated by `record_matching_event`; drained by
    /// `record_settling_event`. Normally empty between messages because
    /// matching and settling happen atomically inside
    /// `process_pending_orders`; carried in the snapshot anyway so a
    /// half-round state (e.g. a trap between the two events) is recoverable
    /// across upgrades.
    pending_settlement: BTreeMap<OrderBookId, MatchingOutput>,
}

impl<MH: Memory, MB: Memory> State<MH, MB> {
    pub fn new(
        init_arg: InitArg,
        order_history: OrderHistory<MH>,
        balances: TokenBalance<MB>,
    ) -> Result<Self, String> {
        Ok(Self {
            mode: init_arg.mode,
            next_book_id: OrderBookId::default(),
            tokens: BTreeMap::default(),
            trading_pairs: TradingPairMap::default(),
            order_books: BTreeMap::default(),
            balances,
            order_history,
            active_tasks: BTreeSet::default(),
            ledger_fee_cache: BTreeMap::default(),
            pending_settlement: BTreeMap::default(),
        })
    }

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

        // Settlement computes `maker_price × fill.quantity` regardless of the
        // maker's side (see `Fill::quote_amount`), so both Buy and Sell must
        // satisfy `price × quantity ≤ u256::MAX`.
        let amount = pending
            .price
            .checked_mul_quantity(&pending.quantity)
            .ok_or(AddLimitOrderError::AmountExceedsMaximum)?;

        let (token, required) = match pending.side {
            Side::Buy => (pair.quote, amount),
            Side::Sell => (pair.base, pending.quantity),
        };
        let free = self
            .balances
            .get_balance(&user, &token)
            .map(|b| *b.free())
            .unwrap_or(Quantity::ZERO);
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

    pub fn record_limit_order(
        &mut self,
        user: Principal,
        book_id: OrderBookId,
        order: Order,
        persistence: StableMemoryOptions,
    ) {
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
                order
                    .price()
                    .checked_mul_quantity(order.remaining_quantity())
                    .expect("BUG: price * quantity overflow — already validated in validate_limit_order"),
            ),
            Side::Sell => (pair.base, *order.remaining_quantity()),
        };

        // Balances and order_history both live in stable memory; replay
        // must skip both or it would double-reserve and re-insert.
        if matches!(persistence, StableMemoryOptions::Write) {
            self.balances
                .reserve(&user, &token, required)
                .expect("BUG: insufficient balance for validated order");

            let order_id = OrderId::new(book_id, order.id());
            self.order_history.insert_once(
                order_id,
                OrderRecord {
                    owner: user,
                    side: order.side(),
                    price: order.price(),
                    quantity: *order.remaining_quantity(),
                    status: OrderStatus::Pending,
                },
            );
        }
        book.add_pending_order(order);
    }

    pub fn process_pending_orders(&mut self, runtime: &impl Runtime) {
        // TODO DEFI-2743: chunk matching orders to avoid hitting the instruction limit.
        let book_ids: Vec<OrderBookId> = self
            .trading_pairs
            .iter()
            .map(|(_, book_id)| *book_id)
            .collect();
        for book_id in book_ids {
            let orders: Vec<OrderSeq> = {
                let book = self
                    .order_books
                    .get(&book_id)
                    .expect("BUG: trading pair registered but order book missing");
                book.pending_order_seqs().collect()
            };
            if orders.is_empty() {
                continue;
            }

            // Matching event drives the engine via `record_matching_event`
            // and parks the output in `state.pending_settlement`.
            audit::process_event(
                self,
                event::EventType::Matching(event::MatchingEvent { book_id, orders }),
                runtime,
            );

            // Read the output back out to build SettlingEvent; the event
            // carries it on the wire for audit-log completeness, while
            // `record_settling_event` will drain and verify against the
            // stored value.
            let output = self
                .pending_settlement
                .get(&book_id)
                .cloned()
                .expect("BUG: Matching apply did not park a pending settlement");
            audit::process_event(
                self,
                event::EventType::Settling(event::SettlingEvent { book_id, output }),
                runtime,
            );
        }
    }

    /// Drive engine matching for the given book and park the resulting
    /// [`MatchingOutput`] in [`State::pending_settlement`] for the paired
    /// [`event::SettlingEvent`] to drain. Called from
    /// [`audit::apply_state_transition`] for `EventType::Matching` on both
    /// the primary path and replay — the behaviour is identical in both.
    pub fn record_matching_event(
        &mut self,
        event: &event::MatchingEvent,
        _persistence: StableMemoryOptions,
    ) {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("matching");
        let book = self
            .order_books
            .get_mut(&event.book_id)
            .expect("BUG: trading pair registered but order book missing");
        let pending: Vec<OrderSeq> = book.pending_order_seqs().collect();
        assert_eq!(
            pending, event.orders,
            "BUG: pending queue diverges from MatchingEvent.orders for book {:?}",
            event.book_id,
        );
        let output = book.process_pending_orders();
        // Park the output for the paired `SettlingEvent` to drain. Insert
        // overwrites any stale entry rather than asserting `None`: a missing
        // prior `SettlingEvent` (e.g., a skipped settle round in some future
        // code path) should not turn into a hard trap here.
        self.pending_settlement.insert(event.book_id, output);
    }

    /// Apply settlement and status transitions from a single matching round.
    pub fn record_settling_event(
        &mut self,
        event: &event::SettlingEvent,
        persistence: StableMemoryOptions,
    ) {
        // Drain the bridge if present. If a preceding `MatchingEvent` parked
        // an output, verify it matches the event — the output on the wire is
        // authoritative for settlement, so we don't panic if `pending_settlement`
        // is empty (e.g., on out-of-order replay or partial history).
        if let Some(stored) = self.pending_settlement.remove(&event.book_id) {
            assert_eq!(
                stored, event.output,
                "BUG: SettlingEvent.output diverges from pending_settlement for book {:?}",
                event.book_id,
            );
        }

        let pair = self
            .trading_pairs
            .get_pair(&event.book_id)
            .cloned()
            .expect("BUG: unknown trading pair in SettlingEvent");
        {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("settling");
            for fill in &event.output.fills {
                self.settle_fill(event.book_id, &pair, fill, persistence);
            }
        }
        {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("status");
            if matches!(persistence, StableMemoryOptions::Write) {
                for seq in &event.output.resting_orders {
                    let order_id = OrderId::new(event.book_id, *seq);
                    self.order_history.set_status(&order_id, OrderStatus::Open);
                }
                for seq in &event.output.filled_orders {
                    let order_id = OrderId::new(event.book_id, *seq);
                    self.order_history
                        .set_status(&order_id, OrderStatus::Filled);
                }
            }
        }
    }

    pub(crate) fn settle_fill(
        &mut self,
        book_id: OrderBookId,
        pair: &TradingPair,
        fill: &Fill,
        persistence: StableMemoryOptions,
    ) {
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
        let base_amount = *fill.base_amount();

        // `StableMemoryOptions::Skip` gates only the stable-memory writes
        // (balance transfers); lookups and computations above still run so
        // replay mirrors the production code path for debugging.
        let write_balances = matches!(persistence, StableMemoryOptions::Write);

        // Quote side: buyer pays reserved, seller receives free
        {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("state::balance_update");
            if write_balances {
                self.balances
                    .transfer(&buyer, &seller, &pair.quote, quote_amount);
            }
            // Unreserve buy-taker surplus (price improvement)
            if fill.taker_side == Side::Buy
                && let Some(price_diff) = fill.taker_price.checked_sub(fill.maker_price)
                && !price_diff.is_zero()
            {
                let surplus = price_diff
                    .checked_mul_quantity(&fill.quantity)
                    .expect("BUG: price_diff * quantity overflow — already validated in validate_limit_order");
                if write_balances {
                    self.balances.unreserve(&taker, &pair.quote, surplus);
                }
            }
        }

        // Base side: seller pays reserved, buyer receives free
        {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("state::balance_update");
            if write_balances {
                self.balances
                    .transfer(&seller, &buyer, &pair.base, base_amount);
            }
        }
    }

    pub fn get_order_status(&self, order_id: OrderId) -> Option<OrderStatus> {
        self.order_history.get(&order_id).map(|r| r.status)
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

    /// Credits `amount` to the user's free balance.
    pub fn deposit(
        &mut self,
        user: Principal,
        token_id: TokenId,
        amount: Quantity,
        persistence: StableMemoryOptions,
    ) {
        if matches!(persistence, StableMemoryOptions::Write) {
            self.balances.deposit(user, token_id, amount);
        }
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

#[cfg(test)]
impl Clone for State<ic_stable_structures::VectorMemory, ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let Self {
            mode,
            next_book_id,
            tokens,
            trading_pairs,
            order_books,
            balances,
            active_tasks,
            ledger_fee_cache,
            order_history,
            pending_settlement,
        } = self;
        Self {
            mode: mode.clone(),
            next_book_id: *next_book_id,
            tokens: tokens.clone(),
            trading_pairs: trading_pairs.clone(),
            order_books: order_books.clone(),
            balances: balances.clone(),
            active_tasks: active_tasks.clone(),
            ledger_fee_cache: ledger_fee_cache.clone(),
            order_history: order_history.clone(),
            pending_settlement: pending_settlement.clone(),
        }
    }
}

#[cfg(test)]
impl PartialEq for State<ic_stable_structures::VectorMemory, ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            mode,
            next_book_id,
            tokens,
            trading_pairs,
            order_books,
            balances,
            active_tasks,
            ledger_fee_cache,
            order_history,
            pending_settlement,
        } = self;
        let Self {
            mode: other_mode,
            next_book_id: other_next_book_id,
            tokens: other_tokens,
            trading_pairs: other_trading_pairs,
            order_books: other_order_books,
            balances: other_balances,
            active_tasks: other_active_tasks,
            ledger_fee_cache: other_ledger_fee_cache,
            order_history: other_order_history,
            pending_settlement: other_pending_settlement,
        } = other;
        mode == other_mode
            && next_book_id == other_next_book_id
            && tokens == other_tokens
            && trading_pairs == other_trading_pairs
            && order_books == other_order_books
            && balances == other_balances
            && active_tasks == other_active_tasks
            && ledger_fee_cache == other_ledger_fee_cache
            && order_history == other_order_history
            && pending_settlement == other_pending_settlement
    }
}

#[cfg(test)]
impl Eq for State<ic_stable_structures::VectorMemory, ic_stable_structures::VectorMemory> {}

#[derive(Debug, PartialEq, Eq)]
pub enum AddLimitOrderError {
    AmountExceedsMaximum,
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
            AddLimitOrderError::AmountExceedsMaximum => {
                dex_types::AddLimitOrderError::AmountExceedsMaximum
            }
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
