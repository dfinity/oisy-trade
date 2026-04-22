use super::*;
use crate::order::{
    OrderBookId, OrderId, OrderSeq, OrderStatus, PendingOrder, Price, Quantity, Side, TokenId,
    TradingPair,
};
use crate::state::StableMemoryOptions;
use crate::state::event::{
    AddLimitOrderEvent, BalanceOperation, DepositEvent, MatchingEvent, OrderStatusTransition,
    PairToken, SettlingEvent,
};
use crate::test_fixtures::event::{add_trading_pair_event, init_event, upgrade_event};
use crate::test_fixtures::{
    LOT_SIZE, TICK_SIZE, balances, base_metadata, order_history, quote_metadata, state,
};
use candid::Principal;
use dex_types_internal::Mode;

#[test]
fn should_replay_init_event() {
    let normal = state();
    let replayed = replay_events(
        vec![init_event(Mode::GeneralAvailability)],
        normal.order_history.clone(),
        normal.balances.clone(),
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
        normal.balances.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
fn should_replay_upgrade_without_mode_change() {
    let normal = state();
    let replayed = replay_events(
        vec![init_event(Mode::GeneralAvailability), upgrade_event(None)],
        normal.order_history.clone(),
        normal.balances.clone(),
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
        normal.balances.clone(),
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
    normal.deposit(
        user,
        TokenId::new(base),
        Quantity::from(amount),
        StableMemoryOptions::Write,
    );

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
        normal.balances.clone(),
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
    normal.deposit(
        user,
        TokenId::new(quote),
        Quantity::from(deposit_amount),
        StableMemoryOptions::Write,
    );
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
        normal.balances.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
fn should_replay_matching() {
    let base = Principal::from_slice(&[0x01]);
    let quote = Principal::from_slice(&[0x02]);
    let buyer = Principal::from_slice(&[0x03]);
    let seller = Principal::from_slice(&[0x04]);
    let price = 100u64;
    let quantity = 1_000_000u64;
    let book_id = OrderBookId::ZERO;

    let pair = TradingPair {
        base: TokenId::new(base),
        quote: TokenId::new(quote),
    };

    // Build `normal` by going through the primary path: trading pair,
    // fund both users, record a crossing buy + sell, run matching.
    let mut normal = state();
    normal.record_trading_pair(
        book_id,
        pair.clone(),
        base_metadata(),
        quote_metadata(),
        TICK_SIZE,
        LOT_SIZE,
    );
    normal.deposit(
        buyer,
        TokenId::new(quote),
        Quantity::from(price * quantity),
        StableMemoryOptions::Write,
    );
    normal.deposit(
        seller,
        TokenId::new(base),
        Quantity::from(quantity),
        StableMemoryOptions::Write,
    );
    let (buy_id, buy_order) = normal
        .validate_limit_order(
            buyer,
            pair.clone(),
            PendingOrder {
                side: Side::Buy,
                price: Price::new(price),
                quantity: Quantity::from(quantity),
            },
        )
        .unwrap();
    normal.record_limit_order(
        buyer,
        buy_id.book_id(),
        buy_order,
        StableMemoryOptions::Write,
    );
    let (sell_id, sell_order) = normal
        .validate_limit_order(
            seller,
            pair.clone(),
            PendingOrder {
                side: Side::Sell,
                price: Price::new(price),
                quantity: Quantity::from(quantity),
            },
        )
        .unwrap();
    normal.record_limit_order(
        seller,
        sell_id.book_id(),
        sell_order,
        StableMemoryOptions::Write,
    );
    let runtime = crate::test_fixtures::mocks::mock_runtime_for(Principal::anonymous());
    normal.process_pending_orders(&runtime);

    // The primary path's process_pending_orders first emits a MatchingEvent
    // (with the FIFO order of pending seqs at round entry — buy then sell)
    // and then a SettlingEvent (with the MatchingOutput: one fill where sell
    // is the taker that crosses the resting buy; both orders fully filled).
    let matching_event = Event {
        timestamp: 7,
        payload: EventType::Matching(MatchingEvent {
            book_id,
            orders: vec![buy_id.seq(), sell_id.seq()],
        }),
    };
    // Sell-taker at `price` matches the resting Buy at `price` for the full
    // quantity. No price improvement → no Unreserve op. Settlement:
    //   1. maker (buyer) pays `price × quantity` quote to taker (seller).
    //   2. taker (seller) pays `quantity` base to maker (buyer).
    let settling_event = Event {
        timestamp: 8,
        payload: EventType::Settling(SettlingEvent {
            book_id,
            balance_operations: vec![
                BalanceOperation::Transfer {
                    from: buy_id.seq(),
                    to: sell_id.seq(),
                    token: PairToken::Quote,
                    amount: Quantity::from(price * quantity),
                },
                BalanceOperation::Transfer {
                    from: sell_id.seq(),
                    to: buy_id.seq(),
                    token: PairToken::Base,
                    amount: Quantity::from(quantity),
                },
            ],
            transitions: vec![
                OrderStatusTransition {
                    seq: buy_id.seq(),
                    status: OrderStatus::Filled,
                },
                OrderStatusTransition {
                    seq: sell_id.seq(),
                    status: OrderStatus::Filled,
                },
            ],
        }),
    };

    let replayed = replay_events(
        vec![
            init_event(Mode::GeneralAvailability),
            add_trading_pair_event(base, quote),
            Event {
                timestamp: 3,
                payload: EventType::Deposit(DepositEvent {
                    user: buyer,
                    token: TokenId::new(quote),
                    amount: Quantity::from(price * quantity),
                }),
            },
            Event {
                timestamp: 4,
                payload: EventType::Deposit(DepositEvent {
                    user: seller,
                    token: TokenId::new(base),
                    amount: Quantity::from(quantity),
                }),
            },
            Event {
                timestamp: 5,
                payload: EventType::AddLimitOrder(AddLimitOrderEvent {
                    user: buyer,
                    order_id: buy_id,
                    side: Side::Buy,
                    price: Price::new(price),
                    quantity: Quantity::from(quantity),
                }),
            },
            Event {
                timestamp: 6,
                payload: EventType::AddLimitOrder(AddLimitOrderEvent {
                    user: seller,
                    order_id: sell_id,
                    side: Side::Sell,
                    price: Price::new(price),
                    quantity: Quantity::from(quantity),
                }),
            },
            matching_event,
            settling_event,
        ],
        normal.order_history.clone(),
        normal.balances.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
fn should_replay_matching_with_price_improvement() {
    let base = Principal::from_slice(&[0x01]);
    let quote = Principal::from_slice(&[0x02]);
    let buyer = Principal::from_slice(&[0x03]);
    let seller = Principal::from_slice(&[0x04]);
    let maker_price = 100u64;
    let taker_price = 110u64;
    let quantity = 1_000_000u64;
    let book_id = OrderBookId::ZERO;

    let pair = TradingPair {
        base: TokenId::new(base),
        quote: TokenId::new(quote),
    };

    // Primary path: sell rests at `maker_price`, buy taker crosses at
    // `taker_price > maker_price`. Buyer reserved `taker_price × quantity`
    // quote; the fill clears at `maker_price`, so the surplus is refunded
    // via an `Unreserve` op.
    let mut normal = state();
    normal.record_trading_pair(
        book_id,
        pair.clone(),
        base_metadata(),
        quote_metadata(),
        TICK_SIZE,
        LOT_SIZE,
    );
    normal.deposit(
        buyer,
        TokenId::new(quote),
        Quantity::from(taker_price * quantity),
        StableMemoryOptions::Write,
    );
    normal.deposit(
        seller,
        TokenId::new(base),
        Quantity::from(quantity),
        StableMemoryOptions::Write,
    );
    let (sell_id, sell_order) = normal
        .validate_limit_order(
            seller,
            pair.clone(),
            PendingOrder {
                side: Side::Sell,
                price: Price::new(maker_price),
                quantity: Quantity::from(quantity),
            },
        )
        .unwrap();
    normal.record_limit_order(
        seller,
        sell_id.book_id(),
        sell_order,
        StableMemoryOptions::Write,
    );
    let (buy_id, buy_order) = normal
        .validate_limit_order(
            buyer,
            pair.clone(),
            PendingOrder {
                side: Side::Buy,
                price: Price::new(taker_price),
                quantity: Quantity::from(quantity),
            },
        )
        .unwrap();
    normal.record_limit_order(
        buyer,
        buy_id.book_id(),
        buy_order,
        StableMemoryOptions::Write,
    );
    let runtime = crate::test_fixtures::mocks::mock_runtime_for(Principal::anonymous());
    normal.process_pending_orders(&runtime);

    let matching_event = Event {
        timestamp: 7,
        payload: EventType::Matching(MatchingEvent {
            book_id,
            orders: vec![sell_id.seq(), buy_id.seq()],
        }),
    };
    let settling_event = Event {
        timestamp: 8,
        payload: EventType::Settling(SettlingEvent {
            book_id,
            balance_operations: vec![
                BalanceOperation::Transfer {
                    from: buy_id.seq(),
                    to: sell_id.seq(),
                    token: PairToken::Quote,
                    amount: Quantity::from(maker_price * quantity),
                },
                BalanceOperation::Unreserve {
                    user: buy_id.seq(),
                    token: PairToken::Quote,
                    amount: Quantity::from((taker_price - maker_price) * quantity),
                },
                BalanceOperation::Transfer {
                    from: sell_id.seq(),
                    to: buy_id.seq(),
                    token: PairToken::Base,
                    amount: Quantity::from(quantity),
                },
            ],
            transitions: vec![
                OrderStatusTransition {
                    seq: sell_id.seq(),
                    status: OrderStatus::Filled,
                },
                OrderStatusTransition {
                    seq: buy_id.seq(),
                    status: OrderStatus::Filled,
                },
            ],
        }),
    };

    let replayed = replay_events(
        vec![
            init_event(Mode::GeneralAvailability),
            add_trading_pair_event(base, quote),
            Event {
                timestamp: 3,
                payload: EventType::Deposit(DepositEvent {
                    user: buyer,
                    token: TokenId::new(quote),
                    amount: Quantity::from(taker_price * quantity),
                }),
            },
            Event {
                timestamp: 4,
                payload: EventType::Deposit(DepositEvent {
                    user: seller,
                    token: TokenId::new(base),
                    amount: Quantity::from(quantity),
                }),
            },
            Event {
                timestamp: 5,
                payload: EventType::AddLimitOrder(AddLimitOrderEvent {
                    user: seller,
                    order_id: sell_id,
                    side: Side::Sell,
                    price: Price::new(maker_price),
                    quantity: Quantity::from(quantity),
                }),
            },
            Event {
                timestamp: 6,
                payload: EventType::AddLimitOrder(AddLimitOrderEvent {
                    user: buyer,
                    order_id: buy_id,
                    side: Side::Buy,
                    price: Price::new(taker_price),
                    quantity: Quantity::from(quantity),
                }),
            },
            matching_event,
            settling_event,
        ],
        normal.order_history.clone(),
        normal.balances.clone(),
    );
    assert_eq!(replayed, normal);
}

#[test]
#[should_panic(expected = "the event log should not be empty")]
fn should_panic_on_empty_events() {
    replay_events(Vec::<Event>::new(), order_history(), balances());
}

#[test]
#[should_panic(expected = "the first event must be an Init event")]
fn should_panic_when_first_event_is_not_init() {
    replay_events(vec![upgrade_event(None)], order_history(), balances());
}
