use super::*;
use crate::balance::Balance;
use crate::order::{
    FeeRates, OrderBookId, OrderId, OrderStatus, PairToken, PendingOrder, Price, Quantity, Side,
    TimeInForce, TokenId, TradingPair,
};
use crate::state::StableMemoryOptions;
use crate::state::event::{
    AddLimitOrderEvent, AddTradingAccountEvent, BalanceOperation, CancelLimitOrderEvent,
    DepositEvent, MatchingEvent, RemoveTradingAccountEvent, WithdrawEvent,
};
use crate::test_fixtures::event::{add_trading_pair_event, init_event, upgrade_event};
use crate::test_fixtures::{
    LOT_SIZE, MAX_NOTIONAL, MIN_NOTIONAL, PRICE_SCALE, TICK_SIZE, balances, base_metadata, maker,
    order_history, quote_metadata, state, trade_history, user_registry,
};
use candid::Principal;
use ic_stable_structures::VectorMemory;
use oisy_trade_types_internal::Mode;

const BASE: [u8; 1] = [0x01];
const QUOTE: [u8; 1] = [0x02];
const USER_1: [u8; 1] = [0x03];
const USER_2: [u8; 1] = [0x04];

fn base() -> Principal {
    Principal::from_slice(&BASE)
}
fn quote() -> Principal {
    Principal::from_slice(&QUOTE)
}
fn user_1() -> Principal {
    Principal::from_slice(&USER_1)
}
fn user_2() -> Principal {
    Principal::from_slice(&USER_2)
}
fn trading_pair() -> TradingPair {
    TradingPair {
        base: TokenId::new(base()),
        quote: TokenId::new(quote()),
    }
}

/// Builds a `normal` state via the primary path and, in lockstep, the event
/// log that replay is expected to consume. `assert_replay_matches` feeds that
/// log into `replay_events` and asserts the reconstructed `State` equals the
/// primary-path `State`.
///
/// Timestamps start at 3 to sit above the reserved slots used by the
/// `init_event` / `upgrade_event` / `add_trading_pair_event` fixtures
/// (0, 1, 2). The exact values don't affect replay equivalence — `State`
/// doesn't embed timestamps — but keeping them monotonic matches the shape
/// of real event logs.
struct Scenario {
    state: crate::state::State<VectorMemory, VectorMemory>,
    events: Vec<Event>,
    next_ts: u64,
}

impl Scenario {
    /// Starts from `state()` (Init-only, no trading pair). Callers layer
    /// upgrades / pairs / deposits / orders / matching on top.
    fn new() -> Self {
        Self {
            state: state(),
            events: vec![init_event(Mode::GeneralAvailability)],
            next_ts: 3,
        }
    }

    fn timestamp(&mut self) -> crate::Timestamp {
        let ts = self.next_ts;
        self.next_ts += 1;
        crate::Timestamp::new(ts)
    }

    fn with_upgrade(mut self, mode: Option<Mode>) -> Self {
        if let Some(ref m) = mode {
            self.state.set_mode(m.clone());
        }
        self.events.push(upgrade_event(mode, None, None, None));
        self
    }

    fn with_upgrade_execution_policy(
        mut self,
        max_orders_per_chunk: u32,
        instruction_budget: u64,
        max_settlement_units_per_event: u32,
    ) -> Self {
        self.state.set_execution_policy(
            crate::state::ExecutionPolicy::try_new(
                max_orders_per_chunk,
                instruction_budget,
                max_settlement_units_per_event,
            )
            .unwrap(),
        );
        self.events.push(upgrade_event(
            None,
            Some(max_orders_per_chunk),
            Some(instruction_budget),
            Some(max_settlement_units_per_event),
        ));
        self
    }

    fn with_trading_pair(mut self) -> Self {
        self.state.record_trading_pair(
            OrderBookId::ZERO,
            trading_pair(),
            base_metadata(),
            quote_metadata(),
            TICK_SIZE,
            LOT_SIZE,
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            FeeRates::default(),
        );
        self.events.push(add_trading_pair_event(base(), quote()));
        self
    }

