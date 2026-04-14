use super::State;
use crate::state::event::{
    AddLimitOrderEvent, AddTradingPairEvent, DepositEvent, Event, EventType,
};
use crate::storage;
use dex_types_internal::UpgradeArg;

#[cfg(test)]
mod tests;

pub fn process_event(state: &mut State, payload: EventType) {
    apply_state_transition(state, &payload);
    storage::record_event(payload);
}

fn apply_state_transition(state: &mut State, payload: &EventType) {
    use crate::order;

    match payload {
        EventType::Init(_) => {
            panic!("BUG: state re-initialization is not allowed");
        }
        EventType::Upgrade(UpgradeArg { mode }) => {
            if let Some(mode) = mode {
                state.set_mode(mode.clone());
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
            state.deposit(*user, *token, amount.clone());
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
                quantity: quantity.clone(),
            };
            let (book_id, order_seq) = order_id.into_parts();
            let order = pending.into_order(order_seq);
            state.record_limit_order(*user, book_id, order);
        }
    }
}

pub fn replay_events<T: IntoIterator<Item = Event>>(events: T) -> State {
    let mut events_iter = events.into_iter();
    let mut state = match events_iter
        .next()
        .expect("the event log should not be empty")
    {
        Event {
            payload: EventType::Init(init_arg),
            ..
        } => State::try_from(init_arg).expect("BUG: state initialization should succeed"),
        other => panic!("ERROR: the first event must be an Init event, got: {other:?}"),
    };
    for event in events_iter {
        apply_state_transition(&mut state, &event.payload);
    }
    state
}
