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
    LotSize, Order, OrderBook, OrderBookId, OrderHistory, OrderSeq, Price, RestingOrder, Side,
    TickSize, TokenId, TokenMetadata, TradingPair,
};
use crate::state::TradingPairMap;
use candid::Nat;
use dex_types_internal::Mode;
use ic_stable_structures::Memory;
use minicbor::{Decode, Encode};
use std::cmp::Reverse;
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

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct OrderBookSnapshot {
    #[n(0)]
    pub id: OrderBookId,
    #[n(1)]
    pub next_seq: OrderSeq,
    #[n(2)]
    pub tick_size: TickSize,
    #[n(3)]
    pub lot_size: LotSize,
    #[n(4)]
    pub pending_orders: Vec<Order>,
    /// Bid side, stored with the natural (un-reversed) price. Converted back
    /// to a `BTreeMap<Reverse<Price>, …>` on restore.
    #[n(5)]
    pub bids: Vec<PriceLevel>,
    #[n(6)]
    pub asks: Vec<PriceLevel>,
    #[n(7)]
    pub filled_orders: Vec<OrderSeq>,
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
pub struct PriceLevel {
    #[n(0)]
    pub price: Price,
    #[n(1)]
    pub orders: Vec<RestingOrder>,
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
            order_books: order_books
                .values()
                .map(OrderBookSnapshot::from_book)
                .collect(),
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
            tokens.insert(entry.token, entry.metadata);
        }

        let mut trading_pairs = TradingPairMap::default();
        for entry in self.trading_pairs {
            trading_pairs.insert(entry.pair, entry.book_id);
        }

        let mut order_books = BTreeMap::new();
        for book_snapshot in self.order_books {
            order_books.insert(book_snapshot.id, book_snapshot.into_book());
        }

        let mut ledger_fee_cache = BTreeMap::new();
        for entry in self.ledger_fee_cache {
            ledger_fee_cache.insert(entry.token, entry.fee);
        }

        State::from_snapshot_parts(
            self.mode,
            self.next_book_id,
            tokens,
            trading_pairs,
            order_books,
            ledger_fee_cache,
            order_history,
            balances,
        )
    }
}

impl OrderBookSnapshot {
    fn from_book(book: &OrderBook) -> Self {
        Self {
            id: book.id(),
            next_seq: book.next_seq(),
            tick_size: book.tick_size(),
            lot_size: book.lot_size(),
            pending_orders: book.pending_orders().iter().cloned().collect(),
            bids: book
                .bids()
                .iter()
                .map(|(Reverse(price), orders)| PriceLevel {
                    price: *price,
                    orders: orders.iter().cloned().collect(),
                })
                .collect(),
            asks: book
                .asks()
                .iter()
                .map(|(price, orders)| PriceLevel {
                    price: *price,
                    orders: orders.iter().cloned().collect(),
                })
                .collect(),
            filled_orders: book.filled_orders().iter().copied().collect(),
        }
    }

    fn into_book(self) -> OrderBook {
        let pending_orders: VecDeque<Order> = self.pending_orders.into_iter().collect();
        let mut bids: BTreeMap<Reverse<Price>, VecDeque<RestingOrder>> = BTreeMap::new();
        let mut asks: BTreeMap<Price, VecDeque<RestingOrder>> = BTreeMap::new();
        let mut resting_orders: BTreeMap<OrderSeq, (Side, Price)> = BTreeMap::new();

        for level in self.bids {
            for order in &level.orders {
                resting_orders.insert(order.id(), (Side::Buy, level.price));
            }
            bids.insert(Reverse(level.price), VecDeque::from(level.orders));
        }
        for level in self.asks {
            for order in &level.orders {
                resting_orders.insert(order.id(), (Side::Sell, level.price));
            }
            asks.insert(level.price, VecDeque::from(level.orders));
        }

        let filled_orders = self.filled_orders.into_iter().collect();
        OrderBook::from_snapshot_parts(
            self.id,
            self.next_seq,
            self.tick_size,
            self.lot_size,
            pending_orders,
            bids,
            asks,
            resting_orders,
            filled_orders,
        )
    }
}
