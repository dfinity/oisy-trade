use oisy_trade_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, CancelLimitOrderError,
    DEFAULT_DEPTH_LIMIT, DepositError, DepositRequest, DepositResponse, FilterToken,
    GetBalancesError, GetMyOrdersArgs, GetOrderBookDepthError, GetOrderBookDepthRequest,
    GetOrderBookTickerError, LimitOrderRequest, MAX_DEPTH_LIMIT, MAX_FILTER_LEN,
    MAX_ORDERS_PER_RESPONSE, OrderBookDepth, OrderBookTicker, OrderId, OrderRecord, PriceLevel,
    Token, TradingPair, TradingPairInfo, UnauthorizedError, UserOrder, UserTokenBalance,
    WithdrawError, WithdrawRequest, WithdrawResponse,
};
use std::{
    num::{NonZeroU64, NonZeroU128},
    time::Duration,
};

pub use execute::EXECUTOR;
pub use runtime::{IC_RUNTIME, Runtime, Timestamp};

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
pub mod user;

#[cfg(feature = "canbench-rs")]
mod benchmarks;
mod ledger;
#[cfg(any(test, feature = "canbench-rs"))]
pub mod test_fixtures;
#[cfg(test)]
mod tests;

pub const MATCHING_INTERVAL: Duration = Duration::from_mins(1);

pub fn add_limit_order(
    request: LimitOrderRequest,
    runtime: &impl Runtime,
) -> Result<OrderId, AddLimitOrderError> {
    state::with_state(|s| s.assert_caller_is_allowed(runtime));
    let caller = runtime.msg_caller();
    let pair = order::TradingPair::from(request.pair);
    let pending = order::PendingOrder::try_from(request).map_err(|_| {
        AddLimitOrderError::request(
            oisy_trade_types::AddLimitOrderRequestError::AmountExceedsMaximum,
        )
    })?;
    let (order_id, order) = state::with_state(|s| s.validate_limit_order(caller, pair, pending))?;

    state::with_state_mut(|s| {
        let permit = s
            .permissions()
            .permit_trading(caller, order_id.book_id())
            .map_err(|e| AddLimitOrderError::from(state::AddLimitOrderError::from(e)))?;
        let event = state::event::AddLimitOrderEvent {
            user: caller,
            order_id,
            side: order.side(),
            price: order.price(),
            quantity: *order.remaining_quantity(),
            time_in_force: order.time_in_force(),
        };
        state::audit::process_event(
            s,
            state::event::EventType::AddLimitOrder(event),
            permit.into(),
            runtime,
        );
        Ok::<(), AddLimitOrderError>(())
    })?;
    Ok(order_id.to_string())
}

pub fn cancel_limit_order(
    order_id: OrderId,
    runtime: &impl Runtime,
) -> Result<OrderRecord, CancelLimitOrderError> {
    state::with_state(|s| s.assert_caller_is_allowed(runtime));
    let caller = runtime.msg_caller();
    let id = order_id.parse::<order::OrderId>().map_err(|_| {
        CancelLimitOrderError::request(
            oisy_trade_types::CancelLimitOrderRequestError::InvalidOrderId,
        )
    })?;
    let record = state::with_state_mut(|s| s.cancel_limit_order(&caller, id, runtime))?;
    Ok(record.into())
}

pub fn process_pending_orders(runtime: &impl Runtime) -> execute::ExecutionStatus {
    state::with_state_mut(|s| EXECUTOR.run_once(s, runtime))
}

/// Schedule a zero-delay timer to drive matching, unless one is already
/// pending. Collapses a burst of kickoffs — e.g. back-to-back `add_limit_order`
/// calls plus the [`drive_matching`] self-reschedule chain — into a single
/// drive loop instead of one timer per call.
pub fn schedule_matching_timer() {
    let should_schedule = state::with_state_mut(|s| s.try_mark_matching_timer_scheduled());
    if should_schedule {
        ic_cdk_timers::set_timer(Duration::ZERO, async {
            drive_matching();
        });
    }
}

/// Run one chunk of matching/settling and, if more work remains, schedule a
/// zero-delay timer to continue. Intended for IC entry points (the periodic
/// matching timer and the post-`add_limit_order` kickoff) — tests should call
/// [`process_pending_orders`] directly, which is synchronous and timer-free.
pub fn drive_matching() {
    // This timer has now fired; clear the flag so the `MoreWork` path below
    // re-arms exactly one continuation and a fresh kickoff can schedule again.
    state::with_state_mut(|s| s.clear_matching_timer_scheduled());
    match process_pending_orders(&IC_RUNTIME) {
        execute::ExecutionStatus::MoreWork => schedule_matching_timer(),
        execute::ExecutionStatus::Complete => {}
    }
}

