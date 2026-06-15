use ic_http_types::{HttpRequest, HttpResponse};
use oisy_trade_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, CancelLimitOrderError,
    DepositError, DepositRequest, DepositResponse, FilterToken, GetBalancesError,
    GetBalancesRequestError, GetMyOrdersArgs, GetOrderBookDepthError, GetOrderBookDepthRequest,
    GetOrderBookTickerError, LedgerTransferError, LedgerTransferFromError, LimitOrderRequest,
    OrderBookDepth, OrderBookTicker, OrderId, OrderRecord, Token, TradingPair, TradingPairInfo,
    UnauthorizedError, UserOrder, UserTokenBalance, WithdrawError, WithdrawRequest,
    WithdrawResponse,
};
use oisy_trade_types_internal::OisyTradeArg;
use oisy_trade_types_internal::log::Priority;

#[ic_cdk::update]
fn add_limit_order(request: LimitOrderRequest) -> Result<OrderId, AddLimitOrderError> {
    let order_id =
        oisy_trade_canister::add_limit_order(request.clone(), &oisy_trade_canister::IC_RUNTIME)?;
    canlog::log!(
        Priority::Info,
        "[add_limit_order]: created order_id={} for request {:?}",
        order_id,
        request
    );
    // Trigger matching immediately, no need to wait for the periodic timer.
    // TODO DEFI-2823: coalesce — a burst of order placements currently
    // queues one zero-delay timer per call.
    ic_cdk_timers::set_timer(std::time::Duration::ZERO, async {
        oisy_trade_canister::drive_matching();
    });
    Ok(order_id)
}

#[ic_cdk::update]
fn cancel_limit_order(order_id: OrderId) -> Result<OrderRecord, CancelLimitOrderError> {
    let result =
        oisy_trade_canister::cancel_limit_order(order_id.clone(), &oisy_trade_canister::IC_RUNTIME);
    match &result {
        Ok(record) => canlog::log!(
            Priority::Info,
            "[cancel_limit_order]: canceled order_id={order_id}: {record:?}"
        ),
        Err(_err) => {
            // do not log errors due to user actions
        }
    }
    result
}

#[ic_cdk::query]
fn get_trading_pairs() -> Vec<TradingPairInfo> {
    oisy_trade_canister::get_trading_pairs()
}

#[ic_cdk::query]
fn get_order_book_ticker(pair: TradingPair) -> Result<OrderBookTicker, GetOrderBookTickerError> {
    oisy_trade_canister::get_order_book_ticker(pair)
}

#[ic_cdk::query]
fn get_order_book_depth(
    request: GetOrderBookDepthRequest,
) -> Result<OrderBookDepth, GetOrderBookDepthError> {
    oisy_trade_canister::get_order_book_depth(request)
}

#[ic_cdk::update]
async fn deposit(request: DepositRequest) -> Result<DepositResponse, DepositError> {
    let deposit_dbg = format!("{request:?}");
    let result = oisy_trade_canister::deposit(request, &oisy_trade_canister::IC_RUNTIME).await;
    match &result {
        Ok(response) => canlog::log!(
            Priority::Info,
            "[deposit]: successful deposit for request {deposit_dbg}, block_index={}",
            response.block_index
        ),
        Err(err) => match err {
            DepositError::CallFailed { .. }
            | DepositError::LedgerError(LedgerTransferFromError::TemporarilyUnavailable)
            | DepositError::LedgerError(LedgerTransferFromError::InternalError(_)) => {
                canlog::log!(
                    Priority::Debug,
                    "[deposit]: deposit for request {deposit_dbg} failed, error={:?}",
                    err
                )
            }
            DepositError::AmountExceedsMaximum
            | DepositError::UnsupportedToken { .. }
            | DepositError::OperationInProgress
            | DepositError::LedgerError(LedgerTransferFromError::InsufficientFunds { .. })
            | DepositError::LedgerError(LedgerTransferFromError::InsufficientAllowance {
                ..
            }) => {
                // do not log errors due to user actions
            }
        },
    }
    result
}

