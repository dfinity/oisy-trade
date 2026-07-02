pub mod event;
mod order;
pub mod tokens;

pub use order::{PlaceOrder, order};

use crate::balance::{Balance, TokenBalance};
use crate::order::{
    FeeRates, Fill, LotSize, Order, OrderBook, OrderBookId, OrderHistory, OrderSeq, PendingOrder,
    Price, Quantity, Side, TickSize, TimeInForce, TokenId, TokenMetadata, TradeHistory,
    TradingPair,
};
use crate::state;
use crate::state::StableMemoryOptions;
use crate::test_fixtures::tokens::SupportedTokens;
use crate::user::{UserId, UserRegistry};
use candid::Principal;
use ic_stable_structures::{Memory, VectorMemory};
use minicbor::Encode;
use oisy_trade_types::{AddTradingPairRequest, LimitOrderRequest, Token};
use std::iter::once;
use std::num::{NonZeroU64, NonZeroU128};

/// Tick/lot for the ICP/ckBTC-like test pair (both tokens 8 decimals).
///
/// Price is denominated in quote smallest units per **whole** base token, and a
/// fill settles to `price × quantity / 10^base_decimals`. `tick × lot = 100 ×
/// 10^6 = 10^8` is a multiple of `10^base_decimals = 10^8`, so every fill
/// settles to an exact quote amount.
pub const TICK_SIZE: TickSize = TickSize::new(NonZeroU128::new(100).unwrap());
/// Minimum order quantity: 0.01 ICP with 8 decimal places, i.e. 0.01 * 10^8.
pub const LOT_SIZE: LotSize = LotSize::new(NonZeroU64::new(1_000_000).unwrap());

/// Minimum order notional for the test pair, in quote smallest units. Set to
/// the smallest notional a 1-tick × 1-lot order produces (`100 × 10^6 / 10^8 =
/// 1`) so the existing fixtures place valid orders.
pub const MIN_NOTIONAL: Quantity = Quantity::from_u128(1);
/// Maximum order notional for the test pair, in quote smallest units. Set to
/// the maximum so the default fixtures never trip the upper bound; the bound
/// itself is still exercised (snapshot round-trip, query response). Tests that
/// assert rejection register their own pair with tight bounds.
pub const MAX_NOTIONAL: Quantity = Quantity::MAX;

/// Scales a whole-quote-per-whole-base price into the on-book representation
/// (quote smallest units per whole base token) for the 8-decimal test pair:
/// `10^quote_decimals`.
pub const PRICE_SCALE: u128 = 100_000_000;

/// A default `OrderBookId` for use in unit tests that operate on a single book.
pub const TEST_BOOK_ID: OrderBookId = OrderBookId::ZERO;

pub fn icp_metadata() -> TokenMetadata {
    TokenMetadata {
        symbol: "ICP".to_string(),
        decimals: 8,
    }
}

pub fn ckbtc_metadata() -> TokenMetadata {
    TokenMetadata {
        symbol: "ckBTC".to_string(),
        decimals: 8,
    }
}

pub fn base_metadata() -> TokenMetadata {
    TokenMetadata {
        symbol: "BASE".to_string(),
        decimals: 8,
    }
}

pub fn quote_metadata() -> TokenMetadata {
    TokenMetadata {
        symbol: "QUOTE".to_string(),
        decimals: 8,
    }
}

pub fn state() -> state::State<VectorMemory, VectorMemory> {
    state::State::new(
        oisy_trade_types_internal::InitArg {
            mode: oisy_trade_types_internal::Mode::GeneralAvailability,
            max_orders_per_chunk: oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
            instruction_budget: oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
        },
        order_history(),
        trade_history(),
        user_registry(),
        balances(),
    )
    .unwrap()
}

/// Build a fresh `State<VMem, VMem>` backed by production stable memory for
/// tests that go through `state::init_state` (i.e. the canister thread_local).
pub fn state_vmem() -> state::State<crate::storage::VMem, crate::storage::VMem> {
    state::State::new(
        oisy_trade_types_internal::InitArg {
            mode: oisy_trade_types_internal::Mode::GeneralAvailability,
            max_orders_per_chunk: oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
            instruction_budget: oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
        },
        crate::order::OrderHistory::new(
            crate::storage::order_history_memory(),
            crate::storage::user_orders_memory(),
        ),
        crate::order::TradeHistory::new(
            crate::storage::trades_memory(),
            crate::storage::trades_by_user_memory(),
        ),
        UserRegistry::new(crate::storage::user_registry_memory()),
        TokenBalance::new(crate::storage::balances_memory()),
    )
    .unwrap()
}

pub fn limit_order_request() -> LimitOrderRequest {
    LimitOrderRequest {
        pair: icp_ckbtc_trading_pair().into(),
        side: oisy_trade_types::Side::Buy,
        price: candid::Nat::from(100 * PRICE_SCALE),
        quantity: candid::Nat::from(u64::from(LOT_SIZE)),
        time_in_force: None,
    }
}

pub fn trading_pair_request(
    base_id: impl Into<oisy_trade_types::TokenId>,
    base_meta: oisy_trade_types::TokenMetadata,
    quote_id: impl Into<oisy_trade_types::TokenId>,
    quote_meta: oisy_trade_types::TokenMetadata,
) -> AddTradingPairRequest {
    AddTradingPairRequest {
        base: Token {
            id: base_id.into(),
            metadata: base_meta,
        },
        quote: Token {
            id: quote_id.into(),
            metadata: quote_meta,
        },
        tick_size: candid::Nat::from(TICK_SIZE.get()),
        lot_size: LOT_SIZE.into(),
        maker_fee_bps: 0,
        taker_fee_bps: 0,
        min_notional: MIN_NOTIONAL.into(),
        max_notional: Some(MAX_NOTIONAL.into()),
    }
}

