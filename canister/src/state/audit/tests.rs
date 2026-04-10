use super::*;
use candid::Principal;
use dex_types_internal::{InitArg, Mode, UpgradeArg};

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
#[should_panic(expected = "the event log should not be empty")]
fn should_panic_on_empty_events() {
    replay_events(Vec::<Event>::new());
}

#[test]
#[should_panic(expected = "the first event must be an Init event")]
fn should_panic_when_first_event_is_not_init() {
    replay_events(vec![upgrade_event(None)]);
}