#[ic_cdk::update]
async fn withdraw(request: WithdrawRequest) -> Result<WithdrawResponse, WithdrawError> {
    let withdraw_dbg = format!("{request:?}");
    let result = oisy_trade_canister::withdraw(request, &oisy_trade_canister::IC_RUNTIME).await;
    match &result {
        Ok(response) => canlog::log!(
            Priority::Info,
            "[withdraw]: successful withdrawal for request {withdraw_dbg}, block_index={}",
            response.block_index
        ),
        Err(err) => match err {
            WithdrawError::CallFailed { .. }
            | WithdrawError::LedgerError(LedgerTransferError::TemporarilyUnavailable)
            | WithdrawError::LedgerError(LedgerTransferError::InternalError(_))
            | WithdrawError::LedgerError(LedgerTransferError::InsufficientFunds { .. }) => {
                canlog::log!(
                    Priority::Debug,
                    "[withdraw]: withdrawal for request {withdraw_dbg} failed, error={:?}",
                    err
                )
            }
            WithdrawError::AmountExceedsMaximum
            | WithdrawError::UnsupportedToken { .. }
            | WithdrawError::InsufficientBalance { .. }
            | WithdrawError::AmountTooSmall { .. }
            | WithdrawError::OperationInProgress => {
                // do not log errors due to user actions
            }
        },
    }
    result
}

#[ic_cdk::query]
fn get_balances(
    filter: Option<Vec<FilterToken>>,
) -> Result<Vec<Result<UserTokenBalance, GetBalancesError>>, GetBalancesRequestError> {
    use oisy_trade_canister::Runtime;
    oisy_trade_canister::get_balances(filter, oisy_trade_canister::IC_RUNTIME.msg_caller())
}

#[ic_cdk::query]
fn get_fee_balances(
    filter: Option<Vec<FilterToken>>,
) -> Result<Vec<Result<UserTokenBalance, GetBalancesError>>, GetBalancesRequestError> {
    oisy_trade_canister::get_fee_balances(filter)
}

#[ic_cdk::query]
fn get_my_orders(args: Option<GetMyOrdersArgs>) -> Vec<UserOrder> {
    use oisy_trade_canister::Runtime;
    match oisy_trade_canister::get_my_orders(args, oisy_trade_canister::IC_RUNTIME.msg_caller()) {
        Ok(orders) => orders,
        Err(oisy_trade_canister::GetMyOrdersError::InvalidOrderId(e)) => {
            panic!("ERROR: invalid order id: {e}")
        }
    }
}

#[ic_cdk::query]
fn list_supported_tokens() -> Vec<Token> {
    oisy_trade_canister::list_supported_tokens()
}

#[ic_cdk::update]
fn add_trading_pair(request: AddTradingPairRequest) -> Result<(), AddTradingPairError> {
    oisy_trade_canister::add_trading_pair(request, &oisy_trade_canister::IC_RUNTIME)
}

#[ic_cdk::update]
fn halt_trading() -> Result<(), UnauthorizedError> {
    oisy_trade_canister::halt_trading(&oisy_trade_canister::IC_RUNTIME)
}

#[ic_cdk::update]
fn resume_trading() -> Result<(), UnauthorizedError> {
    oisy_trade_canister::resume_trading(&oisy_trade_canister::IC_RUNTIME)?;
    // Re-arm matching immediately so orders that piled up while halted match now,
    // without waiting for the periodic timer. Mirrors the add_limit_order kickoff.
    ic_cdk_timers::set_timer(std::time::Duration::ZERO, async {
        oisy_trade_canister::drive_matching();
    });
    Ok(())
}

