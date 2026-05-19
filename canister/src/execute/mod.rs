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
use crate::state::State;
use crate::state::audit;
use crate::state::event::{EventType, MatchingEvent};
use ic_stable_structures::Memory;

#[cfg(test)]
mod tests;

pub const DEFAULT_MAX_ORDERS_PER_CHUNK: usize = 200;

/// ~60% of the IC's 20B per-message instruction cap, leaving headroom for
/// event serialization, settling, and stable-memory writes.
pub const DEFAULT_INSTRUCTION_BUDGET: u64 = 12_000_000_000;

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

impl Executor {
    /// Drive a single chunk of matching + settling against `state`.
    ///
    /// Matching pulls up to `max_orders_per_chunk` pending orders, distributed
    /// across active books in book-ID order. Each book whose share of the
    /// chunk is non-empty becomes one [`MatchingEvent`]. Settling events
    /// queued by the matching pass are drained in the same call, bounded by
    /// the same instruction budget.
    pub fn run_once<MH: Memory, MB: Memory>(
        &self,
        state: &mut State<MH, MB>,
        runtime: &impl Runtime,
    ) -> Outcome {
        let mut order_budget = self.max_orders_per_chunk;
        for book_id in state.book_ids_with_pending_orders() {
            if order_budget == 0 {
                break;
            }
            if runtime.instruction_counter() >= self.instruction_budget {
                break;
            }
            let chunk = state.peek_pending_seqs(&book_id, order_budget);
            if chunk.is_empty() {
                continue;
            }
            order_budget -= chunk.len();
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

        if state.has_pending_orders() || state.has_pending_settling_events() {
            Outcome::MoreWork
        } else {
            Outcome::Complete
        }
    }
}
