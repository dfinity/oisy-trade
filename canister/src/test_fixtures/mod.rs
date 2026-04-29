pub mod event;

use crate::balance::{Balance, TokenBalance};
use crate::order::{
    Fill, LotSize, Order, OrderBook, OrderBookId, OrderHistory, OrderSeq, PendingOrder, Price,
    Quantity, Side, TickSize, TokenId, TokenMetadata, TradingPair,
};
use crate::state::StableMemoryOptions;
use crate::{order, state};
use candid::Principal;
use dex_types::{AddTradingPairRequest, LimitOrderRequest, Token};
use ic_stable_structures::{Memory, VectorMemory};
use std::iter::once;
use std::num::NonZeroU64;

/// ICP/BTC-like parameters from Binance.
/// Source: `GET https://api.binance.com/api/v3/exchangeInfo?symbol=ICPBTC`
///
/// Minimum price increment: 0.00000010 BTC, i.e. 10 satoshis.
pub const TICK_SIZE: TickSize = TickSize::new(NonZeroU64::new(10).unwrap());
/// Minimum order quantity: 0.01 ICP with 8 decimal places, i.e. 0.01 * 10^8.
pub const LOT_SIZE: LotSize = LotSize::new(NonZeroU64::new(1_000_000).unwrap());

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
        dex_types_internal::InitArg {
            mode: dex_types_internal::Mode::GeneralAvailability,
        },
        order_history(),
        balances(),
    )
    .unwrap()
}

/// Build a fresh `State<VMem, VMem>` backed by production stable memory for
/// tests that go through `state::init_state` (i.e. the canister thread_local).
pub fn state_vmem() -> state::State<crate::storage::VMem, crate::storage::VMem> {
    state::State::new(
        dex_types_internal::InitArg {
            mode: dex_types_internal::Mode::GeneralAvailability,
        },
        order::OrderHistory::new(crate::storage::order_history_memory()),
        TokenBalance::new(crate::storage::balances_memory()),
    )
    .unwrap()
}

pub fn limit_order_request() -> LimitOrderRequest {
    LimitOrderRequest {
        pair: icp_ckbtc_trading_pair().into(),
        side: dex_types::Side::Buy,
        price: 100,
        quantity: candid::Nat::from(u64::from(LOT_SIZE)),
    }
}

pub fn trading_pair_request(
    base_id: impl Into<dex_types::TokenId>,
    base_meta: dex_types::TokenMetadata,
    quote_id: impl Into<dex_types::TokenId>,
    quote_meta: dex_types::TokenMetadata,
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
        tick_size: TICK_SIZE.get(),
        lot_size: LOT_SIZE.get(),
    }
}

pub fn order_book() -> OrderBook {
    OrderBook::new(TEST_BOOK_ID, TICK_SIZE, LOT_SIZE)
}

pub fn icp_ckbtc_trading_pair() -> TradingPair {
    TradingPair {
        base: icp_token_id(),
        quote: ckbtc_token_id(),
    }
}

pub fn ckbtc_token_id() -> TokenId {
    TokenId::new(Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap())
}

pub fn icp_token_id() -> TokenId {
    TokenId::new(Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap())
}

fn order(id: u64, side: Side, price: impl Into<u64>, quantity: impl Into<u64>) -> Order {
    PendingOrder {
        side,
        price: Price::new(price.into()),
        quantity: Quantity::from(quantity.into()),
    }
    .into_order(OrderSeq::new(id))
}

pub fn buy(id: u64, price: impl Into<u64>, quantity: impl Into<u64>) -> Order {
    order(id, Side::Buy, price, quantity)
}

pub fn sell(id: u64, price: impl Into<u64>, quantity: impl Into<u64>) -> Order {
    order(id, Side::Sell, price, quantity)
}

/// Construct a [`Fill`] for use in test assertions.
///
/// `taker` provides the taker context (seq, side, price).
/// `maker_order_seq`, `maker_price`, and `quantity` describe the fill itself.
pub fn fill(
    taker: &Order,
    maker_order_seq: OrderSeq,
    maker_price: impl Into<u64>,
    quantity: impl Into<u64>,
) -> Fill {
    Fill {
        taker_order_seq: taker.id(),
        taker_side: taker.side(),
        taker_price: taker.price(),
        maker_order_seq,
        maker_price: Price::new(maker_price.into()),
        quantity: Quantity::from(quantity.into()),
    }
}

