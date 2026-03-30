use dex_types::{LimitOrderRequest, LimitOrderResponse, OrderStatus, Token};

pub mod order;
pub mod state;

#[cfg(test)]
mod tests;

pub fn add_limit_order(_request: LimitOrderRequest) -> LimitOrderResponse {
    let order_id = state::with_state_mut(|s| s.add_limit_order(order::PendingOrder {}));
    LimitOrderResponse {
        order_id: u64::from(order_id),
    }
}

pub fn add_supported_token(token: Token) {
    state::with_state_mut(|s| s.add_supported_token(token));
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    state::with_state(|s| s.get_order_status(order::OrderId::from(order_id)))
}

pub fn get_supported_tokens() -> Vec<Token> {
    state::with_state(|s| s.get_supported_tokens())
}