/// *WARNING*: This is a debug endpoint, backwards-compatibility is not guaranteed.
#[ic_cdk::query]
fn get_events(
    args: oisy_trade_types_internal::event::GetEventsArgs,
) -> oisy_trade_types_internal::event::GetEventsResult {
    use oisy_trade_canister::state::event::{Event, EventType};
    use oisy_trade_types_internal::event;

    const MAX_EVENTS_PER_RESPONSE: u64 = 2_000;

    fn map_pair_token(token: oisy_trade_canister::order::PairToken) -> event::PairToken {
        match token {
            oisy_trade_canister::order::PairToken::Base => event::PairToken::Base,
            oisy_trade_canister::order::PairToken::Quote => event::PairToken::Quote,
        }
    }

    fn map_balance_operation(
        op: oisy_trade_canister::state::event::BalanceOperation,
    ) -> event::BalanceOperation {
        match op {
            oisy_trade_canister::state::event::BalanceOperation::Transfer {
                from_order,
                to_order,
                token,
                amount,
                fee,
            } => event::BalanceOperation::Transfer {
                from_order: from_order.get(),
                to_order: to_order.get(),
                token: map_pair_token(token),
                amount: amount.into(),
                fee: fee.map(Into::into),
            },
            oisy_trade_canister::state::event::BalanceOperation::Unreserve {
                order,
                token,
                amount,
            } => event::BalanceOperation::Unreserve {
                order: order.get(),
                token: map_pair_token(token),
                amount: amount.into(),
            },
        }
    }

    fn map_event(event: Event) -> event::Event {
        event::Event {
            timestamp: event.timestamp.as_nanos(),
            payload: match event.payload {
                EventType::Init(args) => event::EventType::Init(args),
                EventType::Upgrade(args) => event::EventType::Upgrade(args),
                EventType::AddTradingPair(
                    oisy_trade_canister::state::event::AddTradingPairEvent {
                        book_id,
                        base,
                        quote,
                        tick_size,
                        lot_size,
                        base_metadata,
                        quote_metadata,
                        fee_rates,
                        min_notional,
                        max_notional,
                    },
                ) => event::EventType::AddTradingPair(event::AddTradingPairEvent {
                    book_id: book_id.get(),
                    base: oisy_trade_types::TokenId::from(base),
                    quote: oisy_trade_types::TokenId::from(quote),
                    tick_size: candid::Nat::from(tick_size),
                    lot_size: candid::Nat::from(lot_size),
                    base_metadata: oisy_trade_types::TokenMetadata::from(base_metadata),
                    quote_metadata: oisy_trade_types::TokenMetadata::from(quote_metadata),
                    maker_fee_bps: fee_rates.maker.get(),
                    taker_fee_bps: fee_rates.taker.get(),
                    min_notional: candid::Nat::from(min_notional),
                    max_notional: max_notional.map(candid::Nat::from),
                }),
                EventType::Deposit(oisy_trade_canister::state::event::DepositEvent {
                    user,
                    token,
                    amount,
                }) => event::EventType::Deposit(event::DepositEvent {
                    user,
                    token: oisy_trade_types::TokenId::from(token),
                    amount: amount.into(),
                }),
                EventType::Withdraw(oisy_trade_canister::state::event::WithdrawEvent {
                    block_index,
                    user,
                    token,
                    amount,
                }) => event::EventType::Withdraw(event::WithdrawEvent {
                    block_index,
                    user,
                    token: oisy_trade_types::TokenId::from(token),
                    amount: amount.into(),
                }),
                EventType::AddLimitOrder(
                    oisy_trade_canister::state::event::AddLimitOrderEvent {
                        user,
                        order_id,
                        side,
                        price,
                        quantity,
                    },
                ) => event::EventType::AddLimitOrder(event::AddLimitOrderEvent {
                    user,
                    order_id: event::OrderId {
                        book_id: order_id.book_id().get(),
                        seq: order_id.seq().get(),
                    },
                    side: oisy_trade_types::Side::from(side),
                    price: candid::Nat::from(price),
                    quantity: quantity.into(),
                }),
                EventType::CancelLimitOrder(
                    oisy_trade_canister::state::event::CancelLimitOrderEvent { order_id },
                ) => event::EventType::CancelLimitOrder(event::CancelLimitOrderEvent {
                    order_id: event::OrderId {
                        book_id: order_id.book_id().get(),
                        seq: order_id.seq().get(),
                    },
                }),
                EventType::Matching(oisy_trade_canister::state::event::MatchingEvent {
                    book_id,
                    orders,
                }) => event::EventType::Matching(event::MatchingEvent {
                    book_id: book_id.get(),
                    orders: orders.into_iter().map(|s| s.get()).collect(),
                }),
                EventType::Settling(oisy_trade_canister::state::event::SettlingEvent {
                    book_id,
                    balance_operations,
                }) => event::EventType::Settling(event::SettlingEvent {
                    book_id: book_id.get(),
                    balance_operations: balance_operations
                        .into_iter()
                        .map(map_balance_operation)
                        .collect(),
                }),
                EventType::SetGlobalHalt(halted) => event::EventType::SetGlobalHalt(halted),
            },
        }
    }

    let start = usize::try_from(args.start).expect("BUG: start index exceeds usize::MAX");
    let length = usize::try_from(args.length.min(MAX_EVENTS_PER_RESPONSE))
        .expect("BUG: length exceeds usize::MAX");
    let events = oisy_trade_canister::storage::with_event_iter(|it| {
        it.skip(start).take(length).map(map_event).collect()
    });
    event::GetEventsResult {
        events,
        total_event_count: oisy_trade_canister::storage::total_event_count(),
    }
}

