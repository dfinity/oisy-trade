//! Chunked driver for the pending-order matching pipeline.
//!
//! [`Executor::run_once`] processes a bounded slice of pending matching +
//! settling work. It first drains any settling events left over from a prior
//! chunk, then emits at most `max_orders_per_chunk` orders' worth of
//! `MatchingEvent`s across the active books — settling each book's
//! `MatchingEvent` inline before moving on to the next book. Total work per
//! call is additionally capped by `instruction_budget` (compared against
//! [`Runtime::instruction_counter`]). If anything is left over the call
//! reports [`ExecutionStatus::MoreWork`] so the caller can reschedule.

use crate::Runtime;
use crate::order::OrderBookId;
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
pub enum ExecutionStatus {
    Complete,
    MoreWork,
    /// The caller couldn't acquire the matching guard because another task
    /// is already running. The holder is responsible for rescheduling if it
    /// leaves work unfinished.
    AlreadyRunning,
}

impl ExecutionStatus {
    pub fn from_state<MH: Memory, MB: Memory>(state: &State<MH, MB>) -> Self {
        if state.has_pending_orders() || state.has_pending_settling_events() {
            ExecutionStatus::MoreWork
        } else {
            ExecutionStatus::Complete
        }
    }
}

impl Executor {
    /// Drive a single chunk of matching + settling against `state`.
    ///
    /// Pre-drains any settling events left over from a prior chunk, then
    /// pulls up to `max_orders_per_chunk` pending orders distributed across
    /// active books in decreasing pending-order count (ties broken by
    /// ascending book-ID). Each book whose share of the chunk is non-empty
    /// becomes one [`MatchingEvent`] whose paired settling is drained
    /// inline before the next book is visited. The total per-call work is
    /// bounded by `instruction_budget`.
    pub fn run_once<MH: Memory, MB: Memory>(
        &self,
        state: &mut State<MH, MB>,
        runtime: &impl Runtime,
    ) -> ExecutionStatus {
        // Clear any settling events left over from a prior chunk whose
        // inline drain was interrupted by the instruction budget.
        self.drain_settling(state, runtime);

        let mut order_budget = self.max_orders_per_chunk;
        for book_id in books_by_pending_order_count_desc(state) {
            if order_budget == 0 || runtime.instruction_counter() >= self.instruction_budget {
                return ExecutionStatus::from_state(state);
            }
            let chunk: Vec<_> = state
                .order_book(&book_id)
                .expect("BUG: book_id missing from state")
                .pending_order_seqs()
                .take(order_budget)
                .collect();
            if chunk.is_empty() {
                continue;
            }
            order_budget = order_budget
                .checked_sub(chunk.len())
                .expect("BUG: pending_order_seqs().take(order_budget) cannot exceed order_budget");
            audit::process_event(
                state,
                EventType::Matching(MatchingEvent {
                    book_id,
                    orders: chunk,
                }),
                runtime,
            );
            // Settle this book's matches before advancing to the next book.
            self.drain_settling(state, runtime);
        }

        ExecutionStatus::from_state(state)
    }

    fn drain_settling<MH: Memory, MB: Memory>(
        &self,
        state: &mut State<MH, MB>,
        runtime: &impl Runtime,
    ) {
        while runtime.instruction_counter() < self.instruction_budget {
            let Some(event) = state.take_next_pending_settling_event() else {
                break;
            };
            audit::process_event(state, EventType::Settling(event), runtime);
        }
    }
}

/// Order-book IDs ranked by decreasing pending-order count; ties broken by
/// ascending book id. Books with no pending orders are excluded.
fn books_by_pending_order_count_desc<MH: Memory, MB: Memory>(
    state: &State<MH, MB>,
) -> Vec<OrderBookId> {
    let mut counts: Vec<(OrderBookId, usize)> = state
        .order_books()
        .map(|(id, book)| (*id, book.pending_orders_len()))
        .filter(|(_, n)| *n > 0)
        .collect();
    counts.sort_by(
        |(a_id, a_num_pending_orders), (b_id, b_num_pending_orders)| {
            b_num_pending_orders
                .cmp(a_num_pending_orders)
                .then_with(|| a_id.cmp(b_id))
        },
    );
    counts.into_iter().map(|(id, _)| id).collect()
}