    fn with_deposit(mut self, user: Principal, token: TokenId, amount: Quantity) -> Self {
        self.state
            .deposit(user, token, amount, StableMemoryOptions::Write);
        let timestamp = self.timestamp();
        self.events.push(Event {
            timestamp,
            payload: EventType::Deposit(DepositEvent {
                user,
                token,
                amount,
            }),
        });
        self
    }

    /// Debits `amount` of `token` from `user`'s free balance on the primary
    /// path and records the matching `WithdrawEvent`. `block_index` is the
    /// ledger block index the production path would receive from the ledger.
    /// Panics if the balance is insufficient (the test is expected to fund
    /// the user first).
    fn with_withdraw(
        mut self,
        user: Principal,
        token: TokenId,
        amount: Quantity,
        block_index: u64,
    ) -> Self {
        self.state
            .withdraw(user, token, amount)
            .expect("test setup: insufficient balance for withdraw");
        let timestamp = self.timestamp();
        self.events.push(Event {
            timestamp,
            payload: EventType::Withdraw(WithdrawEvent {
                block_index,
                user,
                token,
                amount,
            }),
        });
        self
    }

    /// Records a limit order on the primary path and the matching
    /// `AddLimitOrderEvent`. Returns the assigned `OrderId` so the caller can
    /// reference it in later matching/settling fixtures.
    fn with_limit_order(
        self,
        user: Principal,
        side: Side,
        price: Price,
        quantity: Quantity,
    ) -> (Self, OrderId) {
        self.with_limit_order_tif(user, side, price, quantity, TimeInForce::GoodTilCanceled)
    }

    fn with_limit_order_tif(
        self,
        user: Principal,
        side: Side,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
    ) -> (Self, OrderId) {
        self.with_limit_order_tif_placed_by(user, side, price, quantity, time_in_force, None)
    }

    fn with_limit_order_tif_placed_by(
        mut self,
        user: Principal,
        side: Side,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
        placed_by: Option<Principal>,
    ) -> (Self, OrderId) {
        let (order_id, order) = self
            .state
            .validate_limit_order(
                user,
                trading_pair(),
                PendingOrder {
                    side,
                    price,
                    quantity,
                    time_in_force,
                },
            )
            .unwrap();
        let timestamp = self.timestamp();
        self.state.record_limit_order(
            user,
            order_id.book_id(),
            order,
            placed_by,
            timestamp,
            StableMemoryOptions::Write,
        );
        self.events.push(Event {
            timestamp,
            payload: EventType::AddLimitOrder(AddLimitOrderEvent {
                user,
                order_id,
                side,
                price,
                quantity,
                time_in_force,
                placed_by,
            }),
        });
        (self, order_id)
    }

    /// Cancels `order_id` for `user` on the primary path and records the
    /// two corresponding audit-log events (`CancelLimitOrderEvent` for the
    /// book mutation, then a `SettlingEvent` for the refund + status
    /// transition). Panics if validation would reject (the test is expected
    /// to set up a cancelable order first).
    fn with_cancel(mut self, user: Principal, order_id: OrderId) -> Self {
        self.state
            .validate_cancel_limit_order(&user, &order_id)
            .expect("test setup: order must be cancelable");
        let cancel_ts = self.timestamp();
        self.state
            .record_cancel_limit_order(order_id, cancel_ts, StableMemoryOptions::Write);
        let settling_event = self
            .state
            .take_next_pending_settling_event()
            .expect("BUG: record_cancel_limit_order did not push a settling event");
        let settling_ts = self.timestamp();
        self.state
            .record_settling_event(&settling_event, settling_ts, StableMemoryOptions::Write);
        self.events.push(Event {
            timestamp: cancel_ts,
            payload: EventType::CancelLimitOrder(CancelLimitOrderEvent { order_id }),
        });
        self.events.push(Event {
            timestamp: settling_ts,
            payload: EventType::Settling(settling_event),
        });
        self
    }

