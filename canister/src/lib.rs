use dex_types::{LimitOrderRequest, LimitOrderResponse, OrderStatus};

pub mod order;
pub mod state;

#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;

pub fn add_limit_order(request: LimitOrderRequest) -> LimitOrderResponse {
    let pending = order::PendingOrder {
        side: order::Side::from(request.side),
        price: order::Price::from(request.price),
        quantity: order::Quantity::from(request.quantity),
    };
    let order_id = state::with_state_mut(|s| s.add_limit_order(pending));
    LimitOrderResponse {
        order_id: u64::from(order_id),
    }
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    state::with_state(|s| s.get_order_status(order::OrderId::from(order_id)))
}