pub fn order_book() -> OrderBook {
    OrderBook::new(
        TEST_BOOK_ID,
        TICK_SIZE,
        LOT_SIZE,
        MIN_NOTIONAL,
        Some(MAX_NOTIONAL),
        FeeRates::default(),
    )
}

pub fn icp_ckbtc_trading_pair() -> TradingPair {
    TradingPair {
        base: icp_token_id(),
        quote: ckbtc_token_id(),
    }
}

/// ICP (base, 8 decimals) / ckUSDT (quote, 6 decimals) pair for the DEFI-2901
/// worked example. Base stays ICP, so `base_scale = 10^8` is unchanged from the
/// ckBTC pair; only the quote token's decimals differ.
pub fn icp_ckusdt_trading_pair() -> TradingPair {
    TradingPair {
        base: icp_token_id(),
        quote: ckusdt_token_id(),
    }
}

pub fn ckbtc_token_id() -> TokenId {
    SupportedTokens::CKBTC.token_id().into()
}

pub fn ckusdt_token_id() -> TokenId {
    SupportedTokens::CKUSDT.token_id().into()
}

pub fn icp_token_id() -> TokenId {
    SupportedTokens::ICP.token_id().into()
}

fn gtc_order(id: u64, side: Side, price: impl Into<u128>, quantity: impl Into<u64>) -> Order {
    PendingOrder {
        side,
        price: Price::new(price.into()),
        quantity: Quantity::from(quantity.into()),
        time_in_force: TimeInForce::GoodTilCanceled,
    }
    .into_order(OrderSeq::new(id))
}

pub fn buy(id: u64, price: impl Into<u128>, quantity: impl Into<u64>) -> Order {
    gtc_order(id, Side::Buy, price, quantity)
}

pub fn sell(id: u64, price: impl Into<u128>, quantity: impl Into<u64>) -> Order {
    gtc_order(id, Side::Sell, price, quantity)
}

/// Construct a [`Fill`] for use in test assertions.
///
/// `fill_seq` is the per-book sequence the order book is expected to have minted
/// for this match. `taker` provides the taker context (seq, side, price).
/// `maker_order_seq`, `maker_price`, and `quantity` describe the fill itself.
pub fn fill(
    fill_seq: u64,
    taker: &Order,
    maker_order_seq: OrderSeq,
    maker_price: impl Into<u128>,
    quantity: impl Into<u64>,
) -> Fill {
    Fill {
        fill_seq: crate::order::FillSeq::new(fill_seq),
        taker_order_seq: taker.id(),
        taker_side: taker.side(),
        taker_price: taker.price(),
        maker_order_seq,
        maker_price: Price::new(maker_price.into()),
        quantity: Quantity::from(quantity.into()),
    }
}

pub fn all_order_types(
    price: impl Into<u128>,
    quantity: impl Into<u64>,
) -> impl Iterator<Item = Order> {
    let price = price.into();
    let quantity = quantity.into();
    once(buy(1, price, quantity)).chain(once(sell(2, price, quantity)))
}

/// Builds a fresh `State` with one trading pair (ICP/ckBTC, zero fees)
/// already registered. Used by tests that need a stage with two known
/// `TokenId`s but no balance setup. Returns the state and `(base, quote)`
/// `TokenId`s.
pub fn two_token_state() -> (state::State<VectorMemory, VectorMemory>, TokenId, TokenId) {
    let mut state = state();
    let a_id = ckbtc_token_id();
    let b_id = icp_token_id();
    state.record_trading_pair(
        OrderBookId::ZERO,
        TradingPair {
            base: a_id,
            quote: b_id,
        },
        ckbtc_metadata(),
        icp_metadata(),
        TICK_SIZE,
        LOT_SIZE,
        MIN_NOTIONAL,
        Some(MAX_NOTIONAL),
        FeeRates::default(),
    );
    (state, a_id, b_id)
}

/// Accrue `fee` units of `token` into the canister-owned fee pool by
/// running one reserved-balance `transfer` with a non-zero fee. Generic
/// over the balance memory so tests can use either in-memory or
/// production stable memory.
pub fn accrue_fee<MB: Memory>(balances: &mut TokenBalance<MB>, token: TokenId, fee: u64) {
    let alice = UserId::new(1);
    let bob = UserId::new(2);
    balances.deposit(alice, token, Quantity::from(100u64));
    balances
        .reserve(alice, &token, Quantity::from(100u64))
        .unwrap();
    balances.transfer(
        alice,
        bob,
        &token,
        Quantity::from(100u64),
        Quantity::from(fee),
    );
}

pub fn init_state_with_order_book() {
    init_state_with_order_book_and_fees(FeeRates::default());
}

pub fn init_state_with_order_book_and_fees(fee_rates: FeeRates) {
    let order_history = crate::order::OrderHistory::new(
        crate::storage::order_history_memory(),
        crate::storage::user_orders_memory(),
    );
    let trade_history = crate::order::TradeHistory::new(
        crate::storage::trades_memory(),
        crate::storage::trades_by_user_memory(),
    );
    let user_registry = UserRegistry::new(crate::storage::user_registry_memory());
    let balances = TokenBalance::new(crate::storage::balances_memory());
    state::init_state(
        state::State::new(
            oisy_trade_types_internal::InitArg {
                mode: oisy_trade_types_internal::Mode::GeneralAvailability,
                max_orders_per_chunk: oisy_trade_types_internal::DEFAULT_MAX_ORDERS_PER_CHUNK,
                instruction_budget: oisy_trade_types_internal::DEFAULT_INSTRUCTION_BUDGET,
            },
            order_history,
            trade_history,
            user_registry,
            balances,
        )
        .unwrap(),
    );
    state::with_state_mut(|s| {
        s.record_trading_pair(
            TEST_BOOK_ID,
            icp_ckbtc_trading_pair(),
            icp_metadata(),
            ckbtc_metadata(),
            TICK_SIZE,
            LOT_SIZE,
            MIN_NOTIONAL,
            Some(MAX_NOTIONAL),
            fee_rates,
        );
    });
}

