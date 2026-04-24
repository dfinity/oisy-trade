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
    self, CanceledOrderInfo, LotSize, MatchOrderError, MatchingOutput, Order, OrderBook,
    OrderBookId, OrderHistory, OrderId, OrderRecord, OrderSeq, OrderStatus, PairToken,
    PendingOrder, Quantity, RemovedOrder, Side, TickSize, TokenId, TokenMetadata, TradingPair,
};
use crate::storage::VMem;
use candid::{Nat, Principal};
use dex_types_internal::{InitArg, Mode};
use ic_stable_structures::Memory;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
    /// Cached ledger transfer fees, learned from `BadFee` responses.
    /// Starts at 0 for unknown tokens; updated on the first withdrawal attempt.
    ledger_fee_cache: BTreeMap<TokenId, Nat>,
    /// [`event::SettlingEvent`]s awaiting dispatch. Written by producer
    /// steps (`record_matching_event` for matches, `record_cancel_limit_order`
    /// for cancels) and drained by the paired `SettlingEvent` dispatch in
    /// [`Self::record_settling_event`]. Normally empty between messages —
    /// producers and their paired settling happen atomically in the same
    /// message (see `process_pending_orders` and `cancel_limit_order`).
    pending_settling_events: VecDeque<event::SettlingEvent>,
    active_tasks: BTreeSet<Task>,
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
            pending_settling_events: VecDeque::default(),
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

    pub fn cancel_limit_order(
        &mut self,
        user: &Principal,
        order_id: OrderId,
        runtime: &impl Runtime,
    ) -> Result<OrderRecord, CancelLimitOrderError> {
        self.validate_cancel_limit_order(user, &order_id)?;

        audit::process_event(
            self,
            event::EventType::CancelLimitOrder(event::CancelLimitOrderEvent { order_id }),
            runtime,
        );

        while let Some(event) = self.take_next_pending_settling_event() {
            audit::process_event(self, event::EventType::Settling(event), runtime);
        }

        let order = self
            .order_history
            .get(&order_id)
            .unwrap_or_else(|| panic!("BUG: order {order_id} not found after validation"));
        assert!(
            matches!(order.status, OrderStatus::Canceled(_)),
            "BUG: order {order_id} not canceled"
        );
        Ok(order)
    }

    pub fn validate_cancel_limit_order(
        &self,
        caller: &Principal,
        order_id: &OrderId,
    ) -> Result<(), CancelLimitOrderError> {
        let record = self
            .order_history
            .get(&order_id)
            .ok_or(CancelLimitOrderError::OrderNotFound)?;
        if &record.owner != caller {
            return Err(CancelLimitOrderError::NotOrderOwner);
        }
        match record.status {
            OrderStatus::Pending | OrderStatus::Open => Ok(()),
            OrderStatus::Filled => Err(CancelLimitOrderError::OrderAlreadyFilled),
            OrderStatus::Canceled(_) => Err(CancelLimitOrderError::OrderAlreadyCanceled),
        }
    }

    /// Book-side step of a cancel: remove the order from pending / resting
    /// and push the paired [`event::SettlingEvent`] (one `Unreserve` op and
    /// one `Canceled` transition) to [`State::pending_settling_events`] for
    /// the subsequent `SettlingEvent` dispatch to drain. Mirrors
    /// [`Self::record_matching_event`].
    pub fn record_cancel_limit_order(&mut self, order_id: OrderId) {
        let (book_id, seq) = order_id.into_parts();
        let book = self
            .order_books
            .get_mut(&book_id)
            .expect("BUG: order book missing for canceled order");
        // Unreachable: `validate_cancel_limit_order` rejects every status
        // except `Pending` and `Open`, and the book invariant guarantees those
        // statuses correspond to entries in `pending_orders` / `resting_orders`.
        let RemovedOrder {
            side,
            price,
            remaining_quantity,
        } = book
            .remove_order(seq)
            .expect("BUG: canceled order not found in book");
        let (refund_token, refund_amount) = match side {
            Side::Buy => (
                PairToken::Quote,
                price
                    .checked_mul_quantity(&remaining_quantity)
                    .expect("BUG: price * remaining overflow — validated at placement"),
            ),
            Side::Sell => (PairToken::Base, remaining_quantity),
        };
        self.pending_settling_events
            .push_back(event::SettlingEvent {
                book_id,
                balance_operations: vec![event::BalanceOperation::Unreserve {
                    order: seq,
                    token: refund_token,
                    amount: refund_amount,
                }],
                transitions: vec![event::OrderStatusTransition {
                    seq,
                    status: OrderStatus::Canceled(CanceledOrderInfo { remaining_quantity }),
                }],
            });
    }

    /// Take ownership of the next [`event::SettlingEvent`] pushed by a
    /// producer step (matching round or cancel book removal). Used by the
    /// live path to move the event into the dispatcher without cloning its
    /// `Vec<BalanceOperation>` / `Vec<OrderStatusTransition>`.
    /// [`Self::record_settling_event`]'s own `pop_front` drain becomes a
    /// no-op once the queue is emptied here.
    pub fn take_next_pending_settling_event(&mut self) -> Option<event::SettlingEvent> {
        self.pending_settling_events.pop_front()
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
            if !orders.is_empty() {
                audit::process_event(
                    self,
                    event::EventType::Matching(event::MatchingEvent { book_id, orders }),
                    runtime,
                );
            }

            while let Some(event) = self.take_next_pending_settling_event() {
                audit::process_event(self, event::EventType::Settling(event), runtime);
            }
        }
    }

    /// Drive engine matching for the given book and push the paired
    /// [`event::SettlingEvent`] to [`State::pending_settling_events`] for
    /// the follow-up settling dispatch to drain. If the round produced no
    /// ops and no transitions, nothing is pushed.
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
        let output = book.process_pending_orders(&event.orders);
        let balance_operations = compute_balance_operations(&output);
        let transitions = compute_order_status_transitions(&output);
        if !balance_operations.is_empty() || !transitions.is_empty() {
            self.pending_settling_events
                .push_back(event::SettlingEvent {
                    book_id: event.book_id,
                    balance_operations,
                    transitions,
                });
        }
    }

    /// Apply a declarative list of balance operations and order-status
    /// transitions.
    pub fn record_settling_event(
        &mut self,
        event: &event::SettlingEvent,
        persistence: StableMemoryOptions,
    ) {
        // Drain the paired entry pushed by the producer step (matching round
        // or cancel book removal). Pop unconditionally — the event being
        // dispatched is the canonical payload to apply.
        self.pending_settling_events.pop_front();

        if matches!(persistence, StableMemoryOptions::Skip) {
            return;
        }

        let pair = self
            .trading_pairs
            .get_pair(&event.book_id)
            .cloned()
            .expect("BUG: unknown trading pair in SettlingEvent");
        {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("settling");
            // Resolve each distinct `OrderSeq` referenced by the operations to
            // its owning `Principal` once, then reuse from the map in the
            // loop. A 1000-fill round can reference ~2000 distinct seqs over
            // ~3000 operations, so caching cuts stable-memory reads by ~35%.
            let owner_cache = resolve_op_owners(
                &event.book_id,
                &event.balance_operations,
                &self.order_history,
            );
            for op in &event.balance_operations {
                let token = pair.token(match op {
                    event::BalanceOperation::Transfer { token, .. }
                    | event::BalanceOperation::Unreserve { token, .. } => token,
                });
                match op {
                    event::BalanceOperation::Transfer {
                        from_order,
                        to_order,
                        amount,
                        ..
                    } => {
                        let from_owner = owner_cache[from_order];
                        let to_owner = owner_cache[to_order];
                        self.balances
                            .transfer(&from_owner, &to_owner, &token, *amount);
                    }
                    event::BalanceOperation::Unreserve { order, amount, .. } => {
                        let owner = owner_cache[order];
                        self.balances.unreserve(&owner, &token, *amount);
                    }
                }
            }
        }
        {
            #[cfg(feature = "canbench-rs")]
            let _p = canbench_rs::bench_scope("status");
            for transition in &event.transitions {
                let order_id = OrderId::new(event.book_id, transition.seq);
                self.order_history.set_status(&order_id, transition.status);
            }
        }
    }
}

