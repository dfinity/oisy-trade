//! Pre/post-upgrade snapshot for the transient (heap) portion of [`State`].
//!
//! Balances and order history live in stable-memory maps
//! (see [`crate::balance::TokenBalance`] and [`crate::order::OrderHistory`])
//! and survive upgrades on their own — they are *not* copied into the
//! snapshot. Everything else [`State`] carries — `mode`, `next_book_id`,
//! `tokens`, `trading_pairs`, `order_books`, `ledger_fee_cache` — is
//! serialized here at `pre_upgrade` and restored at `post_upgrade`. The
//! `active_tasks` set is intentionally excluded: it tracks in-flight timer
//! work and is reset to empty after every upgrade.

use super::State;
use crate::balance::TokenBalance;
use crate::order::{
    OrderBook, OrderBookId, OrderBookSnapshot, OrderHistory, TokenId, TokenMetadata, TradingPair,
};
use crate::state::TradingPairMap;
use candid::Nat;
use dex_types_internal::Mode;
use ic_stable_structures::Memory;
use minicbor::{Decode, Encode};
use std::collections::BTreeMap;

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
            next_book_id,
            tokens,
            trading_pairs,
            order_books,
            // ignored: live in stable memory,
            balances: _,
            // ignored: live in stable memory,
            order_history: _,
            // ignored: timers are reset upon upgrades
            active_tasks: _,
            ledger_fee_cache,
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
        }
    }

    /// Reconstruct a [`State`] from the decoded snapshot and the stable-memory
    /// structures that survived the upgrade independently.
    pub fn into_state<MH: Memory, MB: Memory>(
        self,
        order_history: OrderHistory<MH>,
        balances: TokenBalance<MB>,
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

        State {
            mode: self.mode,
            next_book_id: self.next_book_id,
            tokens,
            trading_pairs,
            order_books,
            balances,
            order_history,
            active_tasks: Default::default(),
            ledger_fee_cache,
        }
    }
}
