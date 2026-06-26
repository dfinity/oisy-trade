pub mod audit;
pub mod event;
pub mod execution_policy;
mod map;
pub mod permissions;
pub mod snapshot;

pub use execution_policy::ExecutionPolicy;
pub use map::TradingPairMap;
pub use permissions::Permissions;
pub use snapshot::StateSnapshot;

#[cfg(test)]
mod tests;

use crate::Runtime;
use crate::Task;
use crate::Timestamp;
use crate::balance::{Balance, TokenBalance};
use crate::order::{
    CursorNotFound, FeeRates, Fill, FillSeq, FillSettlement, LotSize, MatchOrderError,
    MatchingOutput, NotionalError, Order, OrderBook, OrderBookId, OrderHistory, OrderId,
    OrderRecord, OrderSeq, OrderStatus, OrderUpdate, PendingOrder, Quantity, RemovedOrder,
    RemovedOrderSettlement, Side, TickSize, TokenId, TokenMetadata, Trade, TradeCursorNotFound,
    TradeHistory, TradeLeg, TradingPair,
};
use crate::storage::VMem;
use crate::user::{UserId, UserRegistry};
use candid::{Nat, Principal};
use ic_stable_structures::Memory;
use oisy_trade_types_internal::{InitArg, Mode};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::num::NonZeroU64;

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
    execution_policy: ExecutionPolicy,
    next_book_id: OrderBookId,
    tokens: BTreeMap<TokenId, TokenMetadata>,
    trading_pairs: TradingPairMap,
    order_books: BTreeMap<OrderBookId, OrderBook>,
    user_registry: UserRegistry<MB>,
    balances: TokenBalance<MB>,
    order_history: OrderHistory<MH>,
    fill_store: TradeHistory<MH>,
    /// Cached ledger transfer fees, learned from `BadFee` responses.
    /// Starts at 0 for unknown tokens; updated on the first withdrawal attempt.
    ledger_fee_cache: BTreeMap<TokenId, Nat>,
    /// [`event::SettlingEvent`]s awaiting dispatch. Written by producer
    /// steps (`record_matching_event` for matches, `record_cancel_limit_order`
    /// for cancels) and drained by the paired `SettlingEvent` dispatch in
    /// [`Self::record_settling_event`].
    pending_settling_events: VecDeque<event::SettlingEvent>,
    active_tasks: BTreeSet<Task>,
    /// Per-`(caller, token)` guard set for in-flight deposit/withdraw
    /// operations. Entries live only for the duration of a single async
    /// request and are reset on upgrade.
    in_flight_user_ops: BTreeSet<(Principal, TokenId)>,
    permissions: Permissions,
}

impl<MH: Memory, MB: Memory> State<MH, MB> {
    pub fn new(
        init_arg: InitArg,
        order_history: OrderHistory<MH>,
        fill_store: TradeHistory<MH>,
        user_registry: UserRegistry<MB>,
        balances: TokenBalance<MB>,
    ) -> Result<Self, String> {
        let execution_policy =
            ExecutionPolicy::try_new(init_arg.max_orders_per_chunk, init_arg.instruction_budget)?;
        Ok(Self {
            mode: init_arg.mode,
            execution_policy,
            next_book_id: OrderBookId::default(),
            tokens: BTreeMap::default(),
            trading_pairs: TradingPairMap::default(),
            order_books: BTreeMap::default(),
            user_registry,
            balances,
            order_history,
            fill_store,
            active_tasks: BTreeSet::default(),
            ledger_fee_cache: BTreeMap::default(),
            pending_settling_events: VecDeque::default(),
            in_flight_user_ops: BTreeSet::default(),
            permissions: Permissions::default(),
        })
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
    }

    pub fn execution_policy(&self) -> &ExecutionPolicy {
        &self.execution_policy
    }

    pub fn set_execution_policy(&mut self, policy: ExecutionPolicy) {
        self.execution_policy = policy;
    }

    pub fn permissions(&self) -> &Permissions {
        &self.permissions
    }

