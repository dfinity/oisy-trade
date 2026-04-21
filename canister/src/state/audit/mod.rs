use super::{StableMemoryOptions, State};
use crate::Runtime;
use crate::balance::TokenBalance;
use crate::order::OrderHistory;
use crate::state::event::{
    AddLimitOrderEvent, AddTradingPairEvent, DepositEvent, Event, EventType,
};
use crate::storage;
use dex_types_internal::UpgradeArg;
use ic_stable_structures::Memory;

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
        EventType::Upgrade(UpgradeArg { mode: new_mode }) => {
            if let Some(new_mode) = new_mode {
                state.set_mode(new_mode.clone());
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
            );
        }
        EventType::Deposit(DepositEvent {
            user,
            token,
            amount,
        }) => {
            state.deposit(*user, *token, *amount, persistence);
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
        EventType::Matching(_) => {
            // Matching and balance settlement mutate state directly inside
            // `State::process_pending_orders`; the event is record-only.
            // `replay_events` is currently dead code — if ever reintroduced,
            // this arm would need to replay `settle_fill` for each
            // `FillEvent`, resolving principals/prices from `OrderHistory`
            // (which survives upgrades via stable memory).
            let _ = persistence;
        }
    }
}

pub fn replay_events<MH: Memory, MB: Memory, T: IntoIterator<Item = Event>>(
    events: T,
    order_history: OrderHistory<MH>,
    balances: TokenBalance<MB>,
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
        apply_state_transition(&mut state, &event.payload, StableMemoryOptions::Skip);
    }
    state
}
