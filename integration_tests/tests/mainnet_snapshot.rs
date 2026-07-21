use candid::{Decode, Encode, Principal};
use oisy_trade_int_tests::oisy_trade_wasm;
use oisy_trade_types_internal::OisyTradeArg;
use oisy_trade_types_internal::event::{GetEventsArgs, GetEventsResult};
use pocket_ic::nonblocking::PocketIc;
use pocket_ic::{CanisterSettings, PocketIcBuilder};
use std::path::{Path, PathBuf};
use std::process::Command;

const SNAPSHOT_URL: &str = "https://dfinity-download-public.s3.eu-central-1.amazonaws.com/testdata/oisy_trade/2026_07_10_oisy_trade_snapshot_00000000000000000000000002300fe50101.tar.gz";
const MAINNET_OISY_TRADE_ID: &str = "sy2xe-miaaa-aaaar-qb7sq-cai";
const CONTROLLER: Principal = Principal::from_slice(&[0x9d, 0xf7, 0x02]);

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

    let total_event_count = event_count(&env, canister_id).await;
    assert!(
        total_event_count > 0,
        "mainnet event log should decode to a non-empty log after upgrade"
    );

    env.drop().await;
}

async fn event_count(env: &PocketIc, canister_id: Principal) -> u64 {
    let bytes = env
        .query_call(
            canister_id,
            Principal::anonymous(),
            "get_events",
            Encode!(&GetEventsArgs {
                start: 0,
                length: 1,
            })
            .unwrap(),
        )
        .await
        .expect("get_events query failed");
    Decode!(&bytes, GetEventsResult).unwrap().total_event_count
}

/// Downloads the mainnet snapshot tarball and extracts it into a directory laid
/// out as `canister_snapshot_upload` expects (metadata.json plus the memory
/// dumps). Mirrors the repo's curl-based external-artifact download; caches the
/// archive under `CARGO_TARGET_TMPDIR`.
fn download_and_extract_snapshot() -> PathBuf {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let archive = tmp.join("oisy_trade_mainnet_snapshot.tar.gz");
    if !archive.exists() {
        let status = Command::new("curl")
            .args(["-fsSL", "-o"])
            .arg(&archive)
            .arg(SNAPSHOT_URL)
            .status()
            .expect("failed to run curl to download the snapshot");
        assert!(status.success(), "curl failed to download the snapshot");
    }

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
    assert!(status.success(), "tar failed to extract the snapshot");
    snapshot_dir
}