pub fn balances_pair<MH: Memory, MB: Memory>(
    state: &state::State<MH, MB>,
    user: &Principal,
    pair: &TradingPair,
) -> (Balance, Balance) {
    (
        state.get_balance(user, &pair.base),
        state.get_balance(user, &pair.quote),
    )
}

/// Fund the given user with a large balance for both tokens of the default
/// trading pair so that balance checks pass in tests that don't care about
/// balance validation.
pub fn fund_user(user: Principal) {
    state::with_state_mut(|s| {
        let pair = icp_ckbtc_trading_pair();
        let amount = Quantity::from(u64::MAX);
        s.deposit(user, pair.base, amount, StableMemoryOptions::Write);
        s.deposit(user, pair.quote, amount, StableMemoryOptions::Write);
    });
}

#[cfg(test)]
pub fn place_limit_order(
    user: Principal,
    side: oisy_trade_types::Side,
    price: u128,
    quantity: u64,
) {
    crate::add_limit_order(
        LimitOrderRequest {
            pair: icp_ckbtc_trading_pair().into(),
            side,
            price: candid::Nat::from(price),
            quantity: candid::Nat::from(quantity),
            time_in_force: None,
        },
        &mocks::mock_runtime_for(user),
    )
    .unwrap();
}

pub fn order_history() -> OrderHistory<VectorMemory> {
    OrderHistory::new(VectorMemory::default(), VectorMemory::default())
}

pub fn trade_history() -> TradeHistory<VectorMemory> {
    TradeHistory::new(VectorMemory::default(), VectorMemory::default())
}

/// Asserts two [`OrderRecord`]s are equal on every field except the
/// `created_at` / `last_updated_at` timestamps, which tests assert separately.
#[track_caller]
pub fn assert_eq_ignoring_timestamp(
    actual: &crate::order::OrderRecord,
    expected: &crate::order::OrderRecord,
) {
    let normalized = crate::order::OrderRecord {
        created_at: expected.created_at,
        last_updated_at: expected.last_updated_at,
        ..actual.clone()
    };
    assert_eq!(&normalized, expected);
}

/// The persisted record for `order_id` as `owner` sees it via
/// `get_user_order`.
pub fn record_of<MH, MB>(
    state: &state::State<MH, MB>,
    owner: Principal,
    order_id: crate::order::OrderId,
) -> crate::order::OrderRecord
where
    MH: ic_stable_structures::Memory,
    MB: ic_stable_structures::Memory,
{
    state
        .get_user_order(&owner, order_id)
        .map(|(_, _, record)| record)
        .expect("order record present")
}

pub fn balances() -> TokenBalance<VectorMemory> {
    TokenBalance::new(VectorMemory::default())
}

pub fn user_registry() -> UserRegistry<VectorMemory> {
    UserRegistry::new(VectorMemory::default())
}

/// A deterministic test principal seeded by a single byte.
pub fn principal(seed: u8) -> Principal {
    Principal::from_slice(&[seed])
}

/// Construct a [`ic_cdk::call::Response`] from Candid-encoded bytes.
///
/// `Response` has a private field, but is a newtype over `Vec<u8>` with
/// identical layout. This is test-only code; the transmute is sound because
/// the struct contains a single `Vec<u8>` field.
pub fn mock_response(bytes: Vec<u8>) -> ic_cdk::call::Response {
    use ic_cdk::call::Response;
    assert_eq!(
        std::mem::size_of::<Response>(),
        std::mem::size_of::<Vec<u8>>(),
        "Response layout changed — update this helper"
    );
    unsafe { std::mem::transmute::<Vec<u8>, Response>(bytes) }
}

/// Build a Candid-encoded ledger reply for `icrc1_transfer`.
pub fn transfer_response(
    result: Result<candid::Nat, icrc_ledger_types::icrc1::transfer::TransferError>,
) -> ic_cdk::call::Response {
    mock_response(candid::encode_args((result,)).unwrap())
}

/// Build a Candid-encoded ledger reply for `icrc2_transfer_from`.
pub fn transfer_from_response(
    result: Result<candid::Nat, icrc_ledger_types::icrc2::transfer_from::TransferFromError>,
) -> ic_cdk::call::Response {
    mock_response(candid::encode_args((result,)).unwrap())
}

pub fn minicbor_encode<T>(t: &T) -> Vec<u8>
where
    for<'a> T: Encode<()>,
{
    let mut buf = vec![];
    minicbor::encode(t, &mut buf).expect("encoding should succeed");
    buf
}

#[cfg(test)]
pub mod arbitrary {
    use super::event::MAX_HALT_BOOKS;
    use super::{LOT_SIZE, TICK_SIZE};
    use crate::Timestamp;
    use crate::balance::{Balance, BalanceKey};
    use crate::order::{
        self, BasisPoint, FeeRates, Fill, FillSeq, LotSize, MatchingOutput, Order, OrderBookId,
        OrderId, OrderRecord, OrderSeq, OrderStatus, PairToken, PendingOrder, Price, Quantity,
        RemovedOrder, Side, TickSize, TimeInForce, TokenId, TokenMetadata, TradeRecord,
    };
    use crate::settlement::FillEvent;
    use crate::state::event::{
        AddLimitOrderEvent, AddTradingPairEvent, BalanceOperation, CancelLimitOrderEvent,
        DepositEvent, Event, EventType, MatchingEvent, SetHaltEvent, SettlingEvent, WithdrawEvent,
    };
    use crate::user::UserId;
    use candid::Principal;
    use minicbor::{Decode, Encode};
    use oisy_trade_types::FilterToken;
    use oisy_trade_types_internal::{InitArg, Mode, UpgradeArg};
    use proptest::collection::{SizeRange, btree_set, vec};
    use proptest::prelude::{Just, Strategy, TestCaseError, any};
    use proptest::prop_oneof;
    use proptest::{option, prop_assert_eq};
    use std::collections::BTreeSet;
    use std::num::{NonZeroU64, NonZeroU128};

