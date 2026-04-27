use super::StateSnapshot;
use crate::order::{OrderBookId, PendingOrder, Price, Quantity, Side};
use crate::state::StableMemoryOptions;
use crate::test_fixtures::mocks::mock_runtime_for;
use crate::test_fixtures::{
    LOT_SIZE, TICK_SIZE, ckbtc_metadata, icp_ckbtc_trading_pair, icp_metadata, state as fresh_state,
};
use candid::Principal;

mod schema_stability {
    use super::super::{
        LedgerFeeEntry, PendingSettlementEntry, StateSnapshot, TokenEntry, TradingPairEntry,
    };
    use crate::order::{
        Fill, LotSize, MatchingOutput, OrderBookId, OrderBookSnapshot, OrderSeq, PendingOrder,
        Price, PriceLevel, Quantity, RestingOrder, Side, TickSize, TokenId, TokenMetadata,
        TradingPair,
    };
    use candid::{Nat, Principal};
    use dex_types_internal::Mode;
    use std::collections::BTreeSet;
    use std::num::NonZeroU64;

    /// Fixture exercising every `#[n(N)]` field reachable from `StateSnapshot`:
    /// `mode`, `next_book_id`, one `TokenEntry` (both fields), one
    /// `TradingPairEntry`, one `OrderBookSnapshot` with a `pending_orders`
    /// entry, one bid `PriceLevel` with a `RestingOrder`, one ask
    /// `PriceLevel`, one `filled_orders` entry, and one `LedgerFeeEntry`.
    fn canned_snapshot() -> StateSnapshot {
        let token_a = TokenId::new(Principal::from_slice(&[1]));
        let token_b = TokenId::new(Principal::from_slice(&[2]));
        let book_id = OrderBookId::new(7);
        let pair = TradingPair {
            base: token_a,
            quote: token_b,
        };

        let pending = PendingOrder {
            side: Side::Buy,
            price: Price::new(100),
            quantity: Quantity::from(1_000_000u64),
        }
        .into_order(OrderSeq::new(0));
        let resting_buy = RestingOrder::from(
            PendingOrder {
                side: Side::Buy,
                price: Price::new(90),
                quantity: Quantity::from(500_000u64),
            }
            .into_order(OrderSeq::new(1)),
        );
        let resting_sell = RestingOrder::from(
            PendingOrder {
                side: Side::Sell,
                price: Price::new(110),
                quantity: Quantity::from(500_000u64),
            }
            .into_order(OrderSeq::new(2)),
        );

        StateSnapshot {
            mode: Mode::GeneralAvailability,
            next_book_id: OrderBookId::new(8),
            tokens: vec![
                TokenEntry {
                    token: token_a,
                    metadata: TokenMetadata {
                        symbol: "A".to_string(),
                        decimals: 8,
                    },
                },
                TokenEntry {
                    token: token_b,
                    metadata: TokenMetadata {
                        symbol: "B".to_string(),
                        decimals: 6,
                    },
                },
            ],
            trading_pairs: vec![TradingPairEntry {
                pair: pair.clone(),
                book_id,
            }],
            order_books: vec![OrderBookSnapshot {
                id: book_id,
                next_seq: OrderSeq::new(3),
                tick_size: TickSize::new(NonZeroU64::new(10).unwrap()),
                lot_size: LotSize::new(NonZeroU64::new(1_000_000).unwrap()),
                pending_orders: vec![pending],
                bids: vec![PriceLevel {
                    price: Price::new(90),
                    orders: vec![resting_buy],
                }],
                asks: vec![PriceLevel {
                    price: Price::new(110),
                    orders: vec![resting_sell],
                }],
                filled_orders: vec![OrderSeq::new(4)],
            }],
            ledger_fee_cache: vec![LedgerFeeEntry {
                token: token_a,
                fee: Nat::from(100_000u64),
            }],
            pending_settlement: vec![PendingSettlementEntry {
                book_id,
                output: MatchingOutput {
                    fills: vec![Fill {
                        taker_order_seq: OrderSeq::new(5),
                        taker_side: Side::Buy,
                        taker_price: Price::new(100),
                        maker_order_seq: OrderSeq::new(6),
                        maker_price: Price::new(100),
                        quantity: Quantity::from(1_000_000u64),
                    }],
                    resting_orders: BTreeSet::new(),
                    filled_orders: BTreeSet::from([OrderSeq::new(5), OrderSeq::new(6)]),
                },
            }],
        }
    }

    fn from_hex(s: &str) -> Vec<u8> {
        assert!(s.len().is_multiple_of(2), "hex string length must be even");
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("invalid hex digit"))
            .collect()
    }

    /// Hex-encoded CBOR of [`canned_snapshot`]. Guards the on-wire schema —
    /// any change that reorders/renumbers `#[n(N)]` fields, renames a
    /// `with = …` adapter, or alters the encoded shape of a referenced type
    /// will cause [`should_match_golden_encoding`] to fail and print the
    /// current hex for pasting back here if the drift was intentional.
    const GOLDEN_HEX: &str = "\
        87820080810882828141018261410882814102826142068182828141018141028107818881078103\
        810a811a000f4240818481008200808118641a000f4240818281185a818281011a0007a120818281\
        186e818281021a0007a12081810481828141011a000186a081828107838186810582008081186481\
        068118641a000f4240808281058106";

    #[test]
    fn should_match_golden_encoding() {
        let expected = canned_snapshot();
        let mut encoded = vec![];
        minicbor::encode(&expected, &mut encoded).expect("encoding should succeed");

        let golden = from_hex(GOLDEN_HEX);
        if encoded != golden {
            let current: String = encoded.iter().map(|b| format!("{:02x}", b)).collect();
            panic!(
                "CBOR schema drift — encoded bytes differ from GOLDEN_HEX. \
                 If the change is intentional, update GOLDEN_HEX to:\n{current}"
            );
        }

        let decoded: StateSnapshot = minicbor::decode(&golden).expect("decoding should succeed");
        assert_eq!(decoded, expected);
    }
}

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
    state.process_pending_orders(&mock_runtime_for(Principal::anonymous()));

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

/// Transient guard sets (`active_tasks`, `in_flight_user_ops`) are
/// intentionally excluded from the snapshot and reset to empty on restore.
#[test]
fn should_drop_transient_guard_sets_on_roundtrip() {
    let mut state = fresh_state();
    let user = Principal::from_slice(&[0x01]);
    let token = crate::order::TokenId::new(Principal::from_slice(&[0xAA]));

    state
        .active_tasks_mut()
        .insert(crate::Task::ProcessPendingOrders);
    state.in_flight_user_ops_mut().insert((user, token));

    let snapshot = StateSnapshot::from_state(&state);
    let mut buf = vec![];
    minicbor::encode(&snapshot, &mut buf).unwrap();
    let decoded: StateSnapshot = minicbor::decode(&buf).unwrap();
    let restored = decoded.into_state(state.order_history.clone(), state.balances.clone());

    assert!(
        restored.in_flight_user_ops().is_empty(),
        "in_flight_user_ops must be empty after restore"
    );
    // `active_tasks` is also dropped, so the populated original cannot equal
    // the restored state.
    assert_ne!(state, restored);
}
