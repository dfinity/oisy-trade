//! Pre/post-upgrade snapshot for the transient (heap) portion of [`State`].
//!
//! Balances and order history live in stable-memory maps
//! (see [`crate::balance::TokenBalance`] and [`crate::order::OrderHistory`])
//! and survive upgrades on their own — they are *not* copied into the
//! snapshot. Everything else [`State`] carries — `mode`, `next_book_id`,
//! `tokens`, `trading_pairs`, `order_books`, `ledger_fee_cache`,
//! `pending_settling_events`, the chunked-matching `execution_policy`
//! (`max_orders_per_chunk` + `instruction_budget`), and the heap-resident
//! `fee_pool` inside [`crate::balance::TokenBalance`] — is serialized
//! here at `pre_upgrade` and restored at `post_upgrade`.

use super::State;
use crate::balance::{FeeEntry, TokenBalance};
use crate::order::{
    OrderBook, OrderBookId, OrderBookSnapshot, OrderHistory, TokenId, TokenMetadata, TradingPair,
};
use crate::state::ExecutionPolicy;
use crate::state::TradingPairMap;
use crate::state::event::SettlingEvent;
use crate::user::UserRegistry;
use candid::Nat;
use dex_types_internal::Mode;
use ic_stable_structures::Memory;
use minicbor::{Decode, Encode};
use std::collections::{BTreeMap, VecDeque};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct StateSnapshot {
    #[n(0)]
    pub mode: Mode,
    #[n(1)]
    pub next_book_id: OrderBookId,
    #[n(2)]
    pub tokens: Vec<TokenEntry>,
    #[n(3)]
    pub trading_pairs: Vec<TradingPairEntry>,
    #[n(4)]
    pub order_books: Vec<OrderBookSnapshot>,
    #[n(5)]
    pub ledger_fee_cache: Vec<LedgerFeeEntry>,
    /// `SettlingEvent`s awaiting dispatch. Typically empty between
    /// messages (hence the `Option` — encoded as `null` when empty).
    #[n(6)]
    pub pending_settling_events: Option<Vec<SettlingEvent>>,
    /// Chunked-matching policy, flattened on the wire in 2 fields.
    #[n(7)]
    pub max_orders_per_chunk: Option<u32>,
    #[n(8)]
    pub instruction_budget: Option<u64>,
    /// Heap-resident fee pool inside [`TokenBalance`]. Encoded as `None`
    /// when the pool is empty.
    #[n(9)]
    pub fee_pool: Option<Vec<FeeEntry>>,
    /// Global order-insertion counter backing the per-user index. `Option` so
    /// snapshots written before this field decode to `None` (→ 0).
    #[n(10)]
    pub next_order_seq: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct TokenEntry {
    #[n(0)]
    pub token: TokenId,
    #[n(1)]
    pub metadata: TokenMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct TradingPairEntry {
    #[n(0)]
    pub pair: TradingPair,
    #[n(1)]
    pub book_id: OrderBookId,
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct LedgerFeeEntry {
    #[n(0)]
    pub token: TokenId,
    #[cbor(n(1), with = "icrc_cbor::nat")]
    pub fee: Nat,
}

impl StateSnapshot {
    pub fn from_state<MH: Memory, MB: Memory>(state: &State<MH, MB>) -> Self {
        let State {
            mode,
            execution_policy,
            next_book_id,
            tokens,
            trading_pairs,
            order_books,
            // ignored: lives in stable memory, survives upgrades on its own
            user_registry: _,
            // only the heap fee pool is snapshotted below; user balances
            // live in stable memory and survive upgrades on their own.
            balances,
            // ignored: live in stable memory,
            order_history: _,
            next_order_seq,
            // ignored: timers are reset upon upgrades
            active_tasks: _,
            ledger_fee_cache,
            pending_settling_events,
            // ignored: per-request guard set, reset upon upgrades
            in_flight_user_ops: _,
        } = state;
        Self {
            mode: mode.clone(),
            next_book_id: *next_book_id,
            tokens: tokens
                .iter()
                .map(|(token, metadata)| TokenEntry {
                    token: *token,
                    metadata: metadata.clone(),
                })
                .collect(),
            trading_pairs: trading_pairs
                .iter()
                .map(|(pair, book_id)| TradingPairEntry {
                    pair: pair.clone(),
                    book_id: *book_id,
                })
                .collect(),
            order_books: order_books.values().map(OrderBookSnapshot::from).collect(),
            ledger_fee_cache: ledger_fee_cache
                .iter()
                .map(|(token, fee)| LedgerFeeEntry {
                    token: *token,
                    fee: fee.clone(),
                })
                .collect(),
            pending_settling_events: if pending_settling_events.is_empty() {
                None
            } else {
                Some(pending_settling_events.iter().cloned().collect())
            },
            max_orders_per_chunk: Some(execution_policy.max_orders_per_chunk()),
            instruction_budget: Some(execution_policy.instruction_budget()),
            fee_pool: {
                let snapshot = balances.fee_pool_snapshot();
                if snapshot.is_empty() {
                    None
                } else {
                    Some(snapshot)
                }
            },
            next_order_seq: Some(*next_order_seq),
        }
    }

    /// Reconstruct a [`State`] from the decoded snapshot and the stable-memory
    /// structures that survived the upgrade independently.
    pub fn into_state<MH: Memory, MB: Memory>(
        self,
        order_history: OrderHistory<MH>,
        mut balances: TokenBalance<MB>,
        user_registry: UserRegistry<MB>,
    ) -> State<MH, MB> {
        let mut tokens = BTreeMap::new();
        for entry in self.tokens {
            assert!(
                tokens.insert(entry.token, entry.metadata).is_none(),
                "invalid snapshot: duplicate token entry for {:?}",
                entry.token
            );
        }

        // `TradingPairMap::insert` already panics on duplicate pair or book_id.
        let mut trading_pairs = TradingPairMap::default();
        for entry in self.trading_pairs {
            trading_pairs.insert(entry.pair, entry.book_id);
        }

        let mut order_books = BTreeMap::new();
        for book_snapshot in self.order_books {
            let id = book_snapshot.id;
            assert!(
                order_books
                    .insert(id, OrderBook::from(book_snapshot))
                    .is_none(),
                "invalid snapshot: duplicate order book entry for {:?}",
                id
            );
        }

        let mut ledger_fee_cache = BTreeMap::new();
        for entry in self.ledger_fee_cache {
            assert!(
                ledger_fee_cache.insert(entry.token, entry.fee).is_none(),
                "invalid snapshot: duplicate ledger fee entry for {:?}",
                entry.token
            );
        }

        let pending_settling_events: VecDeque<SettlingEvent> = self
            .pending_settling_events
            .unwrap_or_default()
            .into_iter()
            .collect();

        balances.restore_fee_pool(self.fee_pool.unwrap_or_default());

        let execution_policy = match (self.max_orders_per_chunk, self.instruction_budget) {
            (Some(max), Some(budget)) => ExecutionPolicy::try_new(max, budget)
                .expect("BUG: snapshot carried an invalid ExecutionPolicy"),
            // Snapshots written before this PR carry neither field; fall
            // back to the production default. Partial states (exactly one
            // field) imply a schema regression and trap so the bug
            // surfaces instead of silently reverting to defaults.
            (None, None) => ExecutionPolicy::default(),
            (max, budget) => panic!(
                "invalid snapshot: partial execution policy fields \
                 (max_orders_per_chunk={:?}, instruction_budget={:?})",
                max, budget,
            ),
        };

        State {
            mode: self.mode,
            execution_policy,
            next_book_id: self.next_book_id,
            tokens,
            trading_pairs,
            order_books,
            user_registry,
            balances,
            order_history,
            next_order_seq: self.next_order_seq.unwrap_or(0),
            active_tasks: Default::default(),
            ledger_fee_cache,
            pending_settling_events,
            in_flight_user_ops: Default::default(),
        }
    }
}
