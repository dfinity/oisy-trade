use super::{StableMemoryOptions, State};
use crate::Runtime;
use crate::balance::TokenBalance;
use crate::order::OrderHistory;
use crate::state::event::{
    AddLimitOrderEvent, AddTradingPairEvent, CancelLimitOrderEvent, DepositEvent, Event, EventType,
    WithdrawEvent, WithdrawFeesEvent,
};
use crate::storage;
use dex_types_internal::UpgradeArg;
use ic_stable_structures::Memory;
use std::collections::VecDeque;

#[cfg(test)]
mod tests;

pub fn process_event<MH: Memory, MB: Memory>(
    state: &mut State<MH, MB>,
    payload: EventType,
    runtime: &impl Runtime,
) {
    apply_state_transition(state, &payload, StableMemoryOptions::Write);
    storage::record_event(runtime.time(), payload);
}

/// Append `payload` to the event log without applying it to `state`. Use this
/// when the primary path has already mutated state through a direct call
/// (e.g. `withdraw`, where the debit has to happen *before* the async ledger
/// call for concurrency safety). Replaying the event through
/// [`apply_state_transition`] reproduces the direct mutation, so replay
/// equivalence is preserved.
pub fn record_event(payload: EventType, runtime: &impl Runtime) {
    storage::record_event(runtime.time(), payload);
}

fn apply_state_transition<MH: Memory, MB: Memory>(
    state: &mut State<MH, MB>,
    payload: &EventType,
    persistence: StableMemoryOptions,
) {
    use crate::order;

    match payload {
        EventType::Init(_) => {
            panic!("BUG: state re-initialization is not allowed");
        }
        EventType::Upgrade(UpgradeArg {
            mode: new_mode,
            max_orders_per_chunk,
            instruction_budget,
        }) => {
            if let Some(new_mode) = new_mode {
                state.set_mode(new_mode.clone());
            }
            if max_orders_per_chunk.is_some() || instruction_budget.is_some() {
                let current = state.execution_policy();
                let policy = crate::state::ExecutionPolicy::try_new(
                    max_orders_per_chunk.unwrap_or_else(|| current.max_orders_per_chunk()),
                    instruction_budget.unwrap_or_else(|| current.instruction_budget()),
                )
                .unwrap_or_else(|e| panic!("BUG: invalid ExecutionPolicy: {e}"));
                state.set_execution_policy(policy);
            }
        }
        EventType::AddTradingPair(AddTradingPairEvent {
            book_id,
            base,
            quote,
            tick_size,
            lot_size,
            base_metadata,
            quote_metadata,
            fee_rates,
        }) => {
            let pair = order::TradingPair {
                base: *base,
                quote: *quote,
            };
            state.record_trading_pair(
                *book_id,
                pair,
                base_metadata.clone(),
                quote_metadata.clone(),
                *tick_size,
                *lot_size,
                *fee_rates,
            );
        }
        EventType::Deposit(DepositEvent {
            user,
            token,
            amount,
        }) => {
            state.deposit(*user, *token, *amount, persistence);
        }
        EventType::Withdraw(WithdrawEvent {
            block_index: _,
            user,
            token,
            amount,
        }) => {
            if matches!(persistence, StableMemoryOptions::Write) {
                state
                    .withdraw(*user, *token, *amount)
                    .expect("BUG: insufficient balance for withdraw event");
            }
        }
        EventType::AddLimitOrder(AddLimitOrderEvent {
            user,
            order_id,
            side,
            price,
            quantity,
        }) => {
            let pending = order::PendingOrder {
                side: *side,
                price: *price,
                quantity: *quantity,
            };
            let (book_id, order_seq) = order_id.into_parts();
            let order = pending.into_order(order_seq);
            state.record_limit_order(*user, book_id, order, persistence);
        }
        EventType::CancelLimitOrder(CancelLimitOrderEvent { order_id }) => {
            state.record_cancel_limit_order(*order_id, persistence);
        }
        EventType::WithdrawFees(WithdrawFeesEvent { token, amount, to }) => {
            if matches!(persistence, StableMemoryOptions::Write) {
                state
                    .drain_fees(token, *amount, *to)
                    .expect("BUG: insufficient fee balance for WithdrawFees event");
            }
        }
        EventType::Matching(event) => {
            state.record_matching_event(event, persistence);
        }
        EventType::Settling(event) => {
            state.record_settling_event(event, persistence);
        }
    }
}

pub fn replay_events<MH: Memory, MB: Memory, T: IntoIterator<Item = Event>>(
    events: T,
    order_history: OrderHistory<MH>,
    balances: TokenBalance<MB>,
    persistence: StableMemoryOptions,
) -> State<MH, MB> {
    let mut events_iter = events.into_iter();
    let mut state = match events_iter
        .next()
        .expect("the event log should not be empty")
    {
        Event {
            payload: EventType::Init(init_arg),
            ..
        } => State::new(init_arg, order_history, balances)
            .expect("BUG: state initialization should succeed"),
        other => panic!("ERROR: the first event must be an Init event, got: {other:?}"),
    };
    for event in events_iter {
        apply_state_transition(&mut state, &event.payload, persistence);
    }
    // Replaying events accumulate pending settling events
    // that must have been already consumed when being written
    // to stable memory to update user's balances or order history.
    state.pending_settling_events = VecDeque::default();
    state
}
