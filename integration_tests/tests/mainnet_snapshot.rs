use candid::{Decode, Encode, Principal};
use oisy_trade_client::OisyTradeClient;
use oisy_trade_int_tests::{PocketIcRuntime, oisy_trade_wasm};
use oisy_trade_types::{
    GetBalancesError, GetMyOrdersArgs, GetMyOrdersError, UserOrder, UserTokenBalance,
};
use oisy_trade_types_internal::OisyTradeArg;
use oisy_trade_types_internal::event::{Event, EventType, GetEventsArgs, GetEventsResult};
use pocket_ic::nonblocking::PocketIc;
use pocket_ic::{CanisterSettings, PocketIcBuilder};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const SNAPSHOT_URL: &str = "https://dfinity-download-public.s3.eu-central-1.amazonaws.com/testdata/oisy_trade/2026_07_10_oisy_trade_snapshot_00000000000000000000000002300fe50101.tar.gz";
const SNAPSHOT_SHA256: &str = "f31fe17fcb222b08d12d6b12884680e8f11091a1b34e8ad1272d773ee72df58b";
const MAINNET_OISY_TRADE_ID: &str = "sy2xe-miaaa-aaaar-qb7sq-cai";
const CONTROLLER: Principal = Principal::from_slice(&[0x9d, 0xf7, 0x02]);

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

/// Downloads the mainnet snapshot tarball and extracts it into a directory laid
/// out as `canister_snapshot_upload` expects (metadata.json plus the memory
/// dumps). Mirrors the repo's curl-based external-artifact download: the archive
/// is cached under the target dir and verified against a pinned SHA-256, so a
/// truncated or tampered download is rejected instead of silently reused.
fn download_and_extract_snapshot() -> PathBuf {
    let tmp = std::env::var("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    let archive = tmp.join("oisy_trade_mainnet_snapshot.tar.gz");
    ensure_snapshot_archive(&archive);

    let snapshot_dir = tmp.join("oisy_trade_mainnet_snapshot");
    let _ = std::fs::remove_dir_all(&snapshot_dir);
    std::fs::create_dir_all(&snapshot_dir).unwrap();
    let status = Command::new("tar")
        .args(["xzf"])
        .arg(&archive)
        .arg("-C")
        .arg(&snapshot_dir)
        .args(["--strip-components=1"])
        .status()
        .expect("failed to run tar to extract the snapshot");
    if !status.success() {
        let _ = std::fs::remove_file(&archive);
        panic!(
            "tar failed to extract the snapshot; removed the cached archive so the next run re-downloads"
        );
    }
    snapshot_dir
}

fn ensure_snapshot_archive(archive: &Path) {
    if archive.exists() && sha256_hex(archive) == SNAPSHOT_SHA256 {
        return;
    }
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(archive)
        .arg(SNAPSHOT_URL)
        .status()
        .expect("failed to run curl to download the snapshot");
    if !status.success() {
        let _ = std::fs::remove_file(archive);
        panic!("curl failed to download the snapshot");
    }
    let actual = sha256_hex(archive);
    if actual != SNAPSHOT_SHA256 {
        let _ = std::fs::remove_file(archive);
        panic!("snapshot SHA-256 mismatch: expected {SNAPSHOT_SHA256}, got {actual}");
    }
}

fn sha256_hex(path: &Path) -> String {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .or_else(|_| {
            Command::new("shasum")
                .args(["-a", "256"])
                .arg(path)
                .output()
        })
        .expect("no SHA-256 tool found (need sha256sum or shasum)");
    assert!(output.status.success(), "SHA-256 computation failed");
    String::from_utf8(output.stdout)
        .expect("SHA-256 output is not UTF-8")
        .split_whitespace()
        .next()
        .expect("empty SHA-256 output")
        .to_string()
}
