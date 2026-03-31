use dex_types::{AddLimitOrderError, LimitOrderRequest, OrderId, OrderStatus};

pub mod guard;
pub mod order;
pub mod state;

#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;

#[derive(Copy, Clone, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub enum Task {
    ProcessPendingOrders,
}

pub fn add_limit_order(request: LimitOrderRequest) -> Result<OrderId, AddLimitOrderError> {
    let pair = order::TradingPair::from(request.pair);
    let pending = order::PendingOrder {
        side: order::Side::from(request.side),
        price: order::Price::from(request.price),
        quantity: order::Quantity::from(request.quantity),
    };
    let order_id = state::with_state_mut(|s| s.add_limit_order(pair, pending))
        .map_err(AddLimitOrderError::from)?;
    Ok(u64::from(order_id))
}

/// Register default trading pairs for testing.
/// TODO DEFI-2744: replace with an admin endpoint.
pub fn register_default_trading_pairs() {
    use candid::Principal;
    let pair = order::TradingPair {
        base: order::TokenId::new(Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap()),
        quote: order::TokenId::new(Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap()),
    };
    let book = order::OrderBook::new(order::Price::new(10), order::Quantity::new(1_000_000));
    state::with_state_mut(|s| s.add_order_book(pair, book));
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    state::with_state(|s| s.get_order_status(order::OrderId::from(order_id)))
}
