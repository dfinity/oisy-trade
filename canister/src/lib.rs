use crate::order::{Price, Quantity, Side};
use dex_types::{LimitOrderRequest, LimitOrderResponse, OrderStatus, TradingPairInfo};

pub mod order;
pub mod state;

#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;

pub fn add_limit_order(_request: LimitOrderRequest) -> LimitOrderResponse {
    let order_id = state::with_state_mut(|s| {
        // TODO DEFI-2723: use value from request
        s.add_limit_order(order::PendingOrder {
            side: Side::Buy,
            price: Price::ZERO,
            quantity: Quantity::ZERO,
        })
    });
    LimitOrderResponse {
        order_id: u64::from(order_id),
    }
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    state::with_state(|s| s.get_order_status(order::OrderId::from(order_id)))
}

pub fn get_trading_pairs() -> Vec<TradingPairInfo> {
    state::with_state(|s| s.get_trading_pairs())
}
