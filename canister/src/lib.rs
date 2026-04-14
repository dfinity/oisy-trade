use dex_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, DepositError, DepositRequest,
    DepositResponse, LimitOrderRequest, OrderId, OrderStatus, TradingPairInfo, WithdrawError,
    WithdrawRequest, WithdrawResponse,
};
use std::{num::NonZeroU64, time::Duration};

pub use runtime::{IC_RUNTIME, Runtime};

pub mod balance;
pub mod cbor;
pub mod guard;
pub mod lifecycle;
pub mod order;
pub mod runtime;
pub mod state;
pub mod storage;

#[cfg(feature = "canbench-rs")]
mod benchmarks;
mod ledger;
#[cfg(test)]
pub mod test_fixtures;
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
    let caller = runtime.msg_caller();
    let pair = order::TradingPair::from(request.pair);
    let pending = order::PendingOrder {
        side: order::Side::from(request.side),
        price: order::Price::from(request.price),
        quantity: order::Quantity::from(request.quantity),
    };
    let order_id = state::with_state_mut(|s| s.add_limit_order(caller, pair, pending))
        .map_err(AddLimitOrderError::from)?;
    Ok(order_id.to_string())
}

pub fn process_pending_orders() {
    let _guard = match guard::TimerGuard::new(Task::ProcessPendingOrders) {
        Some(guard) => guard,
        None => return,
    };

    state::with_state_mut(|s| s.process_pending_orders());
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    match order_id.parse::<order::OrderId>() {
        Ok(id) => state::with_state(|s| s.get_order_status(id)),
        Err(e) => panic!("ERROR: invalid order id: {}", e),
    }
}

pub fn get_trading_pairs() -> Vec<TradingPairInfo> {
    state::with_state(|s| {
        s.trading_pairs()
            .iter()
            .map(|(pair, book_id)| {
                let book = s
                    .order_book(book_id)
                    .expect("BUG: trading pair registered but order book missing");
                let base_meta = s
                    .token_metadata(&pair.base)
                    .expect("BUG: trading pair registered but base token metadata missing");
                let quote_meta = s
                    .token_metadata(&pair.quote)
                    .expect("BUG: trading pair registered but quote token metadata missing");
                TradingPairInfo {
                    base: dex_types::Token {
                        id: dex_types::TokenId::from(pair.base),
                        metadata: base_meta.clone().into(),
                    },
                    quote: dex_types::Token {
                        id: dex_types::TokenId::from(pair.quote),
                        metadata: quote_meta.clone().into(),
                    },
                    tick_size: book.tick_size().get(),
                    lot_size: book.lot_size().get(),
                }
            })
            .collect()
    })
}

pub async fn deposit(
    request: DepositRequest,
    runtime: &impl Runtime,
) -> Result<DepositResponse, DepositError> {
    state::with_state(|s| s.assert_caller_is_allowed(runtime));
    let token_id = request.token_id.clone();
    let internal_token = order::TokenId::from(token_id.clone());
    if !state::with_state(|s| s.is_known_token(&internal_token)) {
        return Err(DepositError::UnsupportedToken { token_id });
    }
    let amount = order::Quantity::from(request.amount.clone());
    let caller = runtime.msg_caller();

    let deposit_response = ledger::deposit(request, runtime).await?;
    let event = state::event::DepositEvent {
        user: caller,
        token: order::TokenId::from(token_id),
        amount,
    };
    state::with_state_mut(|s| {
        state::audit::process_event(s, state::event::EventType::Deposit(event))
    });

    Ok(deposit_response)
}

pub async fn withdraw(
    request: WithdrawRequest,
    runtime: &impl Runtime,
) -> Result<WithdrawResponse, WithdrawError> {
    state::with_state(|s| s.assert_caller_is_allowed(runtime));
    let token_id = request.token_id.clone();
    let internal_token = order::TokenId::from(token_id.clone());
    if !state::with_state(|s| s.is_known_token(&internal_token)) {
        return Err(WithdrawError::UnsupportedToken { token_id });
    }
    let cached_fee = state::with_state(|s| s.get_cached_ledger_fee(&internal_token));

    if request.amount == 0u64 {
        return Err(WithdrawError::AmountTooSmall {
            min_amount: cached_fee + 1u64,
        });
    }

    let caller = runtime.msg_caller();
    let amount = order::Quantity::from(request.amount.clone());

    // Debit the full amount from the user's free balance.
    state::with_state_mut(|s| s.withdraw(caller, internal_token, amount.clone())).map_err(|e| {
        WithdrawError::InsufficientBalance {
            available: e.available.into(),
        }
    })?;

    // Perform the ledger transfer (with automatic BadFee retry).
    let outcome = ledger::withdraw(&token_id, caller, request.amount, cached_fee, runtime).await;

    // Update the fee cache when a BadFee revealed a new fee, regardless of success/failure.
    if let Some(fee) = outcome.ledger_fee {
        state::with_state_mut(|s| {
            s.set_cached_ledger_fee(order::TokenId::from(token_id.clone()), fee);
        });
    }

    match outcome.result {
        Ok(response) => Ok(response),
        Err(e) => {
            // Credit back on failure so the user doesn't lose funds.
            state::with_state_mut(|s| {
                s.deposit(caller, order::TokenId::from(token_id), amount);
            });
            Err(e)
        }
    }
}

pub fn get_balance(token_id: dex_types::TokenId, runtime: &impl Runtime) -> dex_types::Balance {
    // TODO(DEFI-2741): Return an error if the token is not supported by the DEX.
    let caller = runtime.msg_caller();
    state::with_state(|s| {
        s.get_balance(&caller, &order::TokenId::from(token_id))
            .into()
    })
}

pub fn add_trading_pair(
    request: AddTradingPairRequest,
    runtime: &impl Runtime,
) -> Result<(), AddTradingPairError> {
    if !runtime.is_controller(&runtime.msg_caller()) {
        return Err(AddTradingPairError::NotController);
    }
    if request.base.id == request.quote.id {
        return Err(AddTradingPairError::BaseEqualsQuote);
    }
    let tick_size = order::TickSize::new(
        NonZeroU64::new(request.tick_size).ok_or(AddTradingPairError::InvalidTickSize)?,
    );
    let lot_size = order::LotSize::new(
        NonZeroU64::new(request.lot_size).ok_or(AddTradingPairError::InvalidLotSize)?,
    );
    state::with_state_mut(|s| -> Result<(), AddTradingPairError> {
        let pair = order::TradingPair {
            base: order::TokenId::from(request.base.id),
            quote: order::TokenId::from(request.quote.id),
        };
        if s.has_trading_pair(&pair) {
            return Err(AddTradingPairError::TradingPairAlreadyExists);
        }
        let base_metadata = order::TokenMetadata::from(request.base.metadata);
        let quote_metadata = order::TokenMetadata::from(request.quote.metadata);
        s.check_token_metadata_consistency(pair.base, &base_metadata)?;
        s.check_token_metadata_consistency(pair.quote, &quote_metadata)?;
        let book_id = s.next_book_id();
        let event = state::event::AddTradingPairEvent {
            book_id,
            base: pair.base,
            quote: pair.quote,
            tick_size,
            lot_size,
            base_metadata,
            quote_metadata,
        };
        state::audit::process_event(s, state::event::EventType::AddTradingPair(event));
        Ok(())
    })
}
