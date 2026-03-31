use crate::order::{Price, Quantity, Side};
use dex_types::{
    DepositError, DepositRequest, DepositResponse, LimitOrderRequest, LimitOrderResponse,
    OrderStatus, Token,
};

pub mod order;
pub mod state;

mod ledger;
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

pub async fn deposit(request: DepositRequest) -> Result<DepositResponse, DepositError> {
    let token = request.token.clone();
    let amount = request.amount.clone();
    let caller = ic_cdk::api::msg_caller();

    let deposit_response = ledger::deposit(request).await?;
    state::with_state_mut(|s| s.deposit(caller, token.ledger_id, amount));

    Ok(deposit_response)
}

pub fn get_balance(token: Token) -> candid::Nat {
    let caller = ic_cdk::api::msg_caller();
    state::with_state(|s| s.get_balance(caller, token.ledger_id))
}
