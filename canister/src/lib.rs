use candid::Nat;
use dex_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, CancelLimitOrderError,
    DEFAULT_DEPTH_LIMIT, DepositError, DepositRequest, DepositResponse, FilterToken,
    GetBalancesError, GetBalancesRequestError, GetOrderBookDepthError, GetOrderBookDepthRequest,
    GetOrderBookTickerError, LimitOrderRequest, MAX_DEPTH_LIMIT, MAX_FILTER_LEN, OrderBookDepth,
    OrderBookTicker, OrderId, OrderRecord, OrderStatus, PriceLevel, Token, TradingPair,
    TradingPairInfo, UserTokenBalance, WithdrawError, WithdrawRequest, WithdrawResponse,
};
use std::{num::NonZeroU64, time::Duration};

pub use execute::EXECUTOR;
pub use runtime::{IC_RUNTIME, Runtime};

/// Open a pair of canbench scopes: an aggregate scope and a specific scope.
/// Both are no-ops when the `canbench-rs` feature is not enabled.
macro_rules! bench_scopes {
    ($aggregate:expr, $specific:expr) => {
        #[cfg(feature = "canbench-rs")]
        let _aggregate = canbench_rs::bench_scope($aggregate);
        #[cfg(feature = "canbench-rs")]
        let _specific = canbench_rs::bench_scope($specific);
    };
}

pub mod balance;
pub mod cbor;
pub mod dashboard;
pub mod execute;
pub mod guard;
pub mod lifecycle;
pub mod metrics;
pub mod order;
pub mod runtime;
pub mod state;
pub mod storage;

#[cfg(feature = "canbench-rs")]
mod benchmarks;
mod ledger;
#[cfg(any(test, feature = "canbench-rs"))]
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
    let pending = order::PendingOrder::try_from(request)
        .map_err(|_| AddLimitOrderError::AmountExceedsMaximum)?;
    let (order_id, order) = state::with_state(|s| s.validate_limit_order(caller, pair, pending))?;

    state::with_state_mut(|s| {
        let event = state::event::AddLimitOrderEvent {
            user: caller,
            order_id,
            side: order.side(),
            price: order.price(),
            quantity: *order.remaining_quantity(),
        };
        state::audit::process_event(s, state::event::EventType::AddLimitOrder(event), runtime);
    });
    Ok(order_id.to_string())
}

pub fn cancel_limit_order(
    order_id: OrderId,
    runtime: &impl Runtime,
) -> Result<OrderRecord, CancelLimitOrderError> {
    state::with_state(|s| s.assert_caller_is_allowed(runtime));
    let caller = runtime.msg_caller();
    let id = order_id
        .parse::<order::OrderId>()
        .map_err(|_| CancelLimitOrderError::OrderNotFound)?;
    let record = state::with_state_mut(|s| s.cancel_limit_order(&caller, id, runtime))?;
    Ok(record.into())
}

pub fn process_pending_orders(runtime: &impl Runtime) -> execute::ExecutionStatus {
    let _guard = match guard::TimerGuard::new(Task::ProcessPendingOrders) {
        Some(guard) => guard,
        None => return execute::ExecutionStatus::AlreadyRunning,
    };

    state::with_state_mut(|s| EXECUTOR.run_once(s, runtime))
}

/// Run one chunk of matching/settling and, if more work remains, schedule a
/// zero-delay timer to continue. Intended for IC entry points (the periodic
/// matching timer and the post-`add_limit_order` kickoff) — tests should call
/// [`process_pending_orders`] directly, which is synchronous and timer-free.
pub fn drive_matching() {
    match process_pending_orders(&IC_RUNTIME) {
        execute::ExecutionStatus::MoreWork => {
            // TODO DEFI-2823: coalesce zero-delay matching timers so a
            // burst of `add_limit_order` kickoffs plus this self-reschedule
            // chain doesn't queue O(N) redundant timers per burst.
            ic_cdk_timers::set_timer(Duration::ZERO, async {
                drive_matching();
            });
        }
        // Complete: nothing left to do. AlreadyRunning: the holder will
        // reschedule itself if its run left work unfinished, so we don't
        // pile on another timer.
        execute::ExecutionStatus::Complete | execute::ExecutionStatus::AlreadyRunning => {}
    }
}

pub fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    match order_id.parse::<order::OrderId>() {
        Ok(id) => state::with_state(|s| s.get_order_status(id))
            .map(Into::into)
            .unwrap_or(OrderStatus::NotFound),
        Err(e) => panic!("ERROR: invalid order id: {}", e),
    }
}

pub fn get_order_book_ticker(
    pair: TradingPair,
) -> Result<OrderBookTicker, GetOrderBookTickerError> {
    let internal_pair = order::TradingPair::from(pair);
    state::with_state(|s| {
        let book = s
            .get_order_book(&internal_pair)
            .ok_or(GetOrderBookTickerError::UnknownTradingPair)?;
        Ok(OrderBookTicker {
            bid: book.bid_levels(1).next().map(to_price_level),
            ask: book.ask_levels(1).next().map(to_price_level),
        })
    })
}

