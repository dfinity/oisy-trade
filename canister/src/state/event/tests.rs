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

fn arb_token_metadata() -> impl Strategy<Value = crate::order::TokenMetadata> {
    ("[a-zA-Z]{1,10}", any::<u8>())
        .prop_map(|(symbol, decimals)| crate::order::TokenMetadata { symbol, decimals })
}

fn arb_add_trading_pair_event() -> impl Strategy<Value = AddTradingPairEvent> {
    (
        any::<u64>(),
        arb_principal(),
        arb_principal(),
        1..u64::MAX,
        1..u64::MAX,
        arb_token_metadata(),
        arb_token_metadata(),
    )
        .prop_map(
            |(book_id, base, quote, tick_size, lot_size, base_metadata, quote_metadata)| {
                AddTradingPairEvent {
                    book_id: crate::order::OrderBookId::new(book_id),
                    base: crate::order::TokenId::new(base),
                    quote: crate::order::TokenId::new(quote),
                    tick_size: TickSize::new(std::num::NonZeroU64::new(tick_size).unwrap()),
                    lot_size: LotSize::new(std::num::NonZeroU64::new(lot_size).unwrap()),
                    base_metadata,
                    quote_metadata,
                }
            },
        )
}

fn arb_quantity() -> impl Strategy<Value = Quantity> {
    any::<u64>().prop_map(Quantity::from)
}

fn arb_token_id() -> impl Strategy<Value = TokenId> {
    arb_principal().prop_map(TokenId::new)
}

fn arb_deposit_event() -> impl Strategy<Value = DepositEvent> {
    (arb_principal(), arb_token_id(), arb_quantity()).prop_map(|(user, token, amount)| {
        DepositEvent {
            user,
            token,
            amount,
        }
    })
}

fn arb_event_type() -> impl Strategy<Value = EventType> {
    prop_oneof![
        arb_init_arg().prop_map(EventType::Init),
        arb_upgrade_arg().prop_map(EventType::Upgrade),
        arb_add_trading_pair_event().prop_map(EventType::AddTradingPair),
        arb_deposit_event().prop_map(EventType::Deposit),
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

mod worst_case {
    use crate::order::{LotSize, OrderBookId, Quantity, TickSize, TokenId, TokenMetadata};
    use crate::state::event::{AddTradingPairEvent, DepositEvent, Event, EventType};
    use candid::Principal;
    use dex_types_internal::{InitArg, Mode, UpgradeArg};
    use ic_stable_structures::Storable;
    use strum::IntoEnumIterator;

    #[test]
    fn should_know_the_worst_case_event_size() {
        for variant in WorstCaseEvent::iter() {
            let event = variant.worst_case_event();

            let bytes = event.to_bytes();

            assert_eq!(
                bytes.len(),
                variant.expected_size(),
                "serialized size mismatch"
            );
        }
    }

    /// Adding a new variant to `EventType` will cause a compile error in the `From` impl,
    /// reminding you to add a corresponding worst-case entry.
    #[derive(strum::EnumIter)]
    enum WorstCaseEvent {
        Init,
        Upgrade,
        AddTradingPair,
        Deposit,
    }

    impl From<&EventType> for WorstCaseEvent {
        fn from(event: &EventType) -> Self {
            match event {
                EventType::Init(_) => Self::Init,
                EventType::Upgrade(_) => Self::Upgrade,
                EventType::AddTradingPair(_) => Self::AddTradingPair,
                EventType::Deposit(_) => Self::Deposit,
            }
        }
    }

    impl WorstCaseEvent {
        fn worst_case_event(&self) -> Event {
            let principals: std::collections::BTreeSet<Principal> =
                (0u8..10).map(max_principal).collect();

            let payload = match self {
                Self::Init => EventType::Init(InitArg {
                    mode: Mode::RestrictedTo(principals),
                }),
                Self::Upgrade => EventType::Upgrade(UpgradeArg {
                    mode: Some(Mode::RestrictedTo(principals)),
                }),
                Self::AddTradingPair => EventType::AddTradingPair(AddTradingPairEvent {
                    book_id: OrderBookId::new(u64::MAX),
                    base: TokenId::new(max_principal(0)),
                    quote: TokenId::new(max_principal(1)),
                    tick_size: TickSize::new(std::num::NonZeroU64::new(u64::MAX).unwrap()),
                    lot_size: LotSize::new(std::num::NonZeroU64::new(u64::MAX).unwrap()),
                    base_metadata: TokenMetadata {
                        symbol: max_symbol(),
                        decimals: u8::MAX,
                    },
                    quote_metadata: TokenMetadata {
                        symbol: max_symbol(),
                        decimals: u8::MAX,
                    },
                }),
                Self::Deposit => EventType::Deposit(DepositEvent {
                    user: max_principal(0),
                    token: TokenId::new(max_principal(1)),
                    amount: max_quantity(),
                }),
            };
            Event {
                timestamp: u64::MAX,
                payload,
            }
        }

        fn expected_size(&self) -> usize {
            match self {
                Self::Init => 328,
                Self::Upgrade => 328,
                Self::AddTradingPair => 136,
                Self::Deposit => 96,
            }
        }
    }

    fn max_principal(seed: u8) -> Principal {
        Principal::from_slice(&[seed; 29])
    }

    fn max_symbol() -> String {
        "A".repeat(10)
    }

    fn max_quantity() -> Quantity {
        // EVM-based chains use theoretically u256,
        // but note that for example ETH has a supply of 120 million,
        // which comfortably fits in a u128 (18 decimals).
        Quantity::from(candid::Nat::from(u128::MAX))
    }
}
