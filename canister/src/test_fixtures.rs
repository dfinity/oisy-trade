use dex_types::{LimitOrderRequest, Side};

pub fn limit_order_request() -> LimitOrderRequest {
    LimitOrderRequest {
        side: Side::Buy,
        price: 100,
        quantity: 10,
    }
}