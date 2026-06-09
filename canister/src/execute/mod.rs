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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Executor;

pub const EXECUTOR: Executor = Executor;

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
        let policy = state.execution_policy();
        let max_orders_per_chunk = policy.max_orders_per_chunk() as usize;
        let instruction_budget = policy.instruction_budget();

        // Clear any settling events left over from a prior chunk whose
        // inline drain was interrupted by the instruction budget.
        self.drain_settling(instruction_budget, state, runtime);

        if runtime.instruction_counter() >= instruction_budget {
            return ExecutionStatus::from_state(state);
        }

        let mut order_budget = max_orders_per_chunk;
        for book_id in books_by_pending_order_count_desc(state) {
            if order_budget == 0 || runtime.instruction_counter() >= instruction_budget {
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
            let permit = state
                .permissions()
                .permit_matching()
                .expect("BUG: matching is never gated in this build");
            audit::process_event(
                state,
                EventType::Matching(MatchingEvent {
                    book_id,
                    orders: chunk,
                }),
                permit.into(),
                runtime,
            );
            // Settle this book's matches before advancing to the next book.
            self.drain_settling(instruction_budget, state, runtime);
        }

        ExecutionStatus::from_state(state)
    }

    fn drain_settling<MH: Memory, MB: Memory>(
        &self,
        instruction_budget: u64,
        state: &mut State<MH, MB>,
        runtime: &impl Runtime,
    ) {
        while runtime.instruction_counter() < instruction_budget {
            let Some(event) = state.take_next_pending_settling_event() else {
                break;
            };
            let permit = state
                .permissions()
                .permit_settling()
                .expect("BUG: settling is never gated in this build");
            audit::process_event(state, EventType::Settling(event), permit.into(), runtime);
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
    counts.sort_unstable_by(
        |(a_id, a_num_pending_orders), (b_id, b_num_pending_orders)| {
            b_num_pending_orders
                .cmp(a_num_pending_orders)
                .then_with(|| a_id.cmp(b_id))
        },
    );
    counts.into_iter().map(|(id, _)| id).collect()
}
