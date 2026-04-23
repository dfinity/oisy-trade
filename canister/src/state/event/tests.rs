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