#[ic_cdk::init]
fn init(arg: OisyTradeArg) {
    oisy_trade_canister::lifecycle::init(arg, &oisy_trade_canister::IC_RUNTIME);
}

#[ic_cdk::pre_upgrade]
fn pre_upgrade() {
    oisy_trade_canister::lifecycle::pre_upgrade(&oisy_trade_canister::IC_RUNTIME);
}

#[ic_cdk::post_upgrade]
fn post_upgrade(arg: Option<OisyTradeArg>) {
    oisy_trade_canister::lifecycle::post_upgrade(arg, &oisy_trade_canister::IC_RUNTIME);
}

#[ic_cdk::query(hidden = true)]
fn http_request(request: HttpRequest) -> HttpResponse {
    use canlog::{Log, Sort};
    use ic_http_types::HttpResponseBuilder;
    use std::str::FromStr;

    match request.path() {
        "/logs" => {
            let max_skip_timestamp = match request.raw_query_param("time") {
                Some(arg) => match u64::from_str(arg) {
                    Ok(value) => value,
                    Err(_) => {
                        return HttpResponseBuilder::bad_request()
                            .with_body_and_content_length("failed to parse the 'time' parameter")
                            .build();
                    }
                },
                None => 0,
            };

            let mut log: Log<Priority> = Default::default();

            match request.raw_query_param("priority").map(Priority::from_str) {
                Some(Ok(priority)) => match priority {
                    Priority::Info => log.push_logs(Priority::Info),
                    Priority::Debug => log.push_logs(Priority::Debug),
                },
                Some(Err(_)) | None => {
                    log.push_logs(Priority::Info);
                    log.push_logs(Priority::Debug);
                }
            }

            log.entries
                .retain(|entry| entry.timestamp >= max_skip_timestamp);

            fn ordering_from_query_params(sort: Option<&str>, max_skip_timestamp: u64) -> Sort {
                match sort.map(Sort::from_str) {
                    Some(Ok(order)) => order,
                    Some(Err(_)) | None => {
                        if max_skip_timestamp == 0 {
                            Sort::Ascending
                        } else {
                            Sort::Descending
                        }
                    }
                }
            }

            log.sort_logs(ordering_from_query_params(
                request.raw_query_param("sort"),
                max_skip_timestamp,
            ));

            const MAX_BODY_SIZE: usize = 2_000_000;
            HttpResponseBuilder::ok()
                .header("Content-Type", "application/json; charset=utf-8")
                .with_body_and_content_length(log.serialize_logs(MAX_BODY_SIZE))
                .build()
        }
        "/dashboard" => {
            use askama::Template;
            let canister_id = ic_cdk::api::canister_self();
            let total_events = oisy_trade_canister::storage::total_event_count();
            let dashboard = oisy_trade_canister::state::with_state(|s| {
                oisy_trade_canister::dashboard::DashboardTemplate::from_state(
                    s,
                    canister_id,
                    total_events,
                )
            });
            match dashboard.render() {
                Ok(body) => HttpResponseBuilder::ok()
                    .header("Content-Type", "text/html; charset=utf-8")
                    .with_body_and_content_length(body)
                    .build(),
                Err(e) => HttpResponseBuilder::server_error(format!("template error: {e}")).build(),
            }
        }
        "/metrics" => {
            use ic_metrics_encoder::MetricsEncoder;

            let mut writer = MetricsEncoder::new(vec![], ic_cdk::api::time() as i64 / 1_000_000);
            match oisy_trade_canister::metrics::encode_metrics(&mut writer) {
                Ok(()) => HttpResponseBuilder::ok()
                    .header("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
                    .with_body_and_content_length(writer.into_inner())
                    .build(),
                Err(err) => HttpResponseBuilder::server_error(format!("{err}")).build(),
            }
        }
        _ => HttpResponseBuilder::not_found().build(),
    }
}

fn main() {}

#[test]
fn check_candid_interface_compatibility() {
    use candid_parser::utils::{CandidSource, service_equal};

    candid::export_service!();

    let new_interface = __export_service();

    // check the public interface against the actual one
    let old_interface = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("oisy_trade.did");

    service_equal(
        CandidSource::Text(&new_interface),
        CandidSource::File(old_interface.as_path()),
    )
    .unwrap();
}