pub fn all_order_types(
    price: impl Into<u64>,
    quantity: impl Into<u64>,
) -> impl Iterator<Item = Order> {
    let price = price.into();
    let quantity = quantity.into();
    once(buy(1, price, quantity)).chain(once(sell(2, price, quantity)))
}

pub fn init_state_with_order_book() {
    let order_history = order::OrderHistory::new(crate::storage::order_history_memory());
    let balances = TokenBalance::new(crate::storage::balances_memory());
    state::init_state(
        state::State::new(
            dex_types_internal::InitArg {
                mode: dex_types_internal::Mode::GeneralAvailability,
            },
            order_history,
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
        );
    });
}

pub fn balances_pair<MB: Memory>(
    balances: &TokenBalance<MB>,
    user: &Principal,
    pair: &TradingPair,
) -> (Balance, Balance) {
    (
        balances.get_balance(user, &pair.base).unwrap_or_default(),
        balances.get_balance(user, &pair.quote).unwrap_or_default(),
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

/// Deposit just enough of the appropriate token to cover `side`'s reservation,
/// validate the resulting limit order, and record it. Returns the assigned
/// `OrderId`. Each call funds the user from zero, so distinct users get
/// distinct, isolated balances.
pub fn place_order<MH, MB>(
    state: &mut state::State<MH, MB>,
    user: Principal,
    pair: &TradingPair,
    side: Side,
    price: u64,
    quantity: impl Into<Quantity>,
) -> order::OrderId
where
    MH: ic_stable_structures::Memory,
    MB: ic_stable_structures::Memory,
{
    let pending = PendingOrder {
        side,
        price: Price::new(price),
        quantity: quantity.into(),
    };
    let (token, amount) = match side {
        Side::Buy => (
            pair.quote,
            pending
                .price
                .checked_mul_quantity(&pending.quantity)
                .expect("place_order: price × quantity overflow"),
        ),
        Side::Sell => (pair.base, pending.quantity),
    };
    state.deposit(user, token, amount, StableMemoryOptions::Write);
    let (order_id, order) = state
        .validate_limit_order(user, pair.clone(), pending)
        .expect("place_order: validate_limit_order failed");
    state.record_limit_order(user, order_id.book_id(), order, StableMemoryOptions::Write);
    order_id
}

#[cfg(test)]
pub fn place_limit_order(user: Principal, side: dex_types::Side, price: u64, quantity: u64) {
    crate::add_limit_order(
        LimitOrderRequest {
            pair: icp_ckbtc_trading_pair().into(),
            side,
            price,
            quantity: candid::Nat::from(quantity),
        },
        &mocks::mock_runtime_for(user),
    )
    .unwrap();
}

pub fn order_history() -> OrderHistory<VectorMemory> {
    OrderHistory::new(VectorMemory::default())
}

pub fn balances() -> TokenBalance<VectorMemory> {
    TokenBalance::new(VectorMemory::default())
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

#[cfg(test)]
pub mod arbitrary {
    use crate::balance::{Balance, BalanceKey};
    use crate::order::{
        self, CanceledOrderInfo, Fill, LotSize, MatchingOutput, OrderBookId, OrderId, OrderRecord,
        OrderSeq, OrderStatus, PairToken, PendingOrder, Price, Quantity, Side, TickSize, TokenId,
        TokenMetadata,
    };
    use crate::state::event::{
        AddLimitOrderEvent, AddTradingPairEvent, BalanceOperation, CancelLimitOrderEvent,
        DepositEvent, Event, EventType, MatchingEvent, OrderStatusTransition, SettlingEvent,
        WithdrawEvent,
    };
    use candid::Principal;
    use dex_types_internal::{InitArg, Mode, UpgradeArg};
    use proptest::collection::btree_set;
    use proptest::prelude::*;
    use std::num::NonZeroU64;

    use super::{LOT_SIZE, TICK_SIZE};

    /// Strategy for a valid [`PendingOrder`] with a tick-aligned price and a
    /// lot-aligned non-zero quantity.
    pub fn arb_pending_order() -> impl Strategy<Value = PendingOrder> {
        let tick = TICK_SIZE.get();
        let lot = u64::from(LOT_SIZE);
        (
            arb_side(),
            1..1_000u64, // price in ticks
            1..1_000u64, // quantity in lots
        )
            .prop_map(move |(side, price_ticks, qty_lots)| PendingOrder {
                side,
                price: Price::new(price_ticks * tick),
                quantity: Quantity::from(qty_lots * lot),
            })
    }

    /// Strategy for a single pending order whose price falls strictly on one
    /// side of `mid_ticks`: buys in `[1, mid_ticks)`, sells in
    /// `(mid_ticks, max_ticks)`, both in tick units. The buy book and sell
    /// book never cross.
    fn arb_pending_order_around_mid(
        mid_ticks: u64,
        max_ticks: u64,
    ) -> impl Strategy<Value = PendingOrder> {
        let tick = u64::from(TICK_SIZE);
        let lot = u64::from(LOT_SIZE);
        let bid = (1u64..mid_ticks, 1u64..100u64).prop_map(move |(p, q)| PendingOrder {
            side: Side::Buy,
            price: Price::new(p * tick),
            quantity: Quantity::from(q * lot),
        });
        let ask = ((mid_ticks + 1)..max_ticks, 1u64..100u64).prop_map(move |(p, q)| PendingOrder {
            side: Side::Sell,
            price: Price::new(p * tick),
            quantity: Quantity::from(q * lot),
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
        (2u64..500u64).prop_flat_map(|mid_ticks| {
            prop::collection::vec(arb_pending_order_around_mid(mid_ticks, 1000), 0..30)
        })
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
                    let hi = pa.max(pb) * tick;
                    let lo = pa.min(pb) * tick;
                    (Side::Buy, Price::new(hi), Price::new(lo))
                } else {
                    let hi = pa.max(pb) * tick;
                    let lo = pa.min(pb) * tick;
                    (Side::Sell, Price::new(lo), Price::new(hi))
                };
                Fill {
                    taker_order_seq: OrderSeq::new(2 * index),
                    taker_side,
                    taker_price,
                    maker_order_seq: OrderSeq::new(2 * index + 1),
                    maker_price,
                    quantity: Quantity::from(qty_lots * lot),
                }
            })
    }

    /// Strategy for an arbitrary [`Principal`] built from a self-authenticating
    /// byte slice (up to 29 bytes), covering the full principal byte-length
    /// range that appears in canister state.
    pub fn arb_principal() -> impl Strategy<Value = Principal> {
        prop::collection::vec(any::<u8>(), 0..=29).prop_map(|bytes| Principal::from_slice(&bytes))
    }

    pub fn arb_balance_key() -> impl Strategy<Value = BalanceKey> {
        (arb_principal(), arb_principal())
            .prop_map(|(token, owner)| BalanceKey::new(TokenId::new(token), owner))
    }

    pub fn arb_side() -> impl Strategy<Value = Side> {
        prop_oneof![Just(Side::Buy), Just(Side::Sell)]
    }

    pub fn arb_order_status() -> impl Strategy<Value = OrderStatus> {
        prop_oneof![
            Just(OrderStatus::Pending),
            Just(OrderStatus::Open),
            Just(OrderStatus::Filled),
            arb_quantity().prop_map(|remaining_quantity| OrderStatus::Canceled(
                CanceledOrderInfo { remaining_quantity },
            )),
        ]
    }

    pub fn arb_order_id() -> impl Strategy<Value = OrderId> {
        (any::<u64>(), any::<u64>())
            .prop_map(|(book, seq)| OrderId::new(OrderBookId::new(book), OrderSeq::new(seq)))
    }

    pub fn arb_quantity() -> impl Strategy<Value = Quantity> {
        (any::<u128>(), any::<u128>()).prop_map(|(high, low)| Quantity::new(high, low))
    }

    pub fn arb_balance() -> impl Strategy<Value = Balance> {
        (arb_quantity(), arb_quantity()).prop_map(|(free, reserved)| Balance::new(free, reserved))
    }

    /// Strategy for a valid [`OrderRecord`] with a tick-aligned price and a
    /// lot-aligned non-zero quantity.
    pub fn arb_order_record() -> impl Strategy<Value = OrderRecord> {
        let tick = TICK_SIZE.get();
        let lot = u64::from(LOT_SIZE);
        (
            arb_principal(),
            arb_side(),
            1..1_000u64, // price in ticks
            1..1_000u64, // quantity in lots
            arb_order_status(),
        )
            .prop_map(
                move |(owner, side, price_ticks, qty_lots, status)| OrderRecord {
                    owner,
                    side,
                    price: Price::new(price_ticks * tick),
                    quantity: Quantity::from(qty_lots * lot),
                    status,
                },
            )
    }

    pub fn arb_price() -> impl Strategy<Value = Price> {
        any::<u64>().prop_map(Price::new)
    }

    pub fn arb_order_seq() -> impl Strategy<Value = OrderSeq> {
        any::<u64>().prop_map(OrderSeq::new)
    }

    pub fn arb_token_id() -> impl Strategy<Value = TokenId> {
        arb_principal().prop_map(TokenId::new)
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
        arb_mode().prop_map(|mode| InitArg { mode })
    }

    pub fn arb_upgrade_arg() -> impl Strategy<Value = UpgradeArg> {
        prop::option::of(arb_mode()).prop_map(|mode| UpgradeArg { mode })
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
        )
            .prop_map(
                |(book_id, base, quote, tick_size, lot_size, base_metadata, quote_metadata)| {
                    AddTradingPairEvent {
                        book_id: OrderBookId::new(book_id),
                        base: TokenId::new(base),
                        quote: TokenId::new(quote),
                        tick_size: TickSize::new(NonZeroU64::new(tick_size).unwrap()),
                        lot_size: LotSize::new(NonZeroU64::new(lot_size).unwrap()),
                        base_metadata,
                        quote_metadata,
                    }
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

    pub fn arb_cancel_limit_order_event() -> impl Strategy<Value = CancelLimitOrderEvent> {
        arb_order_id().prop_map(|order_id| CancelLimitOrderEvent { order_id })
    }

    pub fn arb_matching_output() -> impl Strategy<Value = MatchingOutput> {
        // `arb_fill` multiplies its index by 2; cap to u32 range so 2 * index
        // fits in a u64.
        let arb_any_fill = any::<u32>().prop_flat_map(|i| arb_fill(i as u64));
        (
            prop::collection::vec(arb_any_fill, 0..5),
            btree_set(arb_order_seq(), 0..5),
            btree_set(arb_order_seq(), 0..5),
        )
            .prop_map(|(fills, resting_orders, filled_orders)| MatchingOutput {
                fills,
                resting_orders,
                filled_orders,
            })
    }

    pub fn arb_matching_event() -> impl Strategy<Value = MatchingEvent> {
        (any::<u64>(), prop::collection::vec(arb_order_seq(), 0..5)).prop_map(
            |(book_id, orders)| MatchingEvent {
                book_id: order::OrderBookId::new(book_id),
                orders,
            },
        )
    }

    pub fn arb_pair_token() -> impl Strategy<Value = PairToken> {
        prop_oneof![Just(PairToken::Base), Just(PairToken::Quote)]
    }

    pub fn arb_balance_operation() -> impl Strategy<Value = BalanceOperation> {
        let transfer = (
            arb_order_seq(),
            arb_order_seq(),
            arb_pair_token(),
            arb_quantity(),
        )
            .prop_map(
                |(from_order, to_order, token, amount)| BalanceOperation::Transfer {
                    from_order,
                    to_order,
                    token,
                    amount,
                },
            );
        let unreserve = (arb_order_seq(), arb_pair_token(), arb_quantity()).prop_map(
            |(order, token, amount)| BalanceOperation::Unreserve {
                order,
                token,
                amount,
            },
        );
        prop_oneof![transfer, unreserve]
    }

    pub fn arb_order_status_transition() -> impl Strategy<Value = OrderStatusTransition> {
        (arb_order_seq(), arb_order_status())
            .prop_map(|(seq, status)| OrderStatusTransition { seq, status })
    }

    pub fn arb_settling_event() -> impl Strategy<Value = SettlingEvent> {
        (
            any::<u64>(),
            prop::collection::vec(arb_balance_operation(), 0..10),
            prop::collection::vec(arb_order_status_transition(), 0..10),
        )
            .prop_map(|(book_id, balance_operations, transitions)| SettlingEvent {
                book_id: order::OrderBookId::new(book_id),
                balance_operations,
                transitions,
            })
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
        ]
    }

    pub fn arb_event() -> impl Strategy<Value = Event> {
        (any::<u64>(), arb_event_type())
            .prop_map(|(timestamp, payload)| Event { timestamp, payload })
    }
}

#[cfg(test)]
pub mod mocks {
    use crate::Runtime;
    use candid::Principal;
    use candid::utils::ArgumentEncoder;
    use ic_cdk::call::{CallFailed, Response};
    use mockall::mock;

    pub fn mock_runtime_for(caller: Principal) -> MockRuntime {
        let mut mock = MockRuntime::new();
        mock.expect_msg_caller().return_const(caller);
        mock.expect_time().return_const(0u64);
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
            fn time(&self) -> u64;
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

        fn time(&self) -> u64 {
            0
        }
    }
}
