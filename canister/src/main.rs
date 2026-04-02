use dex_types::{
    AddLimitOrderError, AddTradingPairError, AddTradingPairRequest, DepositError, DepositRequest,
    DepositResponse, LimitOrderRequest, OrderId, OrderStatus, TokenId, TradingPairInfo,
};

#[ic_cdk::init]
fn init() {
    dex_canister::state::init_state();
    // TODO DEFI-2744: replace with an admin endpoint
    dex_canister::register_default_trading_pairs();
}

#[ic_cdk::update]
fn add_limit_order(request: LimitOrderRequest) -> Result<OrderId, AddLimitOrderError> {
    dex_canister::add_limit_order(request)
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
    dex_canister::deposit(request, &dex_canister::runtime::IC_RUNTIME).await
}

#[ic_cdk::query]
fn get_balance(token_id: TokenId) -> candid::Nat {
    dex_canister::get_balance(token_id)
}

#[ic_cdk::update]
fn add_trading_pair(request: AddTradingPairRequest) -> Result<(), AddTradingPairError> {
    dex_canister::add_trading_pair(request)
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