pub fn get_order_book_ticker(
    pair: TradingPair,
) -> Result<OrderBookTicker, GetOrderBookTickerError> {
    let internal_pair = order::TradingPair::from(pair);
    state::with_state(|s| {
        let book = s.get_order_book(&internal_pair).ok_or_else(|| {
            GetOrderBookTickerError::request(
                oisy_trade_types::GetOrderBookTickerRequestError::UnknownTradingPair,
            )
        })?;
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
            return Err(GetOrderBookDepthError::request(
                oisy_trade_types::GetOrderBookDepthRequestError::LimitTooLarge {
                    requested: n,
                    max: MAX_DEPTH_LIMIT,
                },
            ));
        }
    };
    let internal_pair = order::TradingPair::from(request.trading_pair);
    state::with_state(|s| {
        let book = s.get_order_book(&internal_pair).ok_or_else(|| {
            GetOrderBookDepthError::request(
                oisy_trade_types::GetOrderBookDepthRequestError::UnknownTradingPair,
            )
        })?;
        let limit = limit as usize;
        Ok(OrderBookDepth {
            bids: book.bid_levels(limit).map(to_price_level).collect(),
            asks: book.ask_levels(limit).map(to_price_level).collect(),
        })
    })
}

fn to_price_level((price, quantity): (order::Price, order::Quantity)) -> PriceLevel {
    PriceLevel {
        price: candid::Nat::from(price),
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
                let halted = s.permissions().is_halted(book_id);
                TradingPairInfo {
                    base: oisy_trade_types::Token {
                        id: oisy_trade_types::TokenId::from(pair.base),
                        metadata: base_meta.clone().into(),
                    },
                    quote: oisy_trade_types::Token {
                        id: oisy_trade_types::TokenId::from(pair.quote),
                        metadata: quote_meta.clone().into(),
                    },
                    status: if halted {
                        oisy_trade_types::TradingStatus::Halted
                    } else {
                        oisy_trade_types::TradingStatus::Trading
                    },
                    tick_size: candid::Nat::from(book.tick_size()),
                    lot_size: candid::Nat::from(book.lot_size()),
                    maker_fee_bps: book.fee_rates().maker.get(),
                    taker_fee_bps: book.fee_rates().taker.get(),
                    min_notional: book.min_notional().into(),
                    max_notional: book.max_notional().map(Into::into),
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
        return Err(DepositError::request(
            oisy_trade_types::DepositRequestError::UnsupportedToken { token_id },
        ));
    }
    let amount = order::Quantity::try_from(request.amount.clone()).map_err(|_| {
        DepositError::request(oisy_trade_types::DepositRequestError::AmountExceedsMaximum)
    })?;
    let caller = runtime.msg_caller();

    let _guard = guard::UserOpGuard::new(caller, internal_token).ok_or_else(|| {
        DepositError::temporary(oisy_trade_types::DepositTemporaryError::OperationInProgress)
    })?;

    let existing = state::with_state(|s| s.get_balance(&caller, &internal_token));
    if existing
        .free()
        .checked_add(*existing.reserved())
        .and_then(|held| held.checked_add(amount))
        .is_none()
    {
        return Err(DepositError::request(
            oisy_trade_types::DepositRequestError::AmountExceedsMaximum,
        ));
    }

    let pre = state::with_state(|s| s.permissions().permit_deposit(caller));

    let deposit_response = ledger::deposit(request, runtime).await?;
    let event = state::event::DepositEvent {
        user: caller,
        token: order::TokenId::from(token_id),
        amount,
    };
    let post = pre.reconcile();
    state::with_state_mut(|s| {
        state::audit::process_event(
            s,
            state::event::EventType::Deposit(event),
            post.into(),
            runtime,
        )
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
        return Err(WithdrawError::request(
            oisy_trade_types::WithdrawRequestError::UnsupportedToken { token_id },
        ));
    }
    let cached_fee = state::with_state(|s| s.get_cached_ledger_fee(&internal_token));

    if request.amount == 0u64 {
        return Err(WithdrawError::request(
            oisy_trade_types::WithdrawRequestError::AmountTooSmall {
                min_amount: cached_fee + 1u64,
            },
        ));
    }

    let caller = runtime.msg_caller();
    let amount = order::Quantity::try_from(request.amount.clone()).map_err(|_| {
        WithdrawError::request(oisy_trade_types::WithdrawRequestError::AmountExceedsMaximum)
    })?;

    let _guard = guard::UserOpGuard::new(caller, internal_token).ok_or_else(|| {
        WithdrawError::temporary(oisy_trade_types::WithdrawTemporaryError::OperationInProgress)
    })?;

    // Debit the full amount from the user's free balance.
    state::with_state_mut(|s| s.withdraw(caller, internal_token, amount)).map_err(|e| {
        WithdrawError::request(
            oisy_trade_types::WithdrawRequestError::InsufficientBalance {
                available: e.available.into(),
            },
        )
    })?;

    let pre = state::with_state(|s| s.permissions().permit_withdraw(caller));

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
            let post = pre.reconcile();
            state::audit::record_event(
                state::event::EventType::Withdraw(event),
                post.into(),
                runtime,
            );
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
) -> Result<Vec<UserTokenBalance>, GetBalancesError> {
    validate_filter_len(filter.as_deref())?;
    state::with_state(|s| s.get_balances(&caller, filter.as_deref()))
}

pub fn get_fee_balances(
    filter: Option<Vec<FilterToken>>,
) -> Result<Vec<UserTokenBalance>, GetBalancesError> {
    validate_filter_len(filter.as_deref())?;
    state::with_state(|s| s.get_fee_balances(filter.as_deref()))
}

/// Why a [`get_my_orders`] call could not be served.
///
/// Typically those errors indicate a client bug.
#[derive(Debug, PartialEq, Eq)]
pub enum GetMyOrdersError {
    /// An order id in the filter (`ById` target or `ByPage.after` cursor) was
    /// not a well-formed order id.
    InvalidOrderId(order::OrderIdParseError),
    /// A well-formed order id (`ById` target or `ByPage.after` cursor) is
    /// unknown or not owned by the caller.
    OrderNotFound,
}

pub fn get_my_orders(
    args: Option<GetMyOrdersArgs>,
    caller: candid::Principal,
) -> Result<Vec<UserOrder>, GetMyOrdersError> {
    let filter = args.unwrap_or_default().filter;
    let results = match filter {
        oisy_trade_types::GetMyOrdersFilter::ById(id) => {
            let id = id
                .parse::<order::OrderId>()
                .map_err(GetMyOrdersError::InvalidOrderId)?;
            let order = state::with_state(|s| s.get_user_order(&caller, id))
                .ok_or(GetMyOrdersError::OrderNotFound)?;
            vec![order]
        }
        oisy_trade_types::GetMyOrdersFilter::ByPage(page) => {
            let after = page
                .after
                .map(|id| id.parse::<order::OrderId>())
                .transpose()
                .map_err(GetMyOrdersError::InvalidOrderId)?;
            let length = page.length.min(MAX_ORDERS_PER_RESPONSE) as usize;
            state::with_state(|s| s.get_user_orders(&caller, after, length))
                .map_err(|_| GetMyOrdersError::OrderNotFound)?
        }
    };
    Ok(results
        .into_iter()
        .map(|(id, pair, record)| UserOrder {
            id: id.into(),
            pair: pair.into(),
            order: record.into(),
        })
        .collect())
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
    let tick_size_u128 =
        u128::try_from(&request.tick_size.0).map_err(|_| AddTradingPairError::InvalidTickSize)?;
    let tick_size = order::TickSize::new(
        NonZeroU128::new(tick_size_u128).ok_or(AddTradingPairError::InvalidTickSize)?,
    );
    let lot_size_u64 =
        u64::try_from(&request.lot_size.0).map_err(|_| AddTradingPairError::InvalidLotSize)?;
    let lot_size = order::LotSize::new(
        NonZeroU64::new(lot_size_u64).ok_or(AddTradingPairError::InvalidLotSize)?,
    );
    let maker = order::BasisPoint::new(request.maker_fee_bps)
        .map_err(|_| AddTradingPairError::InvalidBasisPoint(request.maker_fee_bps))?;
    let taker = order::BasisPoint::new(request.taker_fee_bps)
        .map_err(|_| AddTradingPairError::InvalidBasisPoint(request.taker_fee_bps))?;
    let fee_rates = order::FeeRates { maker, taker };
    let invalid_notional = || AddTradingPairError::InvalidNotional {
        min_notional: request.min_notional.clone(),
        max_notional: request.max_notional.clone(),
    };
    let min_notional =
        order::Quantity::try_from(request.min_notional.clone()).map_err(|_| invalid_notional())?;
    if min_notional.is_zero() {
        return Err(invalid_notional());
    }
    let max_notional = match &request.max_notional {
        None => None,
        Some(max) => {
            let max = order::Quantity::try_from(max.clone()).map_err(|_| invalid_notional())?;
            if max < min_notional {
                return Err(invalid_notional());
            }
            Some(max)
        }
    };
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
        // Enforce the settlement-exactness invariant ahead of the upcoming
        // change to settle a fill as `price × quantity / 10^base_decimals`:
        // prices are multiples of the tick and quantities multiples of the lot,
        // so requiring `tick_size × lot_size` to be a multiple of
        // `10^base_decimals` guarantees that division will be exact (no
        // rounding).
        let base_decimals = base_metadata.decimals;
        let base_scale = 10u64.checked_pow(base_decimals as u32).ok_or(
            AddTradingPairError::BaseDecimalsTooLarge {
                decimals: base_decimals,
            },
        )?;
        // `tick_size` is u128 and `lot_size` is u64; their product is computed
        // as a u256 to avoid overflow before checking divisibility.
        let tick_lot = order::Quantity::from_u128(tick_size.get())
            .checked_mul_u64(lot_size.get())
            .expect("BUG: u128 × u64 always fits u256");
        let (_, remainder) = tick_lot
            .checked_div_rem_u64(base_scale)
            .expect("base_scale is a nonzero power of ten");
        if remainder != 0 {
            return Err(AddTradingPairError::IndivisibleTickLotForBaseDecimals {
                tick_size: candid::Nat::from(tick_size),
                lot_size: candid::Nat::from(lot_size),
                base_decimals,
            });
        }
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
            min_notional,
            max_notional,
        };
        let permit = s.permissions().permit_add_trading_pair();
        state::audit::process_event(
            s,
            state::event::EventType::AddTradingPair(event),
            permit.into(),
            runtime,
        );
        Ok(())
    })
}

