use crate::order::{Price, Quantity, Side, TokenId, TradingPair};
use dex_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, DepositError, DepositRequest,
    DepositResponse, LimitOrderRequest, LimitOrderResponse, OrderStatus, TradingPairInfo,
};
use std::num::NonZeroU64;

pub mod order;
pub mod state;

mod ledger;
#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;

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
    let book = order::OrderBook::new(
        order::TickSize::new(NonZeroU64::new(10).unwrap()),
        order::LotSize::new(NonZeroU64::new(1_000_000).unwrap()),
    );
    state::with_state_mut(|s| s.add_order_book(pair, book));
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    state::with_state(|s| s.get_order_status(order::OrderId::from(order_id)))
}

pub fn get_trading_pairs() -> Vec<TradingPairInfo> {
    state::with_state(|s| s.get_trading_pairs())
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
        s.add_trading_pair(
            pair,
            Price::new(request.tick_size),
            Quantity::new(request.lot_size),
        )
    })
}