    /// Drives one matching round on the primary path and records the resulting
    /// `MatchingEvent` + `SettlingEvent` as the expected replay payload. The
    /// caller passes the `MatchingEvent` and the expected `balance_operations`;
    /// the harness drives matching directly (one chunk, mirroring the executor)
    /// so it can capture the real `SettlingEvent` — including the per-fill
    /// settlements — from the pending queue and assert its balance operations
    /// match the expectation before recording it for replay.
    fn with_matching_round(
        mut self,
        matching: MatchingEvent,
        expected_balance_operations: Vec<BalanceOperation>,
    ) -> Self {
        let matching_ts = self.timestamp();
        self.state
            .record_matching_event(&matching, matching_ts, StableMemoryOptions::Write);
        self.events.push(Event {
            timestamp: matching_ts,
            payload: EventType::Matching(matching),
        });
        let mut produced_balance_operations = Vec::new();
        while let Some(settling) = self.state.take_next_pending_settling_event() {
            produced_balance_operations.extend(settling.balance_operations.iter().cloned());
            let settling_ts = self.timestamp();
            self.state
                .record_settling_event(&settling, settling_ts, StableMemoryOptions::Write);
            self.events.push(Event {
                timestamp: settling_ts,
                payload: EventType::Settling(settling),
            });
        }
        assert_eq!(
            produced_balance_operations, expected_balance_operations,
            "matching round produced unexpected balance operations",
        );
        self
    }

    /// Asserts a concrete (free, reserved) balance on the primary-path state.
    /// Independent of whether replay reproduces it — catches bugs that break
    /// both paths identically (e.g. `record_settling_event` dropping a
    /// transfer), which an `replayed == normal` check would miss.
    fn assert_balance(
        self,
        user: Principal,
        token: TokenId,
        free: impl Into<Quantity>,
        reserved: impl Into<Quantity>,
    ) -> Self {
        assert_eq!(
            self.state.get_balance(&user, &token),
            Balance::new(free, reserved),
            "unexpected balance for {user} on {token:?}",
        );
        self
    }

    /// Asserts the order history entry for `order_id` is in `expected` status.
    fn assert_order_status(self, order_id: OrderId, expected: OrderStatus) -> Self {
        let status = self
            .state
            .order_history
            .get(&order_id)
            .unwrap_or_else(|| panic!("order {order_id:?} missing from history"))
            .status;
        assert_eq!(status, expected, "unexpected status for {order_id:?}");
        self
    }

    fn assert_filled_quantity(self, order_id: OrderId, expected: impl Into<Quantity>) -> Self {
        let filled = self
            .state
            .order_history
            .get(&order_id)
            .unwrap_or_else(|| panic!("order {order_id:?} missing from history"))
            .filled_quantity;
        assert_eq!(
            filled,
            expected.into(),
            "unexpected filled_quantity for {order_id:?}"
        );
        self
    }

    /// Asserts the order history entry for `order_id` carries `expected` TIF.
    fn assert_time_in_force(self, order_id: OrderId, expected: TimeInForce) -> Self {
        let tif = self
            .state
            .order_history
            .get(&order_id)
            .unwrap_or_else(|| panic!("order {order_id:?} missing from history"))
            .time_in_force;
        assert_eq!(tif, expected, "unexpected time_in_force for {order_id:?}");
        self
    }

    /// Applies a `SetHalt` mutation on the primary path and records the matching
    /// `SetHalt` event as the expected replay payload. `book_ids` of `None`
    /// targets the global flag (and, when resuming, clears every per-pair halt);
    /// `Some` targets only the listed books.
    fn with_set_halt(mut self, book_ids: Option<Vec<OrderBookId>>, halted: bool) -> Self {
        let timestamp = self.timestamp();
        let payload = EventType::SetHalt(crate::state::event::SetHaltEvent { book_ids, halted });
        apply_state_transition(
            &mut self.state,
            &payload,
            timestamp,
            StableMemoryOptions::Write,
        );
        self.events.push(Event { timestamp, payload });
        self
    }

    fn with_add_trading_account(mut self, funding: Principal, trading: Principal) -> Self {
        let timestamp = self.timestamp();
        let payload = EventType::AddTradingAccount(AddTradingAccountEvent {
            funding: crate::user::FundingAccount(funding),
            trading: crate::user::TradingAccount(trading),
        });
        apply_state_transition(
            &mut self.state,
            &payload,
            timestamp,
            StableMemoryOptions::Write,
        );
        self.events.push(Event { timestamp, payload });
        self
    }

