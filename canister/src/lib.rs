use crate::order::{Price, Quantity, Side, TokenId, TradingPair};
use dex_types::{
    AddTradingPairError, AddTradingPairRequest, DepositError, DepositRequest, DepositResponse,
    LimitOrderRequest, LimitOrderResponse, OrderStatus,
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
    let token_id = request.token_id.clone();
    // TODO(DEFI-2741): Return an error if the token is not supported by the DEX.
    let amount = request.amount.clone();
    let caller = ic_cdk::api::msg_caller();

    let deposit_response = ledger::deposit(request).await?;
    state::with_state_mut(|s| s.deposit(caller, order::TokenId::from(token_id), amount));

    Ok(deposit_response)
}

pub fn get_balance(token_id: dex_types::TokenId) -> candid::Nat {
    // TODO(DEFI-2741): Return an error if the token is not supported by the DEX.
    let caller = ic_cdk::api::msg_caller();
    state::with_state(|s| s.get_balance(caller, order::TokenId::from(token_id)))
}

pub fn add_trading_pair(request: AddTradingPairRequest) -> Result<(), AddTradingPairError> {
    if !ic_cdk::api::is_controller(&ic_cdk::api::msg_caller()) {
        return Err(AddTradingPairError::NotController);
    }
    if request.base == request.quote {
        return Err(AddTradingPairError::BaseEqualsQuote);
    }
    let pair = TradingPair {
        base: TokenId::from(request.base),
        quote: TokenId::from(request.quote),
    };
    state::with_state_mut(|s| {
        s.add_trading_pair(pair, Price::new(request.tick_size), Quantity::new(request.lot_size))
    })
}
