use dex_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, Balance, DepositError,
    DepositRequest, DepositResponse, GetOrderBookDepthError, GetOrderBookDepthRequest,
    GetOrderBookTickerError, LedgerTransferError, LedgerTransferFromError, LimitOrderRequest,
    OrderBookDepth, OrderBookTicker, OrderId, OrderStatus, TokenId, TradingPair, TradingPairInfo,
    WithdrawError, WithdrawRequest, WithdrawResponse,
};
use dex_types_internal::DexArg;
use dex_types_internal::log::Priority;
use ic_http_types::{HttpRequest, HttpResponse};

#[ic_cdk::update]
fn add_limit_order(request: LimitOrderRequest) -> Result<OrderId, AddLimitOrderError> {
    let order_id = dex_canister::add_limit_order(request.clone(), &dex_canister::IC_RUNTIME)?;
    canlog::log!(
        Priority::Info,
        "[add_limit_order]: created order_id={} for request {:?}",
        order_id,
        request
    );
    // Trigger matching immediately, no need to wait for the periodic timer.
    ic_cdk_timers::set_timer(std::time::Duration::ZERO, async {
        dex_canister::process_pending_orders(&dex_canister::IC_RUNTIME);
    });
    Ok(order_id)
}

#[ic_cdk::query]
fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    dex_canister::get_order_status(order_id)
}

#[ic_cdk::query]
fn get_trading_pairs() -> Vec<TradingPairInfo> {
    dex_canister::get_trading_pairs()
}

#[ic_cdk::query]
fn get_order_book_ticker(pair: TradingPair) -> Result<OrderBookTicker, GetOrderBookTickerError> {
    dex_canister::get_order_book_ticker(pair)
}

#[ic_cdk::query]
fn get_order_book_depth(
    request: GetOrderBookDepthRequest,
) -> Result<OrderBookDepth, GetOrderBookDepthError> {
    dex_canister::get_order_book_depth(request)
}

#[ic_cdk::update]
async fn deposit(request: DepositRequest) -> Result<DepositResponse, DepositError> {
    let deposit_dbg = format!("{request:?}");
    let result = dex_canister::deposit(request, &dex_canister::IC_RUNTIME).await;
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
    let result = dex_canister::withdraw(request, &dex_canister::IC_RUNTIME).await;
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
            | WithdrawError::AmountTooSmall { .. } => {
                // do not log errors due to user actions
            }
        },
    }
    result
}

#[ic_cdk::query]
fn get_balance(token_id: TokenId) -> Balance {
    dex_canister::get_balance(token_id, &dex_canister::IC_RUNTIME)
}

#[ic_cdk::update]
fn add_trading_pair(request: AddTradingPairRequest) -> Result<(), AddTradingPairError> {
    dex_canister::add_trading_pair(request, &dex_canister::IC_RUNTIME)
}