    pub fn permissions_mut(&mut self) -> &mut Permissions {
        &mut self.permissions
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

        // Settlement computes `maker_price × fill.quantity / 10^base_decimals`
        // regardless of the maker's side (see `Fill::quote_amount`), so both Buy
        // and Sell must satisfy `price × quantity ≤ u256::MAX`.
        let amount = pending
            .price
            .checked_mul_quantity_scaled(&pending.quantity, self.base_scale(&pair.base))
            .ok_or(AddLimitOrderError::AmountExceedsMaximum)?;

        book.check_notional(&amount)
            .map_err(|NotionalError { notional, min, max }| {
                AddLimitOrderError::InvalidNotional { notional, min, max }
            })?;

        let (token, required) = match pending.side {
            Side::Buy => (pair.quote, amount),
            Side::Sell => (pair.base, pending.quantity),
        };
        let free = self
            .user_registry
            .lookup(user)
            .and_then(|u| self.balances.get_balance(u, &token))
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
        timestamp: Timestamp,
        persistence: StableMemoryOptions,
    ) {
        let pair = self
            .trading_pairs
            .get_pair(&book_id)
            .expect("BUG: unknown trading pair");
        let (quote_token, base_token) = (pair.quote, pair.base);
        let base_scale = self.base_scale(&base_token);
        let book = self
            .order_books
            .get_mut(&book_id)
            .expect("BUG: order book missing");

        let (token, required) = match order.side() {
            Side::Buy => (
                quote_token,
                order
                    .price()
                    .checked_mul_quantity_scaled(order.remaining_quantity(), base_scale)
                    .expect("BUG: price * quantity overflow — already validated in validate_limit_order"),
            ),
            Side::Sell => (base_token, *order.remaining_quantity()),
        };

        // Balances and order_history both live in stable memory; replay
        // must skip both or it would double-reserve and re-insert.
        if matches!(persistence, StableMemoryOptions::Write) {
            let user_id = self
                .user_registry
                .lookup(user)
                .expect("BUG: order owner not registered — deposit registers every user");
            self.balances
                .reserve(user_id, &token, required)
                .expect("BUG: insufficient balance for validated order");

            let order_id = OrderId::new(book_id, order.id());
            self.order_history.insert_once(
                user_id,
                order_id,
                OrderRecord {
                    owner: user,
                    side: order.side(),
                    price: order.price(),
                    quantity: *order.remaining_quantity(),
                    filled_quantity: Quantity::ZERO,
                    status: OrderStatus::Pending,
                    created_at: timestamp,
                    last_updated_at: None,
                    time_in_force: order.time_in_force(),
                    filled_quote: Quantity::ZERO,
                    filled_fee: Quantity::ZERO,
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

        let permit = self.permissions().permit_cancel();
        audit::process_event(
            self,
            event::EventType::CancelLimitOrder(event::CancelLimitOrderEvent { order_id }),
            permit.into(),
            runtime,
        );

        // TODO(DEFI-2882): once PR #89's chunked execution lets matching
        // leave settling events queued across messages, draining the whole
        // queue here lets an unrelated cancel apply balance ops from a
        // previous matching round and inherit its instruction debt. Pop
        // only the event this cancel just pushed.
        while let Some(event) = self.take_next_pending_settling_event() {
            let permit = self.permissions().permit_settling();
            audit::process_event(
                self,
                event::EventType::Settling(event),
                permit.into(),
                runtime,
            );
        }

        let order = self
            .order_history
            .get(&order_id)
            .unwrap_or_else(|| panic!("BUG: order {order_id} not found after validation"));
        assert!(
            matches!(order.status, OrderStatus::Canceled),
            "BUG: order {order_id} not canceled"
        );
        Ok(order)
    }

    fn validate_cancel_limit_order(
        &self,
        caller: &Principal,
        order_id: &OrderId,
    ) -> Result<(), CancelLimitOrderError> {
        let record = self
            .order_history
            .get(order_id)
            .ok_or(CancelLimitOrderError::OrderNotFound)?;
        if &record.owner != caller {
            return Err(CancelLimitOrderError::NotOrderOwner);
        }
        match record.status {
            OrderStatus::Pending | OrderStatus::Open => Ok(()),
            OrderStatus::Filled | OrderStatus::Canceled | OrderStatus::Expired => {
                Err(CancelLimitOrderError::OrderAlreadyTerminal)
            }
        }
    }

    pub fn record_cancel_limit_order(
        &mut self,
        order_id: OrderId,
        now: Timestamp,
        persistence: StableMemoryOptions,
    ) {
        let (book_id, seq) = order_id.into_parts();
        let base_scale = self.base_scale_for_book(book_id);
        let book = self
            .order_books
            .get_mut(&book_id)
            .expect("BUG: order book missing for canceled order");
        let removed = book.remove_order(seq).expect(
            "BUG: canceled order request was validated, but canceled order not found in book",
        );
        if matches!(persistence, StableMemoryOptions::Write) {
            self.order_history.apply_update(
                &order_id,
                OrderUpdate::status(OrderStatus::Canceled),
                now,
            );
            let mut balance_operations = Vec::with_capacity(1);
            RemovedOrderSettlement::new(seq, &removed, base_scale)
                .push_balance_operations(&mut balance_operations);
            self.pending_settling_events
                .push_back(event::SettlingEvent {
                    book_id,
                    balance_operations,
                });
        }
    }

    /// Drive engine matching for the given book; when `persistence` is
    /// [`StableMemoryOptions::Write`], flip every touched order's status
    /// in `order_history`. Push the paired balance-only
    /// [`event::SettlingEvent`] (if any balance operations were produced)
    /// onto [`State::pending_settling_events`] for the settling-event
    /// dispatch to drain.
    pub fn record_matching_event(
        &mut self,
        event: &event::MatchingEvent,
        now: Timestamp,
        persistence: StableMemoryOptions,
    ) {
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("matching");
        let base_scale = self.base_scale_for_book(event.book_id);
        let book = self
            .order_books
            .get_mut(&event.book_id)
            .expect("BUG: trading pair registered but order book missing");
        let fee_rates = book.fee_rates();
        let output = book.process_pending_orders(&event.orders);

        if matches!(persistence, StableMemoryOptions::Write) {
            let MatchingOutput {
                fills,
                resting_orders,
                filled_orders,
                expired_orders,
            } = output;
            let (balance_operations, mut updates, trades) = settle(
                fills,
                &expired_orders,
                fee_rates,
                base_scale,
                event.book_id,
                now,
            );
            if !trades.is_empty() {
                let user_cache = resolve_op_users(
                    &event.book_id,
                    &balance_operations,
                    &self.order_history,
                    &self.user_registry,
                );
                for [taker_leg, maker_leg] in trades {
                    let taker_user = user_cache[&taker_leg.0.order_id().seq()];
                    let maker_user = user_cache[&maker_leg.0.order_id().seq()];
                    self.fill_store
                        .append(taker_leg, taker_user, maker_leg, maker_user);
                }
            }
            {
                #[cfg(feature = "canbench-rs")]
                let _p = canbench_rs::bench_scope("apply_order_updates");
                for seq in &resting_orders {
                    updates.entry(*seq).or_default().status = Some(OrderStatus::Open);
                }
                for seq in &filled_orders {
                    updates.entry(*seq).or_default().status = Some(OrderStatus::Filled);
                }
                for seq in expired_orders.keys() {
                    updates.entry(*seq).or_default().status = Some(OrderStatus::Expired);
                }
                for (seq, update) in updates {
                    let order_id = OrderId::new(event.book_id, seq);
                    self.order_history.apply_update(&order_id, update, now);
                }
            }
            if !balance_operations.is_empty() {
                self.pending_settling_events
                    .push_back(event::SettlingEvent {
                        book_id: event.book_id,
                        balance_operations,
                    });
            }
        }
    }

    /// Apply a declarative list of balance operations to `self.balances`.
    /// No-op under [`StableMemoryOptions::Skip`] (post-upgrade replay):
    /// the function's only side effect is on stable-memory-backed
    /// balances, which are preserved across upgrades.
    pub fn record_settling_event(
        &mut self,
        event: &event::SettlingEvent,
        persistence: StableMemoryOptions,
    ) {
        if matches!(persistence, StableMemoryOptions::Skip) {
            return;
        }

        let pair = self
            .trading_pairs
            .get_pair(&event.book_id)
            .cloned()
            .expect("BUG: unknown trading pair in SettlingEvent");
        #[cfg(feature = "canbench-rs")]
        let _p = canbench_rs::bench_scope("settling");
        // Resolve each distinct `OrderSeq` referenced by the operations to its
        // owner's `UserId` once (one `order_history` read + one registry lookup
        // per seq), then reuse from the map in the loop. A 1000-fill round can
        // reference ~2000 distinct seqs over ~3000 operations, so caching keeps
        // both stable maps out of the per-op hot path.
        let user_cache = resolve_op_users(
            &event.book_id,
            &event.balance_operations,
            &self.order_history,
            &self.user_registry,
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
                    fee,
                    ..
                } => {
                    self.balances.transfer(
                        user_cache[from_order],
                        user_cache[to_order],
                        &token,
                        *amount,
                        fee.unwrap_or(Quantity::ZERO),
                    );
                }
                event::BalanceOperation::Unreserve { order, amount, .. } => {
                    self.balances.unreserve(user_cache[order], &token, *amount);
                }
            }
        }
    }

    /// Returns up to `length` of `owner`'s orders, newest first, resuming
    /// strictly after the `after` order (a cursor from a prior page) — each
    /// paired with its trading pair and full record. An `after` that names an
    /// order the caller does not own (unknown or another principal's) yields
    /// [`CursorNotFound`]; a valid cursor with no older orders is `Ok(vec![])`.
    pub fn get_user_orders(
        &self,
        owner: &Principal,
        after: Option<OrderId>,
        length: usize,
    ) -> Result<Vec<(OrderId, TradingPair, OrderRecord)>, CursorNotFound> {
        let Some(user_id) = self.user_registry.lookup(*owner) else {
            return match after {
                Some(_) => Err(CursorNotFound),
                None => Ok(Vec::new()),
            };
        };
        Ok(self
            .order_history
            .orders_after(user_id, after, length)?
            .into_iter()
            .map(|id| {
                let record = self
                    .order_history
                    .get(&id)
                    .expect("BUG: per-user index references a missing order record");
                let pair = self
                    .trading_pairs
                    .get_pair(&id.book_id())
                    .expect("BUG: order references an unknown trading pair")
                    .clone();
                (id, pair, record)
            })
            .collect())
    }

    /// Returns the single order `id` paired with its trading pair and full
    /// record when `owner` placed it, or `None` if the id is unknown or owned
    /// by another principal.
    pub fn get_user_order(
        &self,
        owner: &Principal,
        id: OrderId,
    ) -> Option<(OrderId, TradingPair, OrderRecord)> {
        let record = self.order_history.get(&id)?;
        if &record.owner != owner {
            return None;
        }
        let pair = self
            .trading_pairs
            .get_pair(&id.book_id())
            .expect("BUG: order references an unknown trading pair")
            .clone();
        Some((id, pair, record))
    }

    /// Returns up to `length` of `owner`'s trades for the single order
    /// `order_id`, newest first, resuming strictly after the `after` cursor (a
    /// cursor from a prior page). Returns an empty page when `order_id` is
    /// unknown or owned by another principal. An `after` cursor that is not one
    /// of the order's trades yields [`TradeCursorNotFound`]; a valid cursor with
    /// no older trades is `Ok(vec![])`.
    pub fn get_user_order_fills(
        &self,
        owner: &Principal,
        order_id: OrderId,
        after: Option<FillSeq>,
        length: usize,
    ) -> Result<Vec<(FillSeq, Trade)>, TradeCursorNotFound> {
        let owns = self
            .order_history
            .get(&order_id)
            .is_some_and(|record| &record.owner == owner);
        if !owns {
            return Ok(Vec::new());
        }
        self.fill_store.trades_for_order(order_id, after, length)
    }

    pub fn next_book_id(&self) -> OrderBookId {
        self.next_book_id
    }

    pub fn has_trading_pair(&self, pair: &TradingPair) -> bool {
        self.trading_pairs.contains(pair)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_trading_pair(
        &mut self,
        book_id: OrderBookId,
        pair: TradingPair,
        base_metadata: TokenMetadata,
        quote_metadata: TokenMetadata,
        tick_size: TickSize,
        lot_size: LotSize,
        min_notional: Quantity,
        max_notional: Option<Quantity>,
        fee_rates: FeeRates,
    ) {
        self.record_token(pair.base, base_metadata);
        self.record_token(pair.quote, quote_metadata);
        assert_eq!(book_id, self.next_book_id, "BUG: order book ID mismatch");
        let book = OrderBook::new(
            book_id,
            tick_size,
            lot_size,
            min_notional,
            max_notional,
            fee_rates,
        );
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
    ) -> Result<(), oisy_trade_types::AddTradingPairError> {
        if let Some(existing) = self.tokens.get(&token_id)
            && existing != submitted
        {
            return Err(
                oisy_trade_types::AddTradingPairError::InconsistentTokenMetadata {
                    token: token_id.into(),
                    expected: existing.clone().into(),
                    submitted: submitted.clone().into(),
                },
            );
        }
        Ok(())
    }

    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    pub fn tokens(&self) -> &BTreeMap<TokenId, TokenMetadata> {
        &self.tokens
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

    /// `10^base_decimals` for the pair's base token — the divisor in
    /// `quote = price × quantity / base_scale`. Always ≥ 1 and ≤ 10^19 (the
    /// pair-creation invariant rejects larger base decimals), so it fits a
    /// `NonZeroU64`.
    pub(crate) fn base_scale(&self, base: &TokenId) -> NonZeroU64 {
        let decimals = self
            .token_metadata(base)
            .expect("BUG: trading pair registered but base token metadata missing")
            .decimals;
        let scale = 10u64.checked_pow(decimals as u32).expect(
            "BUG: 10^base_decimals fits u64 (base decimals ≤ 19 enforced at pair creation)",
        );
        NonZeroU64::new(scale).expect("BUG: 10^base_decimals is non-zero")
    }

    /// [`Self::base_scale`] for the base token of the pair behind `book_id`.
    pub(crate) fn base_scale_for_book(&self, book_id: OrderBookId) -> NonZeroU64 {
        let base_token = self
            .trading_pairs
            .get_pair(&book_id)
            .expect("BUG: unknown trading pair")
            .base;
        self.base_scale(&base_token)
    }

    pub fn withdraw(
        &mut self,
        user: Principal,
        token_id: TokenId,
        amount: Quantity,
    ) -> Result<(), crate::balance::InsufficientBalanceError> {
        // A user with no interned id has never deposited, so has no balance.
        let user_id =
            self.user_registry
                .lookup(user)
                .ok_or(crate::balance::InsufficientBalanceError {
                    available: Quantity::ZERO,
                    required: amount,
                })?;
        self.balances.withdraw(user_id, &token_id, amount)
    }

    pub fn iter_fee_balances(&self) -> impl Iterator<Item = (TokenId, Quantity)> + '_ {
        self.balances.iter_fee_balances()
    }

    /// Returns the canister-owned fee pool, shaped like [`get_balances`].
    /// - `None`: every token with a non-zero fee pool entry.
    /// - `Some(filter)`: one entry per requested token. The whole call fails
    ///   with a [`oisy_trade_types::GetBalancesError`] envelope carrying
    ///   [`oisy_trade_types::GetBalancesRequestError::TokenNotSupported`] under
    ///   `kind = RequestError` if any entry references an unsupported token;
    ///   registered tokens with no accrual return `Balance::ZERO`.
    pub fn get_fee_balances(
        &self,
        filter: Option<&[oisy_trade_types::FilterToken]>,
    ) -> Result<Vec<oisy_trade_types::UserTokenBalance>, oisy_trade_types::GetBalancesError> {
        match filter {
            Some(entries) => self.apply_filter(entries, |t| {
                fee_only_balance(self.balances.fee_balance(t).unwrap_or_default())
            }),
            None => Ok(self
                .balances
                .iter_fee_balances()
                .filter(|(_, amount)| !amount.is_zero())
                .map(|(token, amount)| {
                    let metadata = self
                        .tokens
                        .get(&token)
                        .expect("BUG: fee pool entry for unregistered token")
                        .clone();
                    oisy_trade_types::UserTokenBalance {
                        token: oisy_trade_types::Token {
                            id: token.into(),
                            metadata: metadata.into(),
                        },
                        balance: fee_only_balance(amount),
                    }
                })
                .collect()),
        }
    }

    /// Shared body for the `Some(filter)` branch of both [`Self::get_balances`]
    /// and [`Self::get_fee_balances`]: dedupe filter entries, look up the
    /// token in `self.tokens`, and resolve each entry's balance via the
    /// caller-supplied `balance_lookup`. Unknown tokens are reported as a
    /// [`oisy_trade_types::GetBalancesError`] envelope with
    /// `kind = RequestError(Some(TokenNotSupported(..)))`.
    fn apply_filter<F>(
        &self,
        filter: &[oisy_trade_types::FilterToken],
        balance_lookup: F,
    ) -> Result<Vec<oisy_trade_types::UserTokenBalance>, oisy_trade_types::GetBalancesError>
    where
        F: Fn(&TokenId) -> oisy_trade_types::Balance,
    {
        let mut seen: BTreeSet<oisy_trade_types::FilterToken> = BTreeSet::new();
        filter
            .iter()
            .filter(|ft| seen.insert((*ft).clone()))
            .map(|ft| {
                let internal_token = match ft {
                    oisy_trade_types::FilterToken::ById(t) => TokenId::from(t.clone()),
                };
                match self.tokens.get(&internal_token) {
                    None => Err(oisy_trade_types::GetBalancesError::request(
                        oisy_trade_types::GetBalancesRequestError::TokenNotSupported(ft.clone()),
                    )),
                    Some(metadata) => Ok(oisy_trade_types::UserTokenBalance {
                        token: oisy_trade_types::Token {
                            id: internal_token.into(),
                            metadata: metadata.clone().into(),
                        },
                        balance: balance_lookup(&internal_token),
                    }),
                }
            })
            .collect()
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
            let user_id = self.user_registry.get_or_register(user);
            self.balances.deposit(user_id, token_id, amount);
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
        self.user_registry
            .lookup(*user)
            .and_then(|u| self.balances.get_balance(u, token_id))
            .unwrap_or_default()
    }

    pub fn get_balances(
        &self,
        user: &Principal,
        filter: Option<&[oisy_trade_types::FilterToken]>,
    ) -> Result<Vec<oisy_trade_types::UserTokenBalance>, oisy_trade_types::GetBalancesError> {
        // `lookup` (not `intern`) so mere queriers don't pollute the registry.
        // `None` ⇒ the user has never held a balance, so every balance is zero.
        let user_id = self.user_registry.lookup(*user);
        match filter {
            Some(entries) => self.apply_filter(entries, |t| {
                user_id
                    .and_then(|u| self.balances.get_balance(u, t))
                    .unwrap_or_default()
                    .into()
            }),
            None => {
                let Some(user_id) = user_id else {
                    return Ok(Vec::new());
                };
                Ok(self
                    .tokens
                    .iter()
                    .filter_map(|(t, metadata)| {
                        self.balances
                            .get_balance(user_id, t)
                            .filter(|b| !b.is_zero())
                            .map(|b| oisy_trade_types::UserTokenBalance {
                                token: oisy_trade_types::Token {
                                    id: (*t).into(),
                                    metadata: metadata.clone().into(),
                                },
                                balance: b.into(),
                            })
                    })
                    .collect())
            }
        }
    }

    /// Set of currently active tasks to avoid parallel execution.
    pub fn active_tasks_mut(&mut self) -> &mut BTreeSet<Task> {
        &mut self.active_tasks
    }

    pub fn active_tasks(&self) -> &BTreeSet<Task> {
        &self.active_tasks
    }

    pub fn in_flight_user_ops_mut(&mut self) -> &mut BTreeSet<(Principal, TokenId)> {
        &mut self.in_flight_user_ops
    }

    pub fn in_flight_user_ops(&self) -> &BTreeSet<(Principal, TokenId)> {
        &self.in_flight_user_ops
    }

    pub fn trading_pair_count(&self) -> usize {
        self.trading_pairs.len()
    }

    pub fn get_order_book(&self, trading_pair: &TradingPair) -> Option<&OrderBook> {
        self.trading_pairs
            .get_book_id(trading_pair)
            .and_then(|book_id| self.order_books.get(book_id))
    }

    pub fn take_next_pending_settling_event(&mut self) -> Option<event::SettlingEvent> {
        self.pending_settling_events.pop_front()
    }

    pub fn order_books(&self) -> impl Iterator<Item = (&OrderBookId, &OrderBook)> {
        self.order_books.iter()
    }

    pub fn has_pending_orders(&self) -> bool {
        self.order_books()
            .any(|(_, book)| book.pending_orders_len() > 0)
    }

    pub fn has_pending_settling_events(&self) -> bool {
        !self.pending_settling_events.is_empty()
    }
}

/// Build a `OrderSeq -> Principal` map for every distinct seq referenced by
/// `ops`. Each `get` hits stable memory; callers can then look owners up in
/// the returned `BTreeMap` in O(log n) heap-only time.
fn resolve_op_users<MH: Memory, MB: Memory>(
    book_id: &OrderBookId,
    ops: &[event::BalanceOperation],
    history: &OrderHistory<MH>,
    registry: &UserRegistry<MB>,
) -> BTreeMap<OrderSeq, UserId> {
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
            let user = registry
                .lookup(owner)
                .expect("BUG: order owner not registered");
            (seq, user)
        })
        .collect()
}