    /// Strategy for a valid [`PendingOrder`] with a tick-aligned price and a
    /// lot-aligned non-zero quantity.
    pub fn arb_pending_order() -> impl Strategy<Value = PendingOrder> {
        let tick = TICK_SIZE.get();
        let lot = u64::from(LOT_SIZE);
        (
            arb_side(),
            1..1_000u64, // price in ticks
            1..1_000u64, // quantity in lots
            arb_time_in_force(),
        )
            .prop_map(
                move |(side, price_ticks, qty_lots, time_in_force)| PendingOrder {
                    side,
                    price: Price::new(price_ticks as u128 * tick),
                    quantity: Quantity::from(qty_lots * lot),
                    time_in_force,
                },
            )
    }

    /// Strategy for a valid [`Order`] with a tick-aligned price, a lot-aligned
    /// non-zero quantity, and a fuzzed `time_in_force`.
    pub fn arb_order() -> impl Strategy<Value = Order> {
        (arb_order_seq(), arb_pending_order()).prop_map(|(seq, pending)| pending.into_order(seq))
    }

    /// Strategy for a single pending order whose price falls strictly on one
    /// side of `mid_ticks`: buys in `[1, mid_ticks)`, sells in
    /// `(mid_ticks, max_ticks)`, both in tick units. The buy book and sell
    /// book never cross.
    fn arb_pending_order_around_mid(
        mid_ticks: u64,
        max_ticks: u64,
    ) -> impl Strategy<Value = PendingOrder> {
        let tick = TICK_SIZE.get();
        let lot = u64::from(LOT_SIZE);
        let bid = (1u64..mid_ticks, 1u64..100u64).prop_map(move |(p, q)| PendingOrder {
            side: Side::Buy,
            price: Price::new(p as u128 * tick),
            quantity: Quantity::from(q * lot),
            time_in_force: TimeInForce::GoodTilCanceled,
        });
        let ask = ((mid_ticks + 1)..max_ticks, 1u64..100u64).prop_map(move |(p, q)| PendingOrder {
            side: Side::Sell,
            price: Price::new(p as u128 * tick),
            quantity: Quantity::from(q * lot),
            time_in_force: TimeInForce::GoodTilCanceled,
        });
        prop_oneof![bid, ask]
    }

    /// Strategy for a pending order whose price lives on one side of a fixed
    /// spread: buys land in `[1, 99] * tick_size`, sells in `[101, 199] *
    /// tick_size`. That guarantees the full buy book and the full sell book
    /// never cross, so every generated order rests on the book.
    pub fn arb_non_matching_pending_order() -> impl Strategy<Value = PendingOrder> {
        arb_pending_order_around_mid(100, 200)
    }

    pub fn arb_non_matching_orders() -> impl Strategy<Value = Vec<PendingOrder>> {
        (2u64..500u64)
            .prop_flat_map(|mid_ticks| vec(arb_pending_order_around_mid(mid_ticks, 1000), 0..30))
    }

    /// Strategy for a valid [`Fill`] with unique order sequences.
    ///
    /// `index` must be unique per fill in a test case — it determines the order
    /// sequence numbers (taker = 2*index, maker = 2*index + 1) so they never
    /// collide across fills.
    ///
    /// Generates tick-aligned prices where `maker_price <= taker_price` for buy
    /// takers and `maker_price >= taker_price` for sell takers (matching the
    /// price-improvement semantics of the engine). Quantity is a lot-size multiple.
    pub fn arb_fill(index: u64) -> impl Strategy<Value = Fill> {
        let tick = TICK_SIZE.get();
        let lot = u64::from(LOT_SIZE);
        (
            any::<bool>(), // side: true = Buy
            1..100u64,     // price_a (in ticks)
            1..100u64,     // price_b (in ticks)
            1..10u64,      // quantity (in lots)
        )
            .prop_map(move |(is_buy, pa, pb, qty_lots)| {
                let (taker_side, taker_price, maker_price) = if is_buy {
                    let hi = pa.max(pb) as u128 * tick;
                    let lo = pa.min(pb) as u128 * tick;
                    (Side::Buy, Price::new(hi), Price::new(lo))
                } else {
                    let hi = pa.max(pb) as u128 * tick;
                    let lo = pa.min(pb) as u128 * tick;
                    (Side::Sell, Price::new(lo), Price::new(hi))
                };
                Fill {
                    fill_seq: crate::order::FillSeq::new(index),
                    taker_order_seq: OrderSeq::new(2 * index),
                    taker_side,
                    taker_price,
                    maker_order_seq: OrderSeq::new(2 * index + 1),
                    maker_price,
                    quantity: Quantity::from(qty_lots * lot),
                }
            })
    }

