use dex_types::{LimitOrderRequest, LimitOrderResponse, OrderStatus};

#[ic_cdk::update]
fn add_limit_order(_request: LimitOrderRequest) -> LimitOrderResponse {
    let order_id = dex_canister::state::with_state_mut(|s| s.add_order());
    LimitOrderResponse {
        order_id: u64::from(order_id),
    }
}

#[ic_cdk::query]
fn get_order_status(order_id: dex_types::OrderId) -> OrderStatus {
    dex_canister::state::with_state(|s| {
        s.get_order_status(dex_canister::order::OrderId::from(order_id))
    })
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
