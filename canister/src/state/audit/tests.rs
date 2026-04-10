use super::*;
use crate::order::{LotSize, TickSize};
use crate::state::event::AddTradingPairEvent;
use candid::Principal;
use dex_types_internal::{InitArg, Mode, UpgradeArg};
use std::num::NonZeroU64;

fn init_event(mode: Mode) -> Event {
    Event {
        timestamp: 0,
        payload: EventType::Init(InitArg { mode }),
    }
}

fn upgrade_event(mode: Option<Mode>) -> Event {
    Event {
        timestamp: 1,
        payload: EventType::Upgrade(UpgradeArg { mode }),
    }
}

fn add_trading_pair_event(base: Principal, quote: Principal) -> Event {
    Event {
        timestamp: 2,
        payload: EventType::AddTradingPair(AddTradingPairEvent {
            base,
            quote,
            tick_size: TickSize::new(NonZeroU64::new(10).unwrap()),
            lot_size: LotSize::new(NonZeroU64::new(1_000_000).unwrap()),
        }),
    }
}

#[test]
fn should_replay_init_event() {
    let state = replay_events(vec![init_event(Mode::GeneralAvailability)]);
    let expected = State::try_from(InitArg {
        mode: Mode::GeneralAvailability,
    })
    .unwrap();
    assert_eq!(state, expected);
}

#[test]
fn should_replay_init_then_upgrade() {
    let restricted = Mode::restricted_to(vec![Principal::from_slice(&[0x01])]);
    let state = replay_events(vec![
        init_event(Mode::GeneralAvailability),
        upgrade_event(Some(restricted.clone())),
    ]);
    let mut expected = State::try_from(InitArg {
        mode: Mode::GeneralAvailability,
    })
    .unwrap();
    expected.set_mode(restricted);
    assert_eq!(state, expected);
}

#[test]
fn should_replay_upgrade_without_mode_change() {
    let state = replay_events(vec![
        init_event(Mode::GeneralAvailability),
        upgrade_event(None),
    ]);
    let expected = State::try_from(InitArg {
        mode: Mode::GeneralAvailability,
    })
    .unwrap();
    assert_eq!(state, expected);
}

#[test]
fn should_replay_add_trading_pair() {
    let base = Principal::from_slice(&[0x01]);
    let quote = Principal::from_slice(&[0x02]);
    let state = replay_events(vec![
        init_event(Mode::GeneralAvailability),
        add_trading_pair_event(base, quote),
    ]);
    assert_eq!(state.get_trading_pairs().len(), 1);
}

#[test]
#[should_panic(expected = "the event log should not be empty")]
fn should_panic_on_empty_events() {
    replay_events(Vec::<Event>::new());
}

#[test]
#[should_panic(expected = "the first event must be an Init event")]
fn should_panic_when_first_event_is_not_init() {
    replay_events(vec![upgrade_event(None)]);
}