    /// Strategy for an arbitrary [`TradeRecord`].
    pub fn arb_trade_record() -> impl Strategy<Value = TradeRecord> {
        (
            arb_side(),
            arb_price(),
            arb_quantity(),
            arb_quantity(),
            arb_quantity(),
            arb_pair_token(),
            any::<bool>(),
            arb_timestamp(),
        )
            .prop_map(
                |(side, price, quantity, notional, fee, fee_token, is_maker, timestamp)| {
                    TradeRecord {
                        side,
                        price,
                        quantity,
                        notional,
                        fee,
                        fee_token,
                        is_maker,
                        timestamp,
                    }
                },
            )
    }

    /// Strategy for an arbitrary [`Principal`] built from a self-authenticating
    /// byte slice (up to 29 bytes), covering the full principal byte-length
    /// range that appears in canister state.
    pub fn arb_principal() -> impl Strategy<Value = Principal> {
        vec(any::<u8>(), 0..=29).prop_map(|bytes| Principal::from_slice(&bytes))
    }

    pub fn arb_user_id() -> impl Strategy<Value = UserId> {
        any::<u64>().prop_map(UserId::new)
    }

    pub fn arb_balance_key() -> impl Strategy<Value = BalanceKey> {
        (arb_principal(), arb_user_id())
            .prop_map(|(token, user)| BalanceKey::new(TokenId::new(token), user))
    }

    pub fn arb_side() -> impl Strategy<Value = Side> {
        prop_oneof![Just(Side::Buy), Just(Side::Sell)]
    }

    pub fn arb_order_status() -> impl Strategy<Value = OrderStatus> {
        prop_oneof![
            Just(OrderStatus::Pending),
            Just(OrderStatus::Open),
            Just(OrderStatus::Filled),
            Just(OrderStatus::Canceled),
            Just(OrderStatus::Expired),
        ]
    }

    pub fn arb_time_in_force() -> impl Strategy<Value = TimeInForce> {
        prop_oneof![
            Just(TimeInForce::GoodTilCanceled),
            Just(TimeInForce::FillOrKill),
        ]
    }

    pub fn arb_timestamp() -> impl Strategy<Value = Timestamp> {
        any::<u64>().prop_map(Timestamp::new)
    }

    pub fn arb_order_id() -> impl Strategy<Value = OrderId> {
        (any::<u64>(), any::<u64>())
            .prop_map(|(book, seq)| OrderId::new(OrderBookId::new(book), OrderSeq::new(seq)))
    }

    pub fn arb_quantity() -> impl Strategy<Value = Quantity> {
        // Stratified across regimes so proptests cross the carry/encoding
        // boundaries: u64-sized (CBOR u64 arm / mul fast path), u128-sized
        // (high == 0), and the full u256 range.
        prop_oneof![
            any::<u64>().prop_map(|low| Quantity::new(0, u128::from(low))),
            any::<u128>().prop_map(|low| Quantity::new(0, low)),
            (any::<u128>(), any::<u128>()).prop_map(|(high, low)| Quantity::new(high, low)),
        ]
    }

    pub fn arb_balance() -> impl Strategy<Value = Balance> {
        (arb_quantity(), arb_quantity()).prop_map(|(free, reserved)| Balance::new(free, reserved))
    }

    /// Strategy for a valid [`OrderRecord`] with a tick-aligned price and a
    /// lot-aligned non-zero quantity. `filled_quantity` is a lot multiple
    /// within `[0, quantity]`, upholding the `filled_quantity <= quantity`
    /// invariant.
    pub fn arb_order_record() -> impl Strategy<Value = OrderRecord> {
        let tick = TICK_SIZE.get();
        let lot = u64::from(LOT_SIZE);
        (
            arb_principal(),
            arb_side(),
            1..1_000u64, // price in ticks
            1..1_000u64, // quantity in lots
            arb_order_status(),
            arb_timestamp(),             // created_at
            option::of(arb_timestamp()), // last_updated_at
            arb_time_in_force(),
        )
            .prop_flat_map(
                move |(
                    owner,
                    side,
                    price_ticks,
                    qty_lots,
                    status,
                    created_at,
                    last_updated_at,
                    time_in_force,
                )| {
                    (0..=qty_lots).prop_map(move |filled_lots| {
                        let price = Price::new(price_ticks as u128 * tick);
                        let filled_quantity = Quantity::from(filled_lots * lot);
                        // Realized quote notional, derived exactly as the engine
                        // does: `maker_price × filled_quantity / base_scale`
                        // (cf. `Fill::quote_amount`), with `base_scale = 10^8`
                        // for this fixture.
                        let filled_quote = price
                            .checked_mul_quantity_scaled(
                                &filled_quantity,
                                NonZeroU64::new(100_000_000).unwrap(),
                            )
                            .expect("fixture notional fits in 256 bits");
                        OrderRecord {
                            owner,
                            side,
                            price,
                            quantity: Quantity::from(qty_lots * lot),
                            filled_quantity,
                            status,
                            created_at,
                            last_updated_at,
                            time_in_force,
                            filled_quote,
                            filled_fee: Quantity::from(u128::from(filled_lots)),
                        }
                    })
                },
            )
    }

    pub fn arb_price() -> impl Strategy<Value = Price> {
        any::<u64>().prop_map(|p| Price::new(p as u128))
    }

    pub fn arb_order_seq() -> impl Strategy<Value = OrderSeq> {
        any::<u64>().prop_map(OrderSeq::new)
    }

    pub fn arb_fill_seq() -> impl Strategy<Value = FillSeq> {
        any::<u64>().prop_map(FillSeq::new)
    }

    pub fn arb_token_id() -> impl Strategy<Value = TokenId> {
        arb_principal().prop_map(TokenId::new)
    }

    /// Strategy for an arbitrary [`FilterToken`] built over [`arb_token_id`].
    pub fn arb_filter_token() -> impl Strategy<Value = FilterToken> {
        arb_token_id().prop_map(|id| FilterToken::ById(id.into()))
    }