pub fn get_order_book_depth(
    request: GetOrderBookDepthRequest,
) -> Result<OrderBookDepth, GetOrderBookDepthError> {
    let limit = match request.limit {
        None => DEFAULT_DEPTH_LIMIT,
        Some(n) if n <= MAX_DEPTH_LIMIT => n,
        Some(n) => {
            return Err(GetOrderBookDepthError::LimitTooLarge {
                requested: n,
                max: MAX_DEPTH_LIMIT,
            });
        }
    };
    let internal_pair = order::TradingPair::from(request.trading_pair);
    state::with_state(|s| {
        let book = s
            .get_order_book(&internal_pair)
            .ok_or(GetOrderBookDepthError::UnknownTradingPair)?;
        let limit = limit as usize;
        Ok(OrderBookDepth {
            bids: book.bid_levels(limit).map(to_price_level).collect(),
            asks: book.ask_levels(limit).map(to_price_level).collect(),
        })
    })
}

fn to_price_level((price, quantity): (order::Price, order::Quantity)) -> PriceLevel {
    PriceLevel {
        price: price.get(),
        quantity: quantity.into(),
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
    let amount = order::Quantity::try_from(request.amount.clone())
        .map_err(|_| DepositError::AmountExceedsMaximum)?;
    let caller = runtime.msg_caller();

    let _guard =
        guard::UserOpGuard::new(caller, internal_token).ok_or(DepositError::OperationInProgress)?;

    let existing = state::with_state(|s| s.get_balance(&caller, &internal_token));
    if existing
        .free()
        .checked_add(*existing.reserved())
        .and_then(|held| held.checked_add(amount))
        .is_none()
    {
        return Err(DepositError::AmountExceedsMaximum);
    }

    let deposit_response = ledger::deposit(request, runtime).await?;
    let event = state::event::DepositEvent {
        user: caller,
        token: order::TokenId::from(token_id),
        amount,
    };
    state::with_state_mut(|s| {
        state::audit::process_event(s, state::event::EventType::Deposit(event), runtime)
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
    let amount = order::Quantity::try_from(request.amount.clone())
        .map_err(|_| WithdrawError::AmountExceedsMaximum)?;

    let _guard = guard::UserOpGuard::new(caller, internal_token)
        .ok_or(WithdrawError::OperationInProgress)?;

    // Debit the full amount from the user's free balance.
    state::with_state_mut(|s| s.withdraw(caller, internal_token, amount)).map_err(|e| {
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
        Ok(response) => {
            // The balance debit happened synchronously before the async
            // ledger call (for concurrency safety), so the event is appended
            // record-only — replay re-applies the debit through
            // `apply_state_transition`.
            let block_index = u64::try_from(&response.block_index.0)
                .expect("BUG: ledger block_index exceeds u64::MAX");
            let event = state::event::WithdrawEvent {
                block_index,
                user: caller,
                token: order::TokenId::from(token_id),
                amount,
            };
            state::audit::record_event(state::event::EventType::Withdraw(event), runtime);
            Ok(response)
        }
        Err(e) => {
            // Credit back on failure so the user doesn't lose funds. No event
            // is emitted: replay then sees no WithdrawEvent for the failed
            // call, mirroring the net-zero state mutation on the primary path.
            state::with_state_mut(|s| {
                s.deposit(
                    caller,
                    order::TokenId::from(token_id),
                    amount,
                    state::StableMemoryOptions::Write,
                );
            });
            Err(e)
        }
    }
}

pub fn get_balances(
    filter: Option<Vec<FilterToken>>,
    caller: candid::Principal,
) -> Result<Vec<Result<UserTokenBalance, GetBalancesError>>, GetBalancesRequestError> {
    if let Some(ref f) = filter
        && (f.len() as u32) > MAX_FILTER_LEN
    {
        return Err(GetBalancesRequestError::FilterTooLarge {
            len: f.len() as u32,
            max: MAX_FILTER_LEN,
        });
    }
    Ok(state::with_state(|s| {
        s.get_balances(&caller, filter.as_deref())
    }))
}

pub fn list_supported_tokens() -> Vec<Token> {
    state::with_state(|s| {
        s.tokens()
            .iter()
            .map(|(id, meta)| Token {
                id: (*id).into(),
                metadata: meta.clone().into(),
            })
            .collect()
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
    let maker = order::BasisPoint::new(request.maker_fee_bps)
        .map_err(|_| AddTradingPairError::InvalidBasisPoint(request.maker_fee_bps))?;
    let taker = order::BasisPoint::new(request.taker_fee_bps)
        .map_err(|_| AddTradingPairError::InvalidBasisPoint(request.taker_fee_bps))?;
    let fee_rates = order::FeeRates { maker, taker };
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
            fee_rates,
        };
        state::audit::process_event(s, state::event::EventType::AddTradingPair(event), runtime);
        Ok(())
    })
}

pub fn fee_balance(token: dex_types::TokenId) -> Nat {
    state::with_state(|s| {
        s.fee_balance(&order::TokenId::from(token))
            .map(Into::into)
            .unwrap_or(Nat::from(0u64))
    })
}
