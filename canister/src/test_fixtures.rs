use crate::order::{
    LotSize, Order, OrderBook, OrderBookId, OrderSeq, PendingOrder, Price, Quantity, Side,
    TickSize, TokenId, TradingPair,
};
use crate::state;
use candid::Principal;
use dex_types::LimitOrderRequest;
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

pub fn limit_order_request() -> LimitOrderRequest {
    LimitOrderRequest {
        pair: icp_ckbtc_trading_pair().into(),
        side: dex_types::Side::Buy,
        price: 100,
        quantity: u64::from(LOT_SIZE),
    }
}

pub fn order_book() -> OrderBook {
    OrderBook::new(TEST_BOOK_ID, TICK_SIZE, LOT_SIZE)
}

pub fn icp_ckbtc_trading_pair() -> TradingPair {
    TradingPair {
        base: TokenId::new(Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap()),
        quote: TokenId::new(Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap()),
    }
}

fn order(id: u64, side: Side, price: impl Into<u64>, quantity: impl Into<u64>) -> Order {
    PendingOrder {
        side,
        price: Price::new(price.into()),
        quantity: Quantity::new(quantity.into()),
    }
    .into_order(OrderSeq::new(id))
}

pub fn buy(id: u64, price: impl Into<u64>, quantity: impl Into<u64>) -> Order {
    order(id, Side::Buy, price, quantity)
}

pub fn sell(id: u64, price: impl Into<u64>, quantity: impl Into<u64>) -> Order {
    order(id, Side::Sell, price, quantity)
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
    state::init_state(dex_types_internal::InitArg {
        mode: dex_types_internal::Mode::GeneralAvailability,
    });
    state::with_state_mut(|s| {
        s.add_trading_pair(icp_ckbtc_trading_pair(), TICK_SIZE, LOT_SIZE)
            .unwrap();
    });
}

/// Fund the given user with a large balance for both tokens of the default
/// trading pair so that balance checks pass in tests that don't care about
/// balance validation.
pub fn fund_user(user: Principal) {
    state::with_state_mut(|s| {
        let pair = icp_ckbtc_trading_pair();
        let amount = candid::Nat::from(u64::MAX);
        s.deposit(user, pair.base, amount.clone());
        s.deposit(user, pair.quote, amount);
    });
}

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
        }
    }
}