    /// Strategy for an arbitrary filter (`Vec<FilterToken>`) whose length
    /// falls within `size`. Pick a range straddling `MAX_FILTER_LEN` to
    /// exercise both the under-cap and over-cap branches.
    pub fn arb_filter_tokens(
        size: impl Into<SizeRange>,
    ) -> impl Strategy<Value = Vec<FilterToken>> {
        vec(arb_filter_token(), size)
    }

    pub fn arb_token_metadata() -> impl Strategy<Value = TokenMetadata> {
        ("[a-zA-Z]{1,10}", any::<u8>())
            .prop_map(|(symbol, decimals)| TokenMetadata { symbol, decimals })
    }

    pub fn arb_mode() -> impl Strategy<Value = Mode> {
        prop_oneof![
            Just(Mode::GeneralAvailability),
            btree_set(arb_principal(), 0..=5).prop_map(Mode::RestrictedTo),
        ]
    }

    pub fn arb_init_arg() -> impl Strategy<Value = InitArg> {
        // Stay within ExecutionPolicy's validation bounds so `State::new` won't panic.
        (arb_mode(), 1..=10_000u32, 1..=40_000_000_000u64).prop_map(
            |(mode, max_orders_per_chunk, instruction_budget)| InitArg {
                mode,
                max_orders_per_chunk,
                instruction_budget,
            },
        )
    }

    pub fn arb_upgrade_arg() -> impl Strategy<Value = UpgradeArg> {
        (
            option::of(arb_mode()),
            option::of(1..=10_000u32),
            option::of(1..=40_000_000_000u64),
        )
            .prop_map(
                |(mode, max_orders_per_chunk, instruction_budget)| UpgradeArg {
                    mode,
                    max_orders_per_chunk,
                    instruction_budget,
                },
            )
    }

    /// Strategy for any valid [`BasisPoint`] — uniformly sampled across the
    /// full `0..=10_000` range.
    pub fn arb_basis_point() -> impl Strategy<Value = BasisPoint> {
        (0..=10_000u16).prop_map(|v| BasisPoint::new(v).unwrap())
    }

    /// Strategy for any valid [`FeeRates`], independently sampling maker
    /// and taker rates over the full `BasisPoint` range.
    pub fn arb_fee_rates() -> impl Strategy<Value = FeeRates> {
        (arb_basis_point(), arb_basis_point()).prop_map(|(maker, taker)| FeeRates { maker, taker })
    }

    pub fn arb_add_trading_pair_event() -> impl Strategy<Value = AddTradingPairEvent> {
        (
            any::<u64>(),
            arb_principal(),
            arb_principal(),
            1..u64::MAX,
            1..u64::MAX,
            arb_token_metadata(),
            arb_token_metadata(),
            arb_fee_rates(),
            arb_quantity(),
            option::of(arb_quantity()),
        )
            .prop_map(
                |(
                    book_id,
                    base,
                    quote,
                    tick_size,
                    lot_size,
                    base_metadata,
                    quote_metadata,
                    fee_rates,
                    min_notional,
                    max_notional,
                )| AddTradingPairEvent {
                    book_id: OrderBookId::new(book_id),
                    base: TokenId::new(base),
                    quote: TokenId::new(quote),
                    tick_size: TickSize::new(NonZeroU128::new(tick_size as u128).unwrap()),
                    lot_size: LotSize::new(NonZeroU64::new(lot_size).unwrap()),
                    base_metadata,
                    quote_metadata,
                    fee_rates,
                    min_notional,
                    max_notional,
                },
            )
    }

    pub fn arb_deposit_event() -> impl Strategy<Value = DepositEvent> {
        (arb_principal(), arb_token_id(), arb_quantity()).prop_map(|(user, token, amount)| {
            DepositEvent {
                user,
                token,
                amount,
            }
        })
    }

    pub fn arb_withdraw_event() -> impl Strategy<Value = WithdrawEvent> {
        (
            any::<u64>(),
            arb_principal(),
            arb_token_id(),
            arb_quantity(),
        )
            .prop_map(|(block_index, user, token, amount)| WithdrawEvent {
                block_index,
                user,
                token,
                amount,
            })
    }

    pub fn arb_add_limit_order_event() -> impl Strategy<Value = AddLimitOrderEvent> {
        (
            arb_principal(),
            arb_order_id(),
            arb_side(),
            arb_price(),
            arb_quantity(),
            arb_time_in_force(),
        )
            .prop_map(|(user, order_id, side, price, quantity, time_in_force)| {
                AddLimitOrderEvent {
                    user,
                    order_id,
                    side,
                    price,
                    quantity,
                    time_in_force,
                }
            })
    }

    pub fn arb_cancel_limit_order_event() -> impl Strategy<Value = CancelLimitOrderEvent> {
        arb_order_id().prop_map(|order_id| CancelLimitOrderEvent { order_id })
    }

    pub fn arb_removed_order() -> impl Strategy<Value = RemovedOrder> {
        // Tick-aligned price and lot-aligned quantity so a Buy refund's
        // `price × quantity` settles exactly under the pair invariant, the same
        // alignment `arb_fill` upholds.
        let tick = TICK_SIZE.get();
        let lot = u64::from(LOT_SIZE);
        (arb_side(), 1..100u64, 1..10u64).prop_map(move |(side, price_ticks, qty_lots)| {
            RemovedOrder {
                side,
                price: Price::new(price_ticks as u128 * tick),
                remaining_quantity: Quantity::from(qty_lots * lot),
            }
        })
    }