/// *WARNING*: This is a debug endpoint, backwards-compatibility is not guaranteed.
#[ic_cdk::query]
fn get_events(
    args: dex_types_internal::event::GetEventsArgs,
) -> dex_types_internal::event::GetEventsResult {
    use dex_canister::state::event::{Event, EventType};
    use dex_types_internal::event;

    const MAX_EVENTS_PER_RESPONSE: u64 = 2_000;

    fn map_pair_token(token: dex_canister::order::PairToken) -> event::PairToken {
        match token {
            dex_canister::order::PairToken::Base => event::PairToken::Base,
            dex_canister::order::PairToken::Quote => event::PairToken::Quote,
        }
    }

    fn map_balance_operation(
        op: dex_canister::state::event::BalanceOperation,
    ) -> event::BalanceOperation {
        match op {
            dex_canister::state::event::BalanceOperation::Transfer {
                from_order,
                to_order,
                token,
                amount,
            } => event::BalanceOperation::Transfer {
                from_order: from_order.get(),
                to_order: to_order.get(),
                token: map_pair_token(token),
                amount: amount.into(),
            },
            dex_canister::state::event::BalanceOperation::Unreserve {
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
            timestamp: event.timestamp,
            payload: match event.payload {
                EventType::Init(args) => event::EventType::Init(args),
                EventType::Upgrade(args) => event::EventType::Upgrade(args),
                EventType::AddTradingPair(e) => {
                    event::EventType::AddTradingPair(event::AddTradingPairEvent {
                        book_id: e.book_id.get(),
                        base: dex_types::TokenId::from(e.base),
                        quote: dex_types::TokenId::from(e.quote),
                        tick_size: e.tick_size.get(),
                        lot_size: e.lot_size.get(),
                        base_metadata: dex_types::TokenMetadata::from(e.base_metadata),
                        quote_metadata: dex_types::TokenMetadata::from(e.quote_metadata),
                    })
                }
                EventType::Deposit(e) => event::EventType::Deposit(event::DepositEvent {
                    user: e.user,
                    token: dex_types::TokenId::from(e.token),
                    amount: e.amount.into(),
                }),
                EventType::Withdraw(e) => event::EventType::Withdraw(event::WithdrawEvent {
                    block_index: e.block_index,
                    user: e.user,
                    token: dex_types::TokenId::from(e.token),
                    amount: e.amount.into(),
                }),
                EventType::AddLimitOrder(e) => {
                    event::EventType::AddLimitOrder(event::AddLimitOrderEvent {
                        user: e.user,
                        order_id: event::OrderId {
                            book_id: e.order_id.book_id().get(),
                            seq: e.order_id.seq().get(),
                        },
                        side: dex_types::Side::from(e.side),
                        price: e.price.get(),
                        quantity: e.quantity.into(),
                    })
                }
                EventType::Matching(e) => event::EventType::Matching(event::MatchingEvent {
                    book_id: e.book_id.get(),
                    orders: e.orders.into_iter().map(|s| s.get()).collect(),
                }),
                EventType::Settling(e) => event::EventType::Settling(event::SettlingEvent {
                    book_id: e.book_id.get(),
                    balance_operations: e
                        .balance_operations
                        .into_iter()
                        .map(map_balance_operation)
                        .collect(),
                    transitions: e
                        .transitions
                        .into_iter()
                        .map(|t| event::OrderStatusTransition {
                            seq: t.seq.get(),
                            status: dex_types::OrderStatus::from(t.status),
                        })
                        .collect(),
                }),
            },
        }
    }

    let start = usize::try_from(args.start).expect("BUG: start index exceeds usize::MAX");
    let length = usize::try_from(args.length.min(MAX_EVENTS_PER_RESPONSE))
        .expect("BUG: length exceeds usize::MAX");
    let events = dex_canister::storage::with_event_iter(|it| {
        it.skip(start).take(length).map(map_event).collect()
    });
    event::GetEventsResult {
        events,
        total_event_count: dex_canister::storage::total_event_count(),
    }
}

#[ic_cdk::init]
fn init(arg: DexArg) {
    dex_canister::lifecycle::init(arg, &dex_canister::IC_RUNTIME);
}

#[ic_cdk::pre_upgrade]
fn pre_upgrade() {
    dex_canister::lifecycle::pre_upgrade(&dex_canister::IC_RUNTIME);
}

#[ic_cdk::post_upgrade]
fn post_upgrade(arg: Option<DexArg>) {
    dex_canister::lifecycle::post_upgrade(arg, &dex_canister::IC_RUNTIME);
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
            let total_events = dex_canister::storage::total_event_count();
            let dashboard = dex_canister::state::with_state(|s| {
                dex_canister::dashboard::DashboardTemplate::from_state(s, canister_id, total_events)
            });
            match dashboard.render() {
                Ok(body) => HttpResponseBuilder::ok()
                    .header("Content-Type", "text/html; charset=utf-8")
                    .with_body_and_content_length(body)
                    .build(),
                Err(e) => HttpResponseBuilder::server_error(format!("template error: {e}")).build(),
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
    let old_interface =
        std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("dex.did");

    service_equal(
        CandidSource::Text(&new_interface),
        CandidSource::File(old_interface.as_path()),
    )
    .unwrap();
}