/// Maximum number of trading pairs a single `halt_trading` / `resume_trading`
/// call may carry, bounding the size of the `SetHalt` audit event it records.
pub const MAX_HALT_BOOKS: usize = 100;

pub fn halt_trading(
    pairs: Option<Vec<TradingPair>>,
    runtime: &impl Runtime,
) -> Result<(), UnauthorizedError> {
    set_halt(pairs, true, runtime)
}

pub fn resume_trading(
    pairs: Option<Vec<TradingPair>>,
    runtime: &impl Runtime,
) -> Result<(), UnauthorizedError> {
    set_halt(pairs, false, runtime)
}

fn set_halt(
    pairs: Option<Vec<TradingPair>>,
    halted: bool,
    runtime: &impl Runtime,
) -> Result<(), UnauthorizedError> {
    if !runtime.is_controller(&runtime.msg_caller()) {
        return Err(UnauthorizedError::NotController);
    }
    state::with_state_mut(|s| {
        let book_ids = pairs.map(|pairs| {
            if pairs.len() > MAX_HALT_BOOKS {
                ic_cdk::trap(format!(
                    "too many trading pairs: {} (max {MAX_HALT_BOOKS})",
                    pairs.len()
                ));
            }
            pairs
                .into_iter()
                .map(|pair| {
                    let internal_pair = order::TradingPair::from(pair);
                    *s.trading_pairs()
                        .get_book_id(&internal_pair)
                        .unwrap_or_else(|| {
                            ic_cdk::trap(format!("unknown trading pair: {internal_pair:?}"))
                        })
                })
                .collect()
        });
        let permit = s.permissions().permit_admin();
        let event = state::event::SetHaltEvent { book_ids, halted };
        state::audit::process_event(
            s,
            state::event::EventType::SetHalt(event),
            permit.into(),
            runtime,
        );
    });
    Ok(())
}

fn validate_filter_len(filter: Option<&[FilterToken]>) -> Result<(), GetBalancesError> {
    if let Some(f) = filter
        && (f.len() as u32) > MAX_FILTER_LEN
    {
        return Err(GetBalancesError::request(
            oisy_trade_types::GetBalancesRequestError::FilterTooLarge {
                len: f.len() as u32,
                max: MAX_FILTER_LEN,
            },
        ));
    }
    Ok(())
}
