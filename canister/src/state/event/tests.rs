use super::*;
use candid::Principal;
use dex_types_internal::{InitArg, Mode, UpgradeArg};
use proptest::collection::btree_set;
use proptest::prelude::*;

fn arb_principal() -> impl Strategy<Value = Principal> {
    prop::collection::vec(any::<u8>(), 0..=29).prop_map(|bytes| Principal::from_slice(&bytes))
}

fn arb_mode() -> impl Strategy<Value = Mode> {
    prop_oneof![
        Just(Mode::GeneralAvailability),
        btree_set(arb_principal(), 0..=5).prop_map(Mode::RestrictedTo),
    ]
}

fn arb_init_arg() -> impl Strategy<Value = InitArg> {
    arb_mode().prop_map(|mode| InitArg { mode })
}

fn arb_upgrade_arg() -> impl Strategy<Value = UpgradeArg> {
    prop::option::of(arb_mode()).prop_map(|mode| UpgradeArg { mode })
}

fn arb_add_trading_pair_event() -> impl Strategy<Value = AddTradingPairEvent> {
    (
        arb_principal(),
        arb_principal(),
        1..u64::MAX,
        1..u64::MAX,
        "[a-zA-Z]{1,10}",
        any::<u8>(),
        "[a-zA-Z]{1,10}",
        any::<u8>(),
    )
        .prop_map(
            |(
                base,
                quote,
                tick_size,
                lot_size,
                base_symbol,
                base_decimals,
                quote_symbol,
                quote_decimals,
            )| {
                AddTradingPairEvent {
                    base,
                    quote,
                    tick_size: TickSize::new(std::num::NonZeroU64::new(tick_size).unwrap()),
                    lot_size: LotSize::new(std::num::NonZeroU64::new(lot_size).unwrap()),
                    base_symbol,
                    base_decimals,
                    quote_symbol,
                    quote_decimals,
                }
            },
        )
}

fn arb_event_type() -> impl Strategy<Value = EventType> {
    prop_oneof![
        arb_init_arg().prop_map(EventType::Init),
        arb_upgrade_arg().prop_map(EventType::Upgrade),
        arb_add_trading_pair_event().prop_map(EventType::AddTradingPair),
    ]
}

fn arb_event() -> impl Strategy<Value = Event> {
    (any::<u64>(), arb_event_type()).prop_map(|(timestamp, payload)| Event { timestamp, payload })
}

proptest! {
    #[test]
    fn should_roundtrip_cbor_encoding(event in arb_event()) {
        let bytes = event.to_bytes();
        let decoded = Event::from_bytes(bytes);
        prop_assert_eq!(event, decoded);
    }
}
