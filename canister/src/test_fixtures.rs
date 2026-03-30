use crate::order::{Order, OrderBook, OrderId, PendingOrder, Price, Quantity};
use dex_types::{LimitOrderRequest, Side};
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
        side: Side::Buy,
        price: 100,
        quantity: LOT_SIZE,
    }
}

pub fn order_book() -> OrderBook {
    OrderBook::new(Price::new(TICK_SIZE), Quantity::new(LOT_SIZE))
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