    fn with_remove_trading_account(mut self, funding: Principal, trading: Principal) -> Self {
        let timestamp = self.timestamp();
        let payload = EventType::RemoveTradingAccount(RemoveTradingAccountEvent {
            funding: crate::user::FundingAccount(funding),
            trading: crate::user::TradingAccount(trading),
        });
        apply_state_transition(
            &mut self.state,
            &payload,
            timestamp,
            StableMemoryOptions::Write,
        );
        self.events.push(Event { timestamp, payload });
        self
    }

    fn assert_replay_matches(self) {
        // Replay into *fresh* stable structures (not clones of `normal`'s) so
        // the assertion also validates that replay reconstructs stable memory,
        // not just the heap fields of `State`.
        let replayed = replay_events(
            self.events,
            order_history(),
            trade_history(),
            user_registry(),
            balances(),
            StableMemoryOptions::Write,
        );
        assert_eq!(replayed, self.state);
    }
}

#[test]
fn should_replay_init_event() {
    Scenario::new().assert_replay_matches();
}

#[test]
fn should_replay_init_then_upgrade() {
    let restricted = Mode::restricted_to(vec![Principal::from_slice(&[0x01])]);
    Scenario::new()
        .with_upgrade(Some(restricted))
        .assert_replay_matches();
}

#[test]
fn should_replay_upgrade_without_mode_change() {
    Scenario::new().with_upgrade(None).assert_replay_matches();
}

#[test]
fn should_replay_execution_policy_change_on_upgrade() {
    Scenario::new()
        .with_upgrade_execution_policy(123, 4_567_890, 64)
        .assert_replay_matches();
}

#[test]
fn should_replay_add_trading_pair() {
    Scenario::new().with_trading_pair().assert_replay_matches();
}

#[test]
fn should_replay_deposit() {
    Scenario::new()
        .with_trading_pair()
        .with_deposit(user_1(), TokenId::new(base()), Quantity::from(1_000_000u64))
        .assert_replay_matches();
}

#[test]
fn should_replay_withdraw() {
    let deposit_amount = 1_000_000u64;
    let withdraw_amount = 700_000u64;
    Scenario::new()
        .with_trading_pair()
        .with_deposit(
            user_1(),
            TokenId::new(base()),
            Quantity::from(deposit_amount),
        )
        .with_withdraw(
            user_1(),
            TokenId::new(base()),
            Quantity::from(withdraw_amount),
            42,
        )
        .assert_balance(
            user_1(),
            TokenId::new(base()),
            deposit_amount - withdraw_amount,
            0u64,
        )
        .assert_replay_matches();
}

#[test]
fn should_replay_add_trading_account() {
    Scenario::new()
        .with_trading_pair()
        .with_deposit(user_1(), TokenId::new(base()), Quantity::from(1_000_000u64))
        .with_add_trading_account(user_1(), user_2())
        .assert_replay_matches();
}

#[test]
fn should_replay_remove_trading_account_preserving_the_cooldown_anchor() {
    Scenario::new()
        .with_trading_pair()
        .with_deposit(user_1(), TokenId::new(base()), Quantity::from(1_000_000u64))
        .with_add_trading_account(user_1(), user_2())
        .with_remove_trading_account(user_1(), user_2())
        .assert_replay_matches();
}

#[test]
fn should_replay_add_limit_order() {
    let price = 100u128;
    let quantity = 1_000_000u128;
    let (scenario, _) = Scenario::new()
        .with_trading_pair()
        .with_deposit(
            user_1(),
            TokenId::new(quote()),
            Quantity::from(price * quantity),
        )
        .with_limit_order(
            user_1(),
            Side::Buy,
            Price::new(price * PRICE_SCALE),
            Quantity::from(quantity),
        );
    scenario.assert_replay_matches();
}

