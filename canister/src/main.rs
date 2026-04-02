use dex_types::{
    AddLimitOrderError, DepositError, DepositRequest, DepositResponse, DexArg,
    LedgerTransferFromError, LimitOrderRequest, OrderId, OrderStatus, TokenId, TradingPairInfo,
};
use dex_types_internal::log::Priority;
use ic_http_types::{HttpRequest, HttpResponse};

#[ic_cdk::init]
fn init(arg: DexArg) {
    match arg {
        DexArg::Init(_) => {}
        DexArg::Upgrade(_) => {
            ic_cdk::trap("ERROR: expected Init argument");
        }
    }
    dex_canister::state::init_state();
    // TODO DEFI-2744: replace with an admin endpoint
    dex_canister::register_default_trading_pairs();
    canlog::log!(Priority::Info, "[init]: DEX canister initialized");
}

#[ic_cdk::post_upgrade]
fn post_upgrade(arg: DexArg) {
    match arg {
        DexArg::Init(_) => {
            ic_cdk::trap("ERROR: expected Upgrade argument");
        }
        DexArg::Upgrade(_) => {}
    }
}

#[ic_cdk::update]
fn add_limit_order(request: LimitOrderRequest) -> Result<OrderId, AddLimitOrderError> {
    let order_dbg = format!("{request:?}");
    dex_canister::add_limit_order(request).inspect(|order_id| {
        canlog::log!(
            Priority::Info,
            "[add_limit_order]: created order_id={} for request {order_dbg}",
            order_id
        );
    })
}

#[ic_cdk::query]
fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    dex_canister::get_order_status(order_id)
}

#[ic_cdk::query]
fn get_trading_pairs() -> Vec<TradingPairInfo> {
    dex_canister::get_trading_pairs()
}

#[ic_cdk::update]
async fn deposit(request: DepositRequest) -> Result<DepositResponse, DepositError> {
    let deposit_dbg = format!("{request:?}");
    let result = dex_canister::deposit(request).await;
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
            DepositError::LedgerError(LedgerTransferFromError::InsufficientFunds { .. })
            | DepositError::LedgerError(LedgerTransferFromError::InsufficientAllowance {
                ..
            }) => {
                // do not log errors due to user actions
            }
        },
    }
    result
}

#[ic_cdk::query]
fn get_balance(token_id: TokenId) -> candid::Nat {
    dex_canister::get_balance(token_id)
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
