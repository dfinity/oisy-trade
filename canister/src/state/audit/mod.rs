use super::State;
use crate::state::event::{Event, EventType};
use crate::storage;

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
        EventType::Upgrade(upgrade_arg) => {
            if let Some(mode) = upgrade_arg.mode.clone() {
                state.set_mode(mode);
            }
        }
        EventType::AddTradingPair(event) => {
            let pair = order::TradingPair {
                base: order::TokenId::new(event.base),
                quote: order::TokenId::new(event.quote),
            };
            state
                .add_trading_pair(
                    pair,
                    event.base_metadata.clone(),
                    event.quote_metadata.clone(),
                    event.tick_size,
                    event.lot_size,
                )
                .expect("BUG: replaying AddTradingPair event should succeed");
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