fn settle(
    fills: Vec<Fill>,
    expired_orders: &BTreeMap<OrderSeq, RemovedOrder>,
    fee_rates: FeeRates,
    base_scale: NonZeroU64,
    book_id: OrderBookId,
    now: Timestamp,
) -> (
    Vec<event::BalanceOperation>,
    BTreeMap<OrderSeq, OrderUpdate>,
    Vec<[TradeLeg; 2]>,
) {
    let mut ops = Vec::with_capacity(fills.len() * 3 + expired_orders.len());
    let mut updates = BTreeMap::new();
    let mut trades = Vec::with_capacity(fills.len());
    for fill in fills {
        let settlement = FillSettlement::new(fill, fee_rates, base_scale);
        settlement.push_balance_operations(&mut ops);
        settlement.accrue_fill(&mut updates);
        trades.push(settlement.trades(book_id, now));
    }
    for (seq, removed) in expired_orders {
        RemovedOrderSettlement::new(*seq, removed, base_scale).push_balance_operations(&mut ops);
    }
    (ops, updates, trades)
}

/// `oisy_trade_types::Balance` carrying a fee amount in `free` and zero in
/// `reserved`. Fees have no reserved concept; the `Balance` shape is
/// reused to keep the `get_fee_balances` response identical in shape to
/// `get_balances`.
fn fee_only_balance(amount: Quantity) -> oisy_trade_types::Balance {
    oisy_trade_types::Balance {
        free: amount.into(),
        reserved: candid::Nat::from(0u64),
    }
}