/// Build a `OrderSeq -> Principal` map for every distinct seq referenced by
/// `ops`. Each `get` hits stable memory; callers can then look owners up in
/// the returned `BTreeMap` in O(log n) heap-only time.
fn resolve_op_owners<M: Memory>(
    book_id: &OrderBookId,
    ops: &[event::BalanceOperation],
    history: &OrderHistory<M>,
) -> BTreeMap<OrderSeq, Principal> {
    let mut seqs = BTreeSet::new();
    for op in ops {
        match op {
            event::BalanceOperation::Transfer {
                from_order,
                to_order,
                ..
            } => {
                seqs.insert(*from_order);
                seqs.insert(*to_order);
            }
            event::BalanceOperation::Unreserve { order, .. } => {
                seqs.insert(*order);
            }
        }
    }
    seqs.into_iter()
        .map(|seq| {
            let owner = history
                .get(&OrderId::new(*book_id, seq))
                .expect("BUG: missing order_history entry for BalanceOperation")
                .owner;
            (seq, owner)
        })
        .collect()
}

fn compute_balance_operations(output: &MatchingOutput) -> Vec<event::BalanceOperation> {
    let mut ops = Vec::with_capacity(output.fills.len() * 3);
    for fill in &output.fills {
        let (buyer_seq, seller_seq) = match fill.taker_side {
            Side::Buy => (fill.taker_order_seq, fill.maker_order_seq),
            Side::Sell => (fill.maker_order_seq, fill.taker_order_seq),
        };
        ops.push(event::BalanceOperation::Transfer {
            from_order: buyer_seq,
            to_order: seller_seq,
            token: order::PairToken::Quote,
            amount: fill.quote_amount(),
        });
        if fill.taker_side == Side::Buy
            && let Some(diff) = fill.taker_price.checked_sub(fill.maker_price)
            && !diff.is_zero()
        {
            let surplus = diff
                .checked_mul_quantity(&fill.quantity)
                .expect("BUG: price_diff * quantity overflow — validated in validate_limit_order");
            ops.push(event::BalanceOperation::Unreserve {
                order: fill.taker_order_seq,
                token: order::PairToken::Quote,
                amount: surplus,
            });
        }
        ops.push(event::BalanceOperation::Transfer {
            from_order: seller_seq,
            to_order: buyer_seq,
            token: order::PairToken::Base,
            amount: fill.quantity,
        });
    }
    ops
}

