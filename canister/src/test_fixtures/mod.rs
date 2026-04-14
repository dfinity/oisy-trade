pub mod event;

use crate::order::{
    Fill, LotSize, Order, OrderBook, OrderBookId, OrderSeq, PendingOrder, Price, Quantity, Side,
    TickSize, TokenId, TokenMetadata, TradingPair,
};
use crate::state;
use candid::Principal;
use dex_types::{AddTradingPairRequest, LimitOrderRequest, Token};
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

pub fn state() -> state::State {
    state::State::try_from(dex_types_internal::InitArg {
        mode: dex_types_internal::Mode::GeneralAvailability,
    })
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
    state::init_state(
        state::State::try_from(dex_types_internal::InitArg {
            mode: dex_types_internal::Mode::GeneralAvailability,
        })
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

/// Fund the given user with a large balance for both tokens of the default
/// trading pair so that balance checks pass in tests that don't care about
/// balance validation.
pub fn fund_user(user: Principal) {
    state::with_state_mut(|s| {
        let pair = icp_ckbtc_trading_pair();
        let amount = Quantity::from(u64::MAX);
        s.deposit(user, pair.base, amount.clone());
        s.deposit(user, pair.quote, amount);
    });
}

#[cfg(test)]
pub mod arbitrary {
    use crate::order::{Fill, OrderSeq, Price, Quantity, Side};
    use proptest::prelude::*;

    use super::{LOT_SIZE, TICK_SIZE};

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
}

#[cfg(test)]
pub mod mocks {
    use crate::Runtime;
    use candid::Principal;
    use candid::utils::ArgumentEncoder;
    use ic_cdk::call::{CallFailed, Response};
    use mockall::mock;

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
        }
    }
}