#[cfg(test)]
impl Clone for State<ic_stable_structures::VectorMemory, ic_stable_structures::VectorMemory> {
    fn clone(&self) -> Self {
        let Self {
            mode,
            execution_policy,
            next_book_id,
            tokens,
            trading_pairs,
            order_books,
            user_registry,
            balances,
            active_tasks,
            ledger_fee_cache,
            order_history,
            fill_store,
            pending_settling_events,
            in_flight_user_ops,
            permissions,
        } = self;
        Self {
            mode: mode.clone(),
            execution_policy: execution_policy.clone(),
            next_book_id: *next_book_id,
            tokens: tokens.clone(),
            trading_pairs: trading_pairs.clone(),
            order_books: order_books.clone(),
            user_registry: user_registry.clone(),
            balances: balances.clone(),
            active_tasks: active_tasks.clone(),
            ledger_fee_cache: ledger_fee_cache.clone(),
            order_history: order_history.clone(),
            fill_store: fill_store.clone(),
            pending_settling_events: pending_settling_events.clone(),
            in_flight_user_ops: in_flight_user_ops.clone(),
            permissions: permissions.clone(),
        }
    }
}

#[cfg(test)]
impl PartialEq for State<ic_stable_structures::VectorMemory, ic_stable_structures::VectorMemory> {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            mode,
            execution_policy,
            next_book_id,
            tokens,
            trading_pairs,
            order_books,
            user_registry,
            balances,
            active_tasks,
            ledger_fee_cache,
            order_history,
            fill_store,
            pending_settling_events,
            in_flight_user_ops,
            permissions,
        } = self;
        let Self {
            mode: other_mode,
            execution_policy: other_execution_policy,
            next_book_id: other_next_book_id,
            tokens: other_tokens,
            trading_pairs: other_trading_pairs,
            order_books: other_order_books,
            user_registry: other_user_registry,
            balances: other_balances,
            active_tasks: other_active_tasks,
            ledger_fee_cache: other_ledger_fee_cache,
            order_history: other_order_history,
            fill_store: other_fill_store,
            pending_settling_events: other_pending_settling_events,
            in_flight_user_ops: other_in_flight_user_ops,
            permissions: other_permissions,
        } = other;
        mode == other_mode
            && execution_policy == other_execution_policy
            && next_book_id == other_next_book_id
            && tokens == other_tokens
            && trading_pairs == other_trading_pairs
            && order_books == other_order_books
            && user_registry == other_user_registry
            && balances == other_balances
            && active_tasks == other_active_tasks
            && ledger_fee_cache == other_ledger_fee_cache
            && order_history == other_order_history
            && fill_store == other_fill_store
            && pending_settling_events == other_pending_settling_events
            && in_flight_user_ops == other_in_flight_user_ops
            && permissions == other_permissions
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
    InvalidNotional {
        notional: Quantity,
        min: Quantity,
        max: Option<Quantity>,
    },
    TradingHalted,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CancelLimitOrderError {
    OrderNotFound,
    NotOrderOwner,
    OrderAlreadyTerminal,
}

