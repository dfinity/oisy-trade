use super::*;
use crate::test_fixtures::arbitrary::arb_event;
use proptest::prelude::*;

proptest! {
    #[test]
    fn should_roundtrip_cbor_encoding(event in arb_event()) {
        let bytes = event.to_bytes();
        let decoded = Event::from_bytes(bytes);
        prop_assert_eq!(event, decoded);
    }
}

/// An `Init` event persisted before the `max_settlement_units_per_event` field
/// existed (the `#[n(3)]` slot absent) decodes with that field `None`, so the
/// event log replays historic installs against the default cap.
#[test]
fn should_decode_init_arg_without_settlement_units_to_none() {
    use oisy_trade_types_internal::{InitArg, Mode};

    #[derive(minicbor::Encode)]
    struct PreSettlementUnitsInitArg {
        #[n(0)]
        mode: Mode,
        #[n(1)]
        max_orders_per_chunk: u32,
        #[n(2)]
        instruction_budget: u64,
    }

    let pre = PreSettlementUnitsInitArg {
        mode: Mode::GeneralAvailability,
        max_orders_per_chunk: 1_000,
        instruction_budget: 1_000_000_000,
    };

    let mut buf = vec![];
    minicbor::encode(&pre, &mut buf).unwrap();
    let decoded: InitArg = minicbor::decode(&buf).unwrap();

    assert_eq!(decoded.max_settlement_units_per_event, None);
    assert_eq!(decoded.mode, Mode::GeneralAvailability);
    assert_eq!(decoded.max_orders_per_chunk, 1_000);
    assert_eq!(decoded.instruction_budget, 1_000_000_000);
}

mod worst_case {
    use crate::test_fixtures::event::WorstCaseEvent;
    use ic_stable_structures::Storable;
    use strum::IntoEnumIterator;

    #[test]
    fn should_know_the_worst_case_event_size() {
        for variant in WorstCaseEvent::iter() {
            let name: &'static str = (&variant).into();
            let event = variant.worst_case_memory_event();

            let bytes = event.to_bytes();

            assert_eq!(
                bytes.len(),
                variant.expected_memory_size(),
                "{name}: serialized size mismatch"
            );
        }
    }
}