fn compute_order_status_transitions(output: &MatchingOutput) -> Vec<event::OrderStatusTransition> {
    output
        .resting_orders
        .iter()
        .map(|seq| event::OrderStatusTransition {
            seq: *seq,
            status: OrderStatus::Open,
        })
        .chain(
            output
                .filled_orders
                .iter()
                .map(|seq| event::OrderStatusTransition {
                    seq: *seq,
                    status: OrderStatus::Filled,
                }),
        )
        .collect()
}

impl<MH: Memory, MB: Memory> State<MH, MB> {
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
            pending_settling_events,
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
            pending_settling_events: pending_settling_events.clone(),
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
            pending_settling_events,
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
            pending_settling_events: other_pending_settling_events,
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
            && pending_settling_events == other_pending_settling_events
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

#[derive(Debug, PartialEq, Eq)]
pub enum CancelLimitOrderError {
    OrderNotFound,
    NotOrderOwner,
    OrderAlreadyFilled,
    OrderAlreadyCanceled,
}

impl From<CancelLimitOrderError> for dex_types::CancelLimitOrderError {
    fn from(err: CancelLimitOrderError) -> Self {
        match err {
            CancelLimitOrderError::OrderNotFound => dex_types::CancelLimitOrderError::OrderNotFound,
            CancelLimitOrderError::NotOrderOwner => dex_types::CancelLimitOrderError::NotOrderOwner,
            CancelLimitOrderError::OrderAlreadyFilled => {
                dex_types::CancelLimitOrderError::OrderAlreadyFilled
            }
            CancelLimitOrderError::OrderAlreadyCanceled => {
                dex_types::CancelLimitOrderError::OrderAlreadyCanceled
            }
        }
    }
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
