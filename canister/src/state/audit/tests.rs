use super::*;
use crate::order::{
    OrderBookId, OrderId, OrderSeq, PendingOrder, Price, Quantity, Side, TokenId, TradingPair,
};
use crate::state::StableMemoryOptions;
use crate::state::event::{AddLimitOrderEvent, DepositEvent};
use crate::test_fixtures::event::{add_trading_pair_event, init_event, upgrade_event};
use crate::test_fixtures::{
    LOT_SIZE, TICK_SIZE, base_metadata, order_history, quote_metadata, state,
};
use candid::Principal;
use dex_types_internal::Mode;

#[test]
fn should_replay_init_event() {
    let normal = state();
    let replayed = replay_events(
        vec![init_event(Mode::GeneralAvailability)],
        normal.order_history.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
fn should_replay_init_then_upgrade() {
    let restricted = Mode::restricted_to(vec![Principal::from_slice(&[0x01])]);
    let mut normal = state();
    normal.set_mode(restricted.clone());

    let replayed = replay_events(
        vec![
            init_event(Mode::GeneralAvailability),
            upgrade_event(Some(restricted)),
        ],
        normal.order_history.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
fn should_replay_upgrade_without_mode_change() {
    let normal = state();
    let replayed = replay_events(
        vec![init_event(Mode::GeneralAvailability), upgrade_event(None)],
        normal.order_history.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
fn should_replay_add_trading_pair() {
    let base = Principal::from_slice(&[0x01]);
    let quote = Principal::from_slice(&[0x02]);

    let mut normal = state();
    normal.record_trading_pair(
        OrderBookId::ZERO,
        TradingPair {
            base: TokenId::new(base),
            quote: TokenId::new(quote),
        },
        base_metadata(),
        quote_metadata(),
        TICK_SIZE,
        LOT_SIZE,
    );

    let replayed = replay_events(
        vec![
            init_event(Mode::GeneralAvailability),
            add_trading_pair_event(base, quote),
        ],
        normal.order_history.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
fn should_replay_deposit() {
    let base = Principal::from_slice(&[0x01]);
    let quote = Principal::from_slice(&[0x02]);
    let user = Principal::from_slice(&[0x03]);
    let amount = 1_000_000u64;

    let mut normal = state();
    normal.record_trading_pair(
        OrderBookId::ZERO,
        TradingPair {
            base: TokenId::new(base),
            quote: TokenId::new(quote),
        },
        base_metadata(),
        quote_metadata(),
        TICK_SIZE,
        LOT_SIZE,
    );
    normal.deposit(user, TokenId::new(base), Quantity::from(amount));

    let replayed = replay_events(
        vec![
            init_event(Mode::GeneralAvailability),
            add_trading_pair_event(base, quote),
            Event {
                timestamp: 3,
                payload: EventType::Deposit(DepositEvent {
                    user,
                    token: TokenId::new(base),
                    amount: Quantity::from(amount),
                }),
            },
        ],
        normal.order_history.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
fn should_replay_add_limit_order() {
    let base = Principal::from_slice(&[0x01]);
    let quote = Principal::from_slice(&[0x02]);
    let user = Principal::from_slice(&[0x03]);
    let deposit_amount = 100_000_000u64;
    let order_price = 100u64;
    let order_quantity = 1_000_000u64;

    let pair = TradingPair {
        base: TokenId::new(base),
        quote: TokenId::new(quote),
    };
    let pending = PendingOrder {
        side: Side::Buy,
        price: Price::new(order_price),
        quantity: Quantity::from(order_quantity),
    };

    let mut normal = state();
    normal.record_trading_pair(
        OrderBookId::ZERO,
        pair.clone(),
        base_metadata(),
        quote_metadata(),
        TICK_SIZE,
        LOT_SIZE,
    );
    normal.deposit(user, TokenId::new(quote), Quantity::from(deposit_amount));
    let (order_id, order) = normal.validate_limit_order(user, pair, pending).unwrap();
    normal.record_limit_order(user, order_id.book_id(), order, StableMemoryOptions::Write);

    let replayed = replay_events(
        vec![
            init_event(Mode::GeneralAvailability),
            add_trading_pair_event(base, quote),
            Event {
                timestamp: 3,
                payload: EventType::Deposit(DepositEvent {
                    user,
                    token: TokenId::new(quote),
                    amount: Quantity::from(deposit_amount),
                }),
            },
            Event {
                timestamp: 4,
                payload: EventType::AddLimitOrder(AddLimitOrderEvent {
                    user,
                    order_id: OrderId::new(OrderBookId::ZERO, OrderSeq::new(0)),
                    side: Side::Buy,
                    price: Price::new(order_price),
                    quantity: Quantity::from(order_quantity),
                }),
            },
        ],
        normal.order_history.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
#[should_panic(expected = "the event log should not be empty")]
fn should_panic_on_empty_events() {
    replay_events(Vec::<Event>::new(), order_history());
}

#[test]
#[should_panic(expected = "the first event must be an Init event")]
fn should_panic_when_first_event_is_not_init() {
    replay_events(vec![upgrade_event(None)], order_history());
}
