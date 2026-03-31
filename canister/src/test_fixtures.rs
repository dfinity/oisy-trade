use crate::order::{
    Order, OrderBook, OrderId, PendingOrder, Price, Quantity, Side, TokenId, TradingPair,
};
use crate::state;
use candid::Principal;
use dex_types::LimitOrderRequest;
use std::iter::once;

/// ICP/BTC-like parameters from Binance.
/// Source: `GET https://api.binance.com/api/v3/exchangeInfo?symbol=ICPBTC`
///
/// Minimum price increment: 0.00000010 BTC, i.e. 10 satoshis.
pub const TICK_SIZE: u64 = 10;
/// Minimum order quantity: 0.01 ICP with 8 decimal places, i.e. 0.01 * 10^8.
pub const LOT_SIZE: u64 = 1_000_000;

pub fn limit_order_request() -> LimitOrderRequest {
    LimitOrderRequest {
        pair: dex_types::TradingPair {
            base: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
            quote: Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap(),
        },
        side: dex_types::Side::Buy,
        price: 100,
        quantity: LOT_SIZE,
    }
}

pub fn order_book() -> OrderBook {
    OrderBook::new(Price::new(TICK_SIZE), Quantity::new(LOT_SIZE))
}

pub fn icp_ckbtc_trading_pair() -> TradingPair {
    TradingPair {
        base: TokenId::new(Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap()),
        quote: TokenId::new(Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap()),
    }
}

pub fn order(id: u64, side: Side, price: u64, quantity: u64) -> Order {
    PendingOrder {
        side,
        price: Price::new(price),
        quantity: Quantity::new(quantity),
    }
    .into_order(OrderId::from(id))
}

pub fn buy(id: u64, price: u64, quantity: u64) -> Order {
    order(id, Side::Buy, price, quantity)
}

pub fn sell(id: u64, price: u64, quantity: u64) -> Order {
    order(id, Side::Sell, price, quantity)
}

pub fn all_order_types(price: u64, quantity: u64) -> impl Iterator<Item = Order> {
    once(buy(1, price, quantity)).chain(once(sell(2, price, quantity)))
}

pub fn init_state_with_order_book() {
    state::init_state();
    state::with_state_mut(|s| {
        s.add_order_book(icp_ckbtc_trading_pair(), order_book());
    });
}