#[test]
fn should_replay_add_limit_order_placed_by_a_trading_account() {
    let price = 100u128;
    let quantity = 1_000_000u128;
    let (scenario, _) = Scenario::new()
        .with_trading_pair()
        .with_deposit(
            user_1(),
            TokenId::new(quote()),
            Quantity::from(price * quantity),
        )
        .with_limit_order_tif_placed_by(
            user_1(),
            Side::Buy,
            Price::new(price * PRICE_SCALE),
            Quantity::from(quantity),
            TimeInForce::GoodTilCanceled,
            Some(user_2()),
        );
    scenario.assert_replay_matches();
}

#[test]
fn should_replay_fill_or_kill_order_preserving_time_in_force() {
    let price = 100u128;
    let quantity = 1_000_000u128;
    let (scenario, order_id) = Scenario::new()
        .with_trading_pair()
        .with_deposit(
            user_1(),
            TokenId::new(quote()),
            Quantity::from(price * quantity),
        )
        .with_limit_order_tif(
            user_1(),
            Side::Buy,
            Price::new(price * PRICE_SCALE),
            Quantity::from(quantity),
            TimeInForce::FillOrKill,
        );
    scenario
        .assert_time_in_force(order_id, TimeInForce::FillOrKill)
        .assert_order_status(order_id, OrderStatus::Pending)
        .assert_replay_matches();
}

/// A FOK killed against an empty book transitions to `Expired` and releases its
/// whole placement reservation through a single `Unreserve`. Replaying the
/// matching round under `Skip` (the post-upgrade path) must reconstruct the
/// `Expired` status from history without re-applying the release — the
/// reservation must not be double-refunded.
#[test]
fn should_replay_killed_fill_or_kill_order_preserving_expired_state() {
    let price = 100u128;
    let quantity = 1_000_000u128;
    let reserved = price * quantity;
    let book_id = OrderBookId::ZERO;

    // FOK Buy against an empty book: nothing crosses, so matching kills it.
    let (scenario, fok_id) = Scenario::new()
        .with_trading_pair()
        .with_deposit(user_1(), TokenId::new(quote()), Quantity::from(reserved))
        .with_limit_order_tif(
            user_1(),
            Side::Buy,
            Price::new(price * PRICE_SCALE),
            Quantity::from(quantity),
            TimeInForce::FillOrKill,
        );

    scenario
        .with_matching_round(
            MatchingEvent {
                book_id,
                orders: vec![fok_id.seq()],
            },
            vec![BalanceOperation::Unreserve {
                order: fok_id.seq(),
                token: PairToken::Quote,
                amount: Quantity::from(reserved),
            }],
        )
        // The kill released the whole reservation back to free exactly once.
        .assert_balance(user_1(), TokenId::new(quote()), reserved, 0u64)
        .assert_order_status(fok_id, OrderStatus::Expired)
        .assert_filled_quantity(fok_id, 0u64)
        .assert_time_in_force(fok_id, TimeInForce::FillOrKill)
        .assert_replay_matches();
}