impl From<CancelLimitOrderError> for oisy_trade_types::CancelLimitOrderError {
    fn from(err: CancelLimitOrderError) -> Self {
        use oisy_trade_types::CancelLimitOrderRequestError as Leaf;
        let leaf = match err {
            CancelLimitOrderError::OrderNotFound => Leaf::OrderNotFound,
            CancelLimitOrderError::NotOrderOwner => Leaf::NotOrderOwner,
            CancelLimitOrderError::OrderAlreadyTerminal => Leaf::OrderAlreadyTerminal,
        };
        oisy_trade_types::CancelLimitOrderError::request(leaf)
    }
}

impl From<permissions::UnauthorizedError> for AddLimitOrderError {
    fn from(err: permissions::UnauthorizedError) -> Self {
        match err {
            permissions::UnauthorizedError::TradingHalted => AddLimitOrderError::TradingHalted,
            permissions::UnauthorizedError::NotController => {
                unreachable!("permit_trading is not controller-gated")
            }
        }
    }
}

impl From<AddLimitOrderError> for oisy_trade_types::AddLimitOrderError {
    fn from(err: AddLimitOrderError) -> Self {
        use oisy_trade_types::AddLimitOrderRequestError as Req;
        use oisy_trade_types::AddLimitOrderTemporaryError as Tmp;
        match err {
            AddLimitOrderError::AmountExceedsMaximum => {
                oisy_trade_types::AddLimitOrderError::request(Req::AmountExceedsMaximum)
            }
            AddLimitOrderError::UnknownTradingPair => {
                oisy_trade_types::AddLimitOrderError::request(Req::UnknownTradingPair)
            }
            AddLimitOrderError::InvalidOrder(MatchOrderError::InvalidTickSize {
                price,
                tick_size,
            }) => oisy_trade_types::AddLimitOrderError::request(Req::InvalidPrice {
                price: candid::Nat::from(price),
                tick_size: candid::Nat::from(tick_size),
            }),
            AddLimitOrderError::InvalidOrder(MatchOrderError::InvalidLotSize {
                quantity,
                lot_size,
            }) => oisy_trade_types::AddLimitOrderError::request(Req::InvalidQuantity {
                quantity: quantity.into(),
                lot_size: candid::Nat::from(lot_size),
            }),
            AddLimitOrderError::InsufficientBalance {
                token,
                available,
                required,
            } => oisy_trade_types::AddLimitOrderError::request(Req::InsufficientBalance {
                token: oisy_trade_types::TokenId::from(token),
                available: available.into(),
                required: required.into(),
            }),
            AddLimitOrderError::InvalidNotional { notional, min, max } => {
                oisy_trade_types::AddLimitOrderError::request(Req::InvalidNotional {
                    notional: notional.into(),
                    min: min.into(),
                    max: max.map(Into::into),
                })
            }
            AddLimitOrderError::TradingHalted => {
                oisy_trade_types::AddLimitOrderError::temporary(Tmp::TradingHalted)
            }
        }
    }
}
