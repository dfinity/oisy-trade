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

fn arb_side() -> impl Strategy<Value = crate::order::Side> {
    prop_oneof![
        Just(crate::order::Side::Buy),
        Just(crate::order::Side::Sell),
    ]
}

fn arb_price() -> impl Strategy<Value = crate::order::Price> {
    any::<u64>().prop_map(crate::order::Price::new)
}

fn arb_order_seq() -> impl Strategy<Value = crate::order::OrderSeq> {
    any::<u64>().prop_map(crate::order::OrderSeq::new)
}

fn arb_order_id() -> impl Strategy<Value = crate::order::OrderId> {
    (any::<u64>(), arb_order_seq()).prop_map(|(book_id, seq)| {
        crate::order::OrderId::new(crate::order::OrderBookId::new(book_id), seq)
    })
}

fn arb_add_limit_order_event() -> impl Strategy<Value = AddLimitOrderEvent> {
    (
        arb_principal(),
        arb_order_id(),
        arb_side(),
        arb_price(),
        arb_quantity(),
    )
        .prop_map(
            |(user, order_id, side, price, quantity)| AddLimitOrderEvent {
                user,
                order_id,
                side,
                price,
                quantity,
            },
        )
}

fn arb_event_type() -> impl Strategy<Value = EventType> {
    prop_oneof![
        arb_init_arg().prop_map(EventType::Init),
        arb_upgrade_arg().prop_map(EventType::Upgrade),
        arb_add_trading_pair_event().prop_map(EventType::AddTradingPair),
        arb_deposit_event().prop_map(EventType::Deposit),
        arb_add_limit_order_event().prop_map(EventType::AddLimitOrder),
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