#[test]
fn should_replay_matching() {
    let buyer = user_1();
    let seller = user_2();
    let price = 100u128;
    let quantity = 1_000_000u128;
    let book_id = OrderBookId::ZERO;

    let (scenario, buy_id) = Scenario::new()
        .with_trading_pair()
        .with_deposit(
            buyer,
            TokenId::new(quote()),
            Quantity::from(price * quantity),
        )
        .with_deposit(seller, TokenId::new(base()), Quantity::from(quantity))
        .with_limit_order(
            buyer,
            Side::Buy,
            Price::new(price * PRICE_SCALE),
            Quantity::from(quantity),
        );
    let (scenario, sell_id) = scenario.with_limit_order(
        seller,
        Side::Sell,
        Price::new(price * PRICE_SCALE),
        Quantity::from(quantity),
    );

    // Sell-taker at `price` matches the resting buy at `price` for the full
    // quantity. No price improvement → no Unreserve op. Settlement:
    //   1. maker (buyer) pays `price × quantity` quote to taker (seller).
    //   2. taker (seller) pays `quantity` base to maker (buyer).
    scenario
        .with_matching_round(
            MatchingEvent {
                book_id,
                // FIFO order of pending seqs at round entry — buy then sell.
                orders: vec![buy_id.seq(), sell_id.seq()],
            },
            vec![
                BalanceOperation::Transfer {
                    from_order: buy_id.seq(),
                    to_order: sell_id.seq(),
                    token: PairToken::Quote,
                    amount: Quantity::from(price * quantity),
                    fee: None,
                },
                BalanceOperation::Transfer {
                    from_order: sell_id.seq(),
                    to_order: buy_id.seq(),
                    token: PairToken::Base,
                    amount: Quantity::from(quantity),
                    fee: None,
                },
            ],
        )
        // Post-fill balances: buyer holds `quantity` base free, seller holds
        // `price × quantity` quote free, everything else drained.
        .assert_balance(buyer, TokenId::new(base()), quantity, 0u64)
        .assert_balance(buyer, TokenId::new(quote()), 0u64, 0u64)
        .assert_balance(seller, TokenId::new(base()), 0u64, 0u64)
        .assert_balance(seller, TokenId::new(quote()), price * quantity, 0u64)
        .assert_order_status(buy_id, OrderStatus::Filled)
        .assert_order_status(sell_id, OrderStatus::Filled)
        .assert_replay_matches();
}

#[test]
fn should_replay_matching_with_price_improvement() {
    let buyer = user_1();
    let seller = user_2();
    let maker_price = 100u128;
    let taker_price = 110u128;
    let quantity = 1_000_000u128;
    let book_id = OrderBookId::ZERO;

    // Sell rests first; crossing buy enters at the higher `taker_price`.
    // Buyer reserved `taker_price × quantity` quote; the fill clears at
    // `maker_price`, so the surplus refunds via `Unreserve`.
    let (scenario, sell_id) = Scenario::new()
        .with_trading_pair()
        .with_deposit(
            buyer,
            TokenId::new(quote()),
            Quantity::from(taker_price * quantity),
        )
        .with_deposit(seller, TokenId::new(base()), Quantity::from(quantity))
        .with_limit_order(
            seller,
            Side::Sell,
            Price::new(maker_price * PRICE_SCALE),
            Quantity::from(quantity),
        );
    let (scenario, buy_id) = scenario.with_limit_order(
        buyer,
        Side::Buy,
        Price::new(taker_price * PRICE_SCALE),
        Quantity::from(quantity),
    );

    scenario
        .with_matching_round(
            MatchingEvent {
                book_id,
                orders: vec![sell_id.seq(), buy_id.seq()],
            },
            vec![
                BalanceOperation::Transfer {
                    from_order: buy_id.seq(),
                    to_order: sell_id.seq(),
                    token: PairToken::Quote,
                    amount: Quantity::from(maker_price * quantity),
                    fee: None,
                },
                BalanceOperation::Unreserve {
                    order: buy_id.seq(),
                    token: PairToken::Quote,
                    amount: Quantity::from((taker_price - maker_price) * quantity),
                },
                BalanceOperation::Transfer {
                    from_order: sell_id.seq(),
                    to_order: buy_id.seq(),
                    token: PairToken::Base,
                    amount: Quantity::from(quantity),
                    fee: None,
                },
            ],
        )
        // Post-fill balances: buyer gets `quantity` base, the `(taker-maker) ×
        // quantity` quote surplus refunded to free, seller gets
        // `maker_price × quantity` quote.
        .assert_balance(buyer, TokenId::new(base()), quantity, 0u64)
        .assert_balance(
            buyer,
            TokenId::new(quote()),
            (taker_price - maker_price) * quantity,
            0u64,
        )
        .assert_balance(seller, TokenId::new(base()), 0u64, 0u64)
        .assert_balance(seller, TokenId::new(quote()), maker_price * quantity, 0u64)
        .assert_order_status(sell_id, OrderStatus::Filled)
        .assert_order_status(buy_id, OrderStatus::Filled)
        .assert_replay_matches();
}

