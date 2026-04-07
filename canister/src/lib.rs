use crate::order::TokenId;
use dex_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, DepositError, DepositRequest,
    DepositResponse, LimitOrderRequest, OrderId, OrderStatus, TradingPairInfo,
};
use std::{num::NonZeroU64, time::Duration};

pub use runtime::{IC_RUNTIME, Runtime};

pub mod balance;
pub mod guard;
pub mod order;
pub mod runtime;
pub mod state;

mod ledger;
#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;

pub const MATCHING_INTERVAL: Duration = Duration::from_mins(1);

#[derive(Copy, Clone, Eq, PartialEq, Debug, Ord, PartialOrd)]
pub enum Task {
    ProcessPendingOrders,
}

pub fn add_limit_order(
    request: LimitOrderRequest,
    runtime: &impl Runtime,
) -> Result<OrderId, AddLimitOrderError> {
    state::with_state(|s| s.assert_caller_is_allowed(runtime));
    let pair = order::TradingPair::from(request.pair);
    let pending = order::PendingOrder {
        side: order::Side::from(request.side),
        price: order::Price::from(request.price),
        quantity: order::Quantity::from(request.quantity),
    };
    let order_id = state::with_state_mut(|s| s.add_limit_order(pair, pending))
        .map_err(AddLimitOrderError::from)?;
    // Trigger matching, no need to wait for the timer to fire
    ic_cdk_timers::set_timer(Duration::ZERO, async {
        process_pending_orders();
    });
    Ok(order_id.to_string())
}

pub fn process_pending_orders() {
    let _guard = match guard::TimerGuard::new(Task::ProcessPendingOrders) {
        Some(guard) => guard,
        None => return,
    };

    state::with_state_mut(|s| s.process_pending_orders());
}

/// Register default trading pairs for testing.
/// TODO DEFI-2744: replace with an admin endpoint.
pub fn register_default_trading_pairs() {
    use candid::Principal;
    let pair = order::TradingPair {
        base: order::TokenId::new(Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap()),
        quote: order::TokenId::new(Principal::from_text("mxzaz-hqaaa-aaaar-qaada-cai").unwrap()),
    };
    state::with_state_mut(|s| {
        s.add_trading_pair(
            pair,
            order::TickSize::new(NonZeroU64::new(10).unwrap()),
            order::LotSize::new(NonZeroU64::new(1_000_000).unwrap()),
        )
        .unwrap()
    });
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    match order_id.parse::<order::OrderId>() {
        Ok(id) => state::with_state(|s| s.get_order_status(id)),
        Err(e) => panic!("ERROR: invalid order id: {}", e),
    }
}

pub fn get_trading_pairs() -> Vec<TradingPairInfo> {
    state::with_state(|s| s.get_trading_pairs())
}

pub async fn deposit(
    request: DepositRequest,
    runtime: &impl Runtime,
) -> Result<DepositResponse, DepositError> {
    state::with_state(|s| s.assert_caller_is_allowed(runtime));
    let token_id = request.token_id.clone();
    // TODO(DEFI-2741): Return an error if the token is not supported by the DEX.
    let amount = request.amount.clone();
    let caller = runtime.msg_caller();

    let deposit_response = ledger::deposit(request, runtime).await?;
    state::with_state_mut(|s| s.deposit(caller, order::TokenId::from(token_id), amount));

    Ok(deposit_response)
}

pub fn get_balance(token_id: dex_types::TokenId, runtime: &impl Runtime) -> dex_types::Balance {
    // TODO(DEFI-2741): Return an error if the token is not supported by the DEX.
    let caller = runtime.msg_caller();
    state::with_state(|s| s.get_balance(caller, order::TokenId::from(token_id)))
}

pub fn add_trading_pair(
    request: AddTradingPairRequest,
    runtime: &impl Runtime,
) -> Result<(), AddTradingPairError> {
    if !runtime.is_controller(&runtime.msg_caller()) {
        return Err(AddTradingPairError::NotController);
    }
    if request.base == request.quote {
        return Err(AddTradingPairError::BaseEqualsQuote);
    }
    let pair = order::TradingPair {
        base: TokenId::from(request.base),
        quote: TokenId::from(request.quote),
    };
    let tick_size = order::TickSize::new(
        NonZeroU64::new(request.tick_size).ok_or(AddTradingPairError::InvalidTickSize)?,
    );
    let lot_size = order::LotSize::new(
        NonZeroU64::new(request.lot_size).ok_or(AddTradingPairError::InvalidLotSize)?,
    );
    state::with_state_mut(|s| s.add_trading_pair(pair, tick_size, lot_size))
}