    pub fn arb_matching_output() -> impl Strategy<Value = MatchingOutput> {
        // `arb_fill` multiplies its index by 2; cap to u32 range so 2 * index
        // fits in a u64.
        let arb_any_fill = any::<u32>().prop_flat_map(|i| arb_fill(i as u64));
        // A single order cannot be resting, filled, and expired at once, so the
        // three seq buckets must be pairwise disjoint. Draw one pool of unique
        // seqs, then deal a random prefix-split across the three buckets.
        let arb_disjoint_seqs = btree_set(any::<u64>(), 0..15).prop_flat_map(|seqs| {
            let seqs: Vec<u64> = seqs.into_iter().collect();
            let len = seqs.len();
            (Just(seqs), 0..=len, 0..=len).prop_map(|(seqs, a, b)| {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                let resting: BTreeSet<OrderSeq> =
                    seqs[..lo].iter().copied().map(OrderSeq::new).collect();
                let filled: BTreeSet<OrderSeq> =
                    seqs[lo..hi].iter().copied().map(OrderSeq::new).collect();
                let expired: Vec<OrderSeq> =
                    seqs[hi..].iter().copied().map(OrderSeq::new).collect();
                (resting, filled, expired)
            })
        });
        (vec(arb_any_fill, 0..5), arb_disjoint_seqs)
            .prop_flat_map(|(fills, (resting_orders, filled_orders, expired_seqs))| {
                let expired_len = expired_seqs.len();
                (
                    Just(fills),
                    Just(resting_orders),
                    Just(filled_orders),
                    Just(expired_seqs),
                    vec(arb_removed_order(), expired_len..=expired_len),
                )
            })
            .prop_map(
                |(fills, resting_orders, filled_orders, expired_seqs, removed)| {
                    let expired_orders = expired_seqs.into_iter().zip(removed).collect();
                    MatchingOutput {
                        fills,
                        resting_orders,
                        filled_orders,
                        expired_orders,
                    }
                },
            )
    }

    pub fn arb_matching_event() -> impl Strategy<Value = MatchingEvent> {
        (any::<u64>(), vec(arb_order_seq(), 0..5)).prop_map(|(book_id, orders)| MatchingEvent {
            book_id: order::OrderBookId::new(book_id),
            orders,
        })
    }

    pub fn arb_pair_token() -> impl Strategy<Value = PairToken> {
        prop_oneof![Just(PairToken::Base), Just(PairToken::Quote)]
    }

    pub fn arb_balance_operation() -> impl Strategy<Value = BalanceOperation> {
        // Mirror the fill path: derive `fee` from a basis point applied to
        // `amount`, then collapse zero (mul_ceil's bps=0 / amount=0 case)
        // to `None` per the `nonzero` helper in `compute_balance_operations`.
        // This guarantees `Some(fee)` ⇒ `0 < fee ≤ amount`, the same
        // invariant the production path produces.
        let transfer = (
            arb_order_seq(),
            arb_order_seq(),
            arb_pair_token(),
            arb_quantity(),
            arb_basis_point(),
        )
            .prop_map(|(from_order, to_order, token, amount, bp)| {
                let raw = bp.mul_ceil(amount);
                let fee = if raw.is_zero() { None } else { Some(raw) };
                BalanceOperation::Transfer {
                    from_order,
                    to_order,
                    token,
                    amount,
                    fee,
                }
            });
        let unreserve = (arb_order_seq(), arb_pair_token(), arb_quantity()).prop_map(
            |(order, token, amount)| BalanceOperation::Unreserve {
                order,
                token,
                amount,
            },
        );
        prop_oneof![transfer, unreserve]
    }

    /// Strategy for an arbitrary [`FillEvent`], fuzzing every field
    /// independently — the lean record persisted on a settling event.
    pub fn arb_fill_event() -> impl Strategy<Value = FillEvent> {
        (
            arb_fill_seq(),
            arb_order_seq(),
            arb_order_seq(),
            arb_quantity(),
            arb_fee_rates(),
        )
            .prop_map(
                |(fill_seq, taker_order_seq, maker_order_seq, quantity, fee_rates)| FillEvent {
                    fill_seq,
                    taker_order_seq,
                    maker_order_seq,
                    quantity,
                    fee_rates,
                },
            )
    }

    pub fn arb_settling_event() -> impl Strategy<Value = SettlingEvent> {
        (
            any::<u64>(),
            vec(arb_balance_operation(), 0..10),
            vec(arb_fill_event(), 0..10),
        )
            .prop_map(|(book_id, balance_operations, fills)| SettlingEvent {
                book_id: order::OrderBookId::new(book_id),
                balance_operations,
                fills,
            })
    }

    pub fn arb_permissions() -> impl Strategy<Value = crate::state::permissions::Permissions> {
        (
            any::<bool>(),
            btree_set(any::<u64>().prop_map(OrderBookId::new), 0..=MAX_HALT_BOOKS),
        )
            .prop_map(|(globally_halted, halted_pairs)| {
                let mut permissions = crate::state::permissions::Permissions::default();
                if globally_halted {
                    permissions.halt_trading_globally();
                }
                for book in halted_pairs {
                    permissions.halt_trading(book);
                }
                permissions
            })
    }

    /// A `Permissions` that halts trading on [`OrderBookId::ZERO`], either
    /// globally or for that pair only, paired with a distinct `other` book and
    /// the `global` flag so callers can assert per-pair isolation.
    pub fn arb_book_halted_permissions()
    -> impl Strategy<Value = (crate::state::permissions::Permissions, OrderBookId, bool)> {
        let other = (1..=u64::MAX).prop_map(OrderBookId::new);
        let global = other.clone().prop_map(|other| {
            let mut permissions = crate::state::permissions::Permissions::default();
            permissions.halt_trading_globally();
            (permissions, other, true)
        });
        let pair = other.prop_map(|other| {
            let mut permissions = crate::state::permissions::Permissions::default();
            permissions.halt_trading(OrderBookId::ZERO);
            (permissions, other, false)
        });
        prop_oneof![global, pair]
    }