/// A matching round with more than `max_settlement_units_per_event` fills is
/// enqueued as several bounded settling events; replaying that multi-event log
/// must reconstruct exactly the same state the primary path produced.
#[test]
fn should_replay_matching_round_split_across_multiple_settling_events() {
    let buyer = user_1();
    let price = 100u128;
    let quantity = 1_000_000u128;
    let book_id = OrderBookId::ZERO;
    let cap = oisy_trade_types_internal::DEFAULT_MAX_SETTLEMENT_UNITS_PER_EVENT as usize;
    let num_makers = cap + 1;

    let mut scenario = Scenario::new().with_trading_pair().with_deposit(
        buyer,
        TokenId::new(quote()),
        Quantity::from(price * quantity * num_makers as u128),
    );

    let mut maker_seqs = Vec::new();
    for i in 0..num_makers {
        let seller = maker(i);
        scenario = scenario.with_deposit(seller, TokenId::new(base()), Quantity::from(quantity));
        let (next, sell_id) = scenario.with_limit_order(
            seller,
            Side::Sell,
            Price::new(price * PRICE_SCALE),
            Quantity::from(quantity),
        );
        scenario = next;
        maker_seqs.push(sell_id.seq());
    }
    let (scenario, buy_id) = scenario.with_limit_order(
        buyer,
        Side::Buy,
        Price::new(price * PRICE_SCALE),
        Quantity::from(num_makers as u128 * quantity),
    );

    let mut expected_ops = Vec::new();
    for maker_seq in &maker_seqs {
        expected_ops.push(BalanceOperation::Transfer {
            from_order: buy_id.seq(),
            to_order: *maker_seq,
            token: PairToken::Quote,
            amount: Quantity::from(price * quantity),
            fee: None,
        });
        expected_ops.push(BalanceOperation::Transfer {
            from_order: *maker_seq,
            to_order: buy_id.seq(),
            token: PairToken::Base,
            amount: Quantity::from(quantity),
            fee: None,
        });
    }

    let mut orders = maker_seqs;
    orders.push(buy_id.seq());

    scenario
        .with_matching_round(MatchingEvent { book_id, orders }, expected_ops)
        .assert_order_status(buy_id, OrderStatus::Filled)
        .assert_replay_matches();
}

#[test]
fn should_replay_cancel_pending_order() {
    let price = 100u128;
    let quantity = 1_000_000u128;
    let reserved = price * quantity;

    let (scenario, buy_id) = Scenario::new()
        .with_trading_pair()
        .with_deposit(user_1(), TokenId::new(quote()), Quantity::from(reserved))
        .with_limit_order(
            user_1(),
            Side::Buy,
            Price::new(price * PRICE_SCALE),
            Quantity::from(quantity),
        );

    // Cancel before any matching round runs — the order is still pending and
    // the full reserve returns to free.
    scenario
        .with_cancel(user_1(), buy_id)
        .assert_balance(user_1(), TokenId::new(quote()), reserved, 0u64)
        .assert_order_status(buy_id, OrderStatus::Canceled)
        // Never matched: remaining (quantity − filled_quantity) is the full
        // placed quantity, so filled_quantity is zero.
        .assert_filled_quantity(buy_id, 0u64)
        .assert_replay_matches();
}

