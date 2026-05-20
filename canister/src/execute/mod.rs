//! Chunked driver for the pending-order matching pipeline.
//!
//! [`Executor::run_once`] processes a bounded slice of pending matching +
//! settling work. It emits at most `max_orders_per_chunk` orders' worth of
//! `MatchingEvent`s across the active books, then drains paired
//! `SettlingEvent`s. Total work per call is additionally capped by
//! `instruction_budget` (compared against [`Runtime::instruction_counter`]).
//! If anything is left over the call reports [`Outcome::MoreWork`] so the
//! caller can reschedule.

use crate::Runtime;
use crate::order::{OrderBookId, OrderSeq};
use crate::state::State;
use crate::state::audit;
use crate::state::event::{EventType, MatchingEvent};
use ic_stable_structures::Memory;

#[cfg(test)]
mod tests;

pub const DEFAULT_MAX_ORDERS_PER_CHUNK: usize = 1_000;

/// 1B instructions per chunk — ~5% of the IC's 20B per-message cap,
/// leaving generous headroom for event serialization, settling, and
/// stable-memory writes.
pub const DEFAULT_INSTRUCTION_BUDGET: u64 = 1_000_000_000;

pub const EXECUTOR: Executor = Executor {
    max_orders_per_chunk: DEFAULT_MAX_ORDERS_PER_CHUNK,
    instruction_budget: DEFAULT_INSTRUCTION_BUDGET,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Executor {
    pub max_orders_per_chunk: usize,
    pub instruction_budget: u64,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Outcome {
    Complete,
    MoreWork,
}

impl Outcome {
    pub fn from_state<MH: Memory, MB: Memory>(state: &State<MH, MB>) -> Self {
        if state.has_pending_orders() || state.has_pending_settling_events() {
            Outcome::MoreWork
        } else {
            Outcome::Complete
        }
    }
}

impl Executor {
    /// Drive a single chunk of matching + settling against `state`.
    ///
    /// Matching pulls up to `max_orders_per_chunk` pending orders, distributed
    /// across active books in decreasing pending-order count (ties broken by
    /// ascending book-ID). Each book whose share of the chunk is non-empty
    /// becomes one [`MatchingEvent`]. Settling events queued by the matching
    /// pass are drained in the same call, bounded by the same instruction
    /// budget.
    pub fn run_once<MH: Memory, MB: Memory>(
        &self,
        state: &mut State<MH, MB>,
        runtime: &impl Runtime,
    ) -> Outcome {
        let mut order_budget = self.max_orders_per_chunk;
        for book_id in books_by_pending_count_desc(state) {
            if order_budget == 0 || runtime.instruction_counter() >= self.instruction_budget {
                return Outcome::from_state(state);
            }
            let chunk = peek_pending_seqs(state, &book_id, order_budget);
            if chunk.is_empty() {
                continue;
            }
            order_budget = order_budget
                .checked_sub(chunk.len())
                .expect("BUG: peek_pending_seqs returns at most order_budget pending orders");
            audit::process_event(
                state,
                EventType::Matching(MatchingEvent {
                    book_id,
                    orders: chunk,
                }),
                runtime,
            );
        }

        while runtime.instruction_counter() < self.instruction_budget {
            let Some(event) = state.take_next_pending_settling_event() else {
                break;
            };
            audit::process_event(state, EventType::Settling(event), runtime);
        }

        Outcome::from_state(state)
    }
}

/// Order-book IDs ranked by decreasing pending-order count; ties broken by
/// ascending book id. Books with no pending orders are excluded.
fn books_by_pending_count_desc<MH: Memory, MB: Memory>(state: &State<MH, MB>) -> Vec<OrderBookId> {
    let mut counts: Vec<(OrderBookId, usize)> = state
        .order_books()
        .map(|(id, book)| (*id, book.pending_orders_len()))
        .filter(|(_, n)| *n > 0)
        .collect();
    counts.sort_by(|(a_id, a_n), (b_id, b_n)| b_n.cmp(a_n).then_with(|| a_id.cmp(b_id)));
    counts.into_iter().map(|(id, _)| id).collect()
}

/// FIFO pending-order seqs of `book_id`, capped at `limit`.
fn peek_pending_seqs<MH: Memory, MB: Memory>(
    state: &State<MH, MB>,
    book_id: &OrderBookId,
    limit: usize,
) -> Vec<OrderSeq> {
    state
        .order_book(book_id)
        .expect("BUG: ranked book_id missing from state")
        .pending_order_seqs()
        .take(limit)
        .collect()
}