    pub fn arb_set_halt_event() -> impl Strategy<Value = SetHaltEvent> {
        let book_ids = option::of(vec(
            any::<u64>().prop_map(order::OrderBookId::new),
            0..=MAX_HALT_BOOKS,
        ));
        (book_ids, any::<bool>()).prop_map(|(book_ids, halted)| SetHaltEvent { book_ids, halted })
    }

    pub fn arb_event_type() -> impl Strategy<Value = EventType> {
        prop_oneof![
            arb_init_arg().prop_map(EventType::Init),
            arb_upgrade_arg().prop_map(EventType::Upgrade),
            arb_add_trading_pair_event().prop_map(EventType::AddTradingPair),
            arb_deposit_event().prop_map(EventType::Deposit),
            arb_withdraw_event().prop_map(EventType::Withdraw),
            arb_add_limit_order_event().prop_map(EventType::AddLimitOrder),
            arb_cancel_limit_order_event().prop_map(EventType::CancelLimitOrder),
            arb_matching_event().prop_map(EventType::Matching),
            arb_settling_event().prop_map(EventType::Settling),
            arb_set_halt_event().prop_map(EventType::SetHalt),
        ]
    }

    pub fn arb_event() -> impl Strategy<Value = Event> {
        (arb_timestamp(), arb_event_type())
            .prop_map(|(timestamp, payload)| Event { timestamp, payload })
    }

    pub fn check_minicbor_roundtrip<T>(v: &T) -> Result<(), TestCaseError>
    where
        for<'a> T: PartialEq + std::fmt::Debug + Encode<()> + Decode<'a, ()>,
    {
        let mut buf = vec![];
        minicbor::encode(v, &mut buf).expect("encoding should succeed");
        let decoded = minicbor::decode(&buf).expect("decoding should succeed");
        prop_assert_eq!(v, &decoded);
        Ok(())
    }
}

#[cfg(test)]
pub mod mocks {
    use crate::Runtime;
    use crate::Timestamp;
    use candid::Principal;
    use candid::utils::ArgumentEncoder;
    use ic_cdk::call::{CallFailed, Response};
    use mockall::mock;

    pub fn mock_runtime_for(caller: Principal) -> MockRuntime {
        mock_runtime_at(caller, Timestamp::EPOCH)
    }

    pub fn mock_runtime_for_timer() -> MockRuntime {
        let mut mock = MockRuntime::new();
        mock.expect_instruction_counter().return_const(0u64);
        mock.expect_time().return_const(Timestamp::EPOCH);
        mock
    }

    /// Like [`mock_runtime_for`] but with `time()` pinned to `now`, so a test
    /// can give placement and cancellation distinct timestamps.
    pub fn mock_runtime_at(caller: Principal, now: Timestamp) -> MockRuntime {
        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(caller);
        mock.expect_time().return_const(now);
        mock.expect_instruction_counter().return_const(0u64);
        mock
    }

    mock! {
        pub Runtime {}

        #[async_trait::async_trait]
        impl Runtime for Runtime {
            #[mockall::concretize]
            async fn call_unbounded_wait<A>(
                &self,
                canister_id: Principal,
                method: &str,
                args: A,
            ) -> Result<Response, CallFailed>
            where
                A: ArgumentEncoder + Send;

            fn msg_caller(&self) -> Principal;
            fn canister_self(&self) -> Principal;
            fn is_controller(&self, principal: &Principal) -> bool;
            fn instruction_counter(&self) -> u64;
            fn time(&self) -> Timestamp;
        }
    }

    /// A test runtime that captures `call_unbounded_wait` arguments as
    /// candid-encoded bytes so tests can decode and assert on them.
    pub struct CapturingRuntime {
        caller: Principal,
        responses: std::sync::Mutex<std::collections::VecDeque<Result<Response, CallFailed>>>,
        captured_calls: std::sync::Mutex<Vec<CapturedCall>>,
    }

    pub struct CapturedCall {
        pub canister_id: Principal,
        pub method: String,
        args: Vec<u8>,
    }

    impl CapturedCall {
        pub fn decode_args<'a, T: candid::utils::ArgumentDecoder<'a>>(&'a self) -> T {
            candid::decode_args(&self.args).expect("failed to decode captured call args")
        }
    }

    impl CapturingRuntime {
        pub fn new(caller: Principal, responses: Vec<Result<Response, CallFailed>>) -> Self {
            Self {
                caller,
                responses: std::sync::Mutex::new(responses.into()),
                captured_calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        pub fn captured_calls(&self) -> std::sync::MutexGuard<'_, Vec<CapturedCall>> {
            self.captured_calls.lock().unwrap()
        }
    }

    #[async_trait::async_trait]
    impl Runtime for CapturingRuntime {
        async fn call_unbounded_wait<A>(
            &self,
            canister_id: Principal,
            method: &str,
            args: A,
        ) -> Result<Response, CallFailed>
        where
            A: ArgumentEncoder + Send,
        {
            let encoded = candid::encode_args(args).expect("failed to encode args");
            self.captured_calls.lock().unwrap().push(CapturedCall {
                canister_id,
                method: method.to_string(),
                args: encoded,
            });
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("no more pre-configured responses")
        }

        fn msg_caller(&self) -> Principal {
            self.caller
        }

        fn canister_self(&self) -> Principal {
            Principal::anonymous()
        }

        fn is_controller(&self, _principal: &Principal) -> bool {
            false
        }

        fn instruction_counter(&self) -> u64 {
            0
        }

        fn time(&self) -> Timestamp {
            Timestamp::EPOCH
        }
    }
}
