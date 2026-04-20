use super::StateSnapshot;
use crate::order::{OrderBookId, PendingOrder, Price, Quantity, Side};
use crate::state::StableMemoryOptions;
use crate::test_fixtures::{
    LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata, state as fresh_state,
};
use candid::Principal;

/// Drives `State` through a non-trivial transient shape (trading pair, two
/// user balances, a resting buy, a resting sell) and verifies that a
/// `StateSnapshot` round trip through CBOR reconstructs an identical
/// `State::PartialEq` value. Balances and order_history live in stable
/// memory and are *passed through* the snapshot (not copied into it), so
/// on restore we clone the post-mutation stable maps and hand them to
/// `into_state` to keep the comparison meaningful.
#[test]
fn should_roundtrip_state_through_snapshot() {
    let mut state = fresh_state();
    let pair = icp_ckbtc_trading_pair();
    state.record_trading_pair(
        OrderBookId::ZERO,
        pair.clone(),
        icp_metadata(),
        ckbtc_metadata(),
        TICK_SIZE,
        LOT_SIZE,
    );

    let buyer = Principal::from_slice(&[0x01]);
    let seller = Principal::from_slice(&[0x02]);
    state.deposit(
        buyer,
        pair.quote,
        Quantity::from(1_000_000_000u64),
        StableMemoryOptions::Write,
    );
    state.deposit(
        seller,
        pair.base,
        Quantity::from(1_000_000_000u64),
        StableMemoryOptions::Write,
    );

    // A resting buy (price 1 × tick) and a resting sell (price 3 × tick) at
    // non-crossing prices, so after `process_pending_orders` both `bids` and
    // `asks` hold an entry — which exercises the encode paths and the
    // on-load reconstruction of the `resting_orders` index.
    let (buy_id, buy_order) = state
        .validate_limit_order(
            buyer,
            pair.clone(),
            PendingOrder {
                side: Side::Buy,
                price: Price::new(TICK_SIZE.get()),
                quantity: Quantity::from(LOT_SIZE.get()),
            },
        )
        .unwrap();
    state.record_limit_order(
        buyer,
        buy_id.book_id(),
        buy_order,
        StableMemoryOptions::Write,
    );
    let (sell_id, sell_order) = state
        .validate_limit_order(
            seller,
            pair.clone(),
            PendingOrder {
                side: Side::Sell,
                price: Price::new(3 * TICK_SIZE.get()),
                quantity: Quantity::from(LOT_SIZE.get()),
            },
        )
        .unwrap();
    state.record_limit_order(
        seller,
        sell_id.book_id(),
        sell_order,
        StableMemoryOptions::Write,
    );
    state.process_pending_orders();

    // Round-trip via CBOR.
    let snapshot = StateSnapshot::from_state(&state);
    let mut buf = vec![];
    minicbor::encode(&snapshot, &mut buf).unwrap();
    let decoded: StateSnapshot = minicbor::decode(&buf).unwrap();
    // `balances` and `order_history` live in stable memory; the snapshot
    // intentionally doesn't copy them. Hand the post-mutation instances to
    // `into_state` to reconstruct a state that compares equal.
    let restored = decoded.into_state(state.order_history.clone(), state.balances.clone());

    assert_eq!(state, restored);
}
