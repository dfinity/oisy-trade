use candid::{Decode, Encode, Principal};
use oisy_trade_client::OisyTradeClient;
use oisy_trade_int_tests::{
    CONTROLLER, MAINNET_OISY_TRADE_ID, PocketIcRuntime, download_and_extract_snapshot,
    oisy_trade_wasm,
};
use oisy_trade_types::{
    GetBalancesError, GetMyOrdersArgs, GetMyOrdersError, UserOrder, UserTokenBalance,
};
use oisy_trade_types_internal::OisyTradeArg;
use oisy_trade_types_internal::event::{Event, EventType, GetEventsArgs, GetEventsResult};
use pocket_ic::nonblocking::PocketIc;
use pocket_ic::{CanisterSettings, PocketIcBuilder};
use std::collections::{BTreeMap, BTreeSet};

/// The per-user state we require to survive the upgrade unchanged.
type UserState = (
    Result<Vec<UserTokenBalance>, GetBalancesError>,
    Result<Vec<UserOrder>, GetMyOrdersError>,
);

#[tokio::test]
async fn should_load_mainnet_snapshot_and_upgrade_to_current_wasm() {
    let snapshot_dir = download_and_extract_snapshot();

    let env = PocketIcBuilder::new()
        .with_fiduciary_subnet()
        .build_async()
        .await;

    let canister_id = Principal::from_text(MAINNET_OISY_TRADE_ID).unwrap();
    env.create_canister_with_id(
        Some(CONTROLLER),
        Some(CanisterSettings {
            controllers: Some(vec![CONTROLLER]),
            ..CanisterSettings::default()
        }),
        canister_id,
    )
    .await
    .expect("failed to create canister at the mainnet id");
    env.add_cycles(canister_id, u128::MAX).await;

    let snapshot_id = env
        .canister_snapshot_upload(canister_id, CONTROLLER, None, snapshot_dir)
        .await;
    env.stop_canister(canister_id, Some(CONTROLLER))
        .await
        .expect("failed to stop canister before loading snapshot");
    env.load_canister_snapshot(canister_id, Some(CONTROLLER), snapshot_id)
        .await
        .expect("failed to load mainnet snapshot");
    env.start_canister(canister_id, Some(CONTROLLER))
        .await
        .expect("failed to start canister after loading snapshot");

    let users = users_from_events(&all_events(&env, canister_id).await);
    assert!(
        !users.is_empty(),
        "mainnet snapshot should reference at least one user"
    );
    let state_before = collect_user_state(&env, canister_id, &users).await;

    env.stop_canister(canister_id, Some(CONTROLLER))
        .await
        .expect("failed to stop canister before upgrade");
    env.upgrade_canister(
        canister_id,
        oisy_trade_wasm(),
        Encode!(&OisyTradeArg::Upgrade(None)).unwrap(),
        Some(CONTROLLER),
    )
    .await
    .expect("failed to upgrade mainnet snapshot to the current wasm");
    env.start_canister(canister_id, Some(CONTROLLER))
        .await
        .expect("failed to start canister after upgrade");

    let state_after = collect_user_state(&env, canister_id, &users).await;
    assert_eq!(
        state_before, state_after,
        "user balances and orders must be identical after the upgrade"
    );

    env.drop().await;
}

async fn collect_user_state(
    env: &PocketIc,
    canister_id: Principal,
    users: &BTreeSet<Principal>,
) -> BTreeMap<Principal, UserState> {
    let mut state = BTreeMap::new();
    for &user in users {
        let client = OisyTradeClient::new(PocketIcRuntime::new(env, user), canister_id);
        let balances = client.get_balances(None).await;
        let orders = client.get_my_orders(GetMyOrdersArgs::default()).await;
        state.insert(user, (balances, orders));
    }
    state
}

fn users_from_events(events: &[Event]) -> BTreeSet<Principal> {
    let mut users = BTreeSet::new();
    for event in events {
        match &event.payload {
            EventType::Deposit(e) => {
                users.insert(e.user);
            }
            EventType::Withdraw(e) => {
                users.insert(e.user);
            }
            EventType::AddLimitOrder(e) => {
                users.insert(e.user);
                users.extend(e.placed_by);
            }
            EventType::AddTradingAccount(e) => {
                users.insert(e.funding);
                users.insert(e.trading);
            }
            EventType::RemoveTradingAccount(e) => {
                users.insert(e.funding);
                users.insert(e.trading);
            }
            _ => {}
        }
    }
    users
}

async fn all_events(env: &PocketIc, canister_id: Principal) -> Vec<Event> {
    const BATCH: u64 = 2000;
    let mut events = Vec::new();
    loop {
        let GetEventsResult {
            events: batch,
            total_event_count,
        } = get_events(env, canister_id, events.len() as u64, BATCH).await;
        events.extend(batch);
        if events.len() as u64 >= total_event_count {
            break;
        }
    }
    events
}

async fn get_events(
    env: &PocketIc,
    canister_id: Principal,
    start: u64,
    length: u64,
) -> GetEventsResult {
    let bytes = env
        .query_call(
            canister_id,
            Principal::anonymous(),
            "get_events",
            Encode!(&GetEventsArgs { start, length }).unwrap(),
        )
        .await
        .expect("get_events query failed");
    Decode!(&bytes, GetEventsResult).unwrap()
}