#[test]
fn should_replay_cancel_partially_filled_order() {
    let buyer = user_1();
    let seller = user_2();
    let price = 100u128;
    let quantity = 1_000_000u128; // one lot
    let book_id = OrderBookId::ZERO;

    // Seller rests 1 lot; buyer takes 3 lots — 1 lot fills, 2 lots rest as
    // Open. Cancelling the buy must refund the 2-lot residual in quote.
    let (scenario, sell_id) = Scenario::new()
        .with_trading_pair()
        .with_deposit(
            buyer,
            TokenId::new(quote()),
            Quantity::from(price * 3 * quantity),
        )
        .with_deposit(seller, TokenId::new(base()), Quantity::from(quantity))
        .with_limit_order(
            seller,
            Side::Sell,
            Price::new(price * PRICE_SCALE),
            Quantity::from(quantity),
        );
    let (scenario, buy_id) = scenario.with_limit_order(
        buyer,
        Side::Buy,
        Price::new(price * PRICE_SCALE),
        Quantity::from(3 * quantity),
    );

    // Same price on both sides → no price improvement, no Unreserve op.
    // Sell fully fills; buy rests Open with 2 lots of quote still reserved.
    let scenario = scenario.with_matching_round(
        MatchingEvent {
            book_id,
            orders: vec![sell_id.seq(), buy_id.seq()],
        },
        vec![
            BalanceOperation::Transfer {
                from_order: buy_id.seq(),
                to_order: sell_id.seq(),
                token: PairToken::Quote,
                amount: Quantity::from(price * quantity),
                fee: None,
            },
            BalanceOperation::Transfer {
                from_order: sell_id.seq(),
                to_order: buy_id.seq(),
                token: PairToken::Base,
                amount: Quantity::from(quantity),
                fee: None,
            },
        ],
    );

    scenario
        .with_cancel(buyer, buy_id)
        // Buyer: 1 lot base from the fill, 2 lots × price quote refunded.
        .assert_balance(buyer, TokenId::new(base()), quantity, 0u64)
        .assert_balance(buyer, TokenId::new(quote()), price * 2 * quantity, 0u64)
        // Seller: fully filled, 1 lot × price quote free.
        .assert_balance(seller, TokenId::new(base()), 0u64, 0u64)
        .assert_balance(seller, TokenId::new(quote()), price * quantity, 0u64)
        .assert_order_status(sell_id, OrderStatus::Filled)
        .assert_order_status(buy_id, OrderStatus::Canceled)
        // The canceled buy keeps the 1 lot it filled before cancel;
        // remaining (2 lots) is derived as quantity − filled_quantity.
        .assert_filled_quantity(buy_id, quantity)
        .assert_replay_matches();
}

#[test]
fn should_apply_global_halt() {
    let scenario = Scenario::new()
        .with_trading_pair()
        .with_set_halt(None, true);
    assert!(scenario.state.permissions().trading_halted());
    scenario.assert_replay_matches();
}

#[test]
fn should_apply_global_resume_after_halt() {
    let scenario = Scenario::new()
        .with_trading_pair()
        .with_set_halt(None, true)
        .with_set_halt(None, false);
    assert!(!scenario.state.permissions().trading_halted());
    scenario.assert_replay_matches();
}

#[test]
fn should_apply_pair_halt() {
    let scenario = Scenario::new()
        .with_trading_pair()
        .with_set_halt(Some(vec![OrderBookId::ZERO]), true);
    assert!(scenario.state.permissions().is_halted(&OrderBookId::ZERO));
    scenario.assert_replay_matches();
}

#[test]
fn should_apply_pair_resume_after_pair_halt() {
    let scenario = Scenario::new()
        .with_trading_pair()
        .with_set_halt(Some(vec![OrderBookId::ZERO]), true)
        .with_set_halt(Some(vec![OrderBookId::ZERO]), false);
    assert!(!scenario.state.permissions().is_halted(&OrderBookId::ZERO));
    scenario.assert_replay_matches();
}

#[test]
fn should_clear_every_pair_halt_on_global_resume() {
    let scenario = Scenario::new()
        .with_trading_pair()
        .with_set_halt(Some(vec![OrderBookId::ZERO]), true)
        .with_set_halt(None, true)
        .with_set_halt(None, false);
    assert!(!scenario.state.permissions().trading_halted());
    assert!(!scenario.state.permissions().is_halted(&OrderBookId::ZERO));
    scenario.assert_replay_matches();
}

#[test]
#[should_panic(expected = "the event log should not be empty")]
fn should_panic_on_empty_events() {
    replay_events(
        Vec::<Event>::new(),
        order_history(),
        trade_history(),
        user_registry(),
        balances(),
        StableMemoryOptions::Write,
    );
}

#[test]
#[should_panic(expected = "the first event must be an Init event")]
fn should_panic_when_first_event_is_not_init() {
    replay_events(
        vec![upgrade_event(None, None, None, None)],
        order_history(),
        trade_history(),
        user_registry(),
        balances(),
        StableMemoryOptions::Write,
    );
}
