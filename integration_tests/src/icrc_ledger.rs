use candid::{CandidType, Decode, Encode, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::nonblocking::PocketIc;
use pocket_ic::{CanisterId, CanisterSettings};
use serde::{Deserialize, Serialize};

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct ArchiveOptions {
    pub num_blocks_to_archive: u64,
    pub max_transactions_per_response: Option<u64>,
    pub trigger_threshold: u64,
    pub max_message_size_bytes: Option<u64>,
    pub cycles_for_archive_creation: Option<u64>,
    pub node_max_memory_size_bytes: Option<u64>,
    pub controller_id: Principal,
    pub more_controller_ids: Option<Vec<Principal>>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct InitArgs {
    pub minting_account: Account,
    pub fee_collector_account: Option<Account>,
    pub transfer_fee: Nat,
    pub decimals: Option<u8>,
    pub max_memo_length: Option<u16>,
    pub token_symbol: String,
    pub token_name: String,
    pub metadata: Vec<(
        String,
        icrc_ledger_types::icrc::generic_metadata_value::MetadataValue,
    )>,
    pub initial_balances: Vec<(Account, Nat)>,
    pub feature_flags: Option<()>,
    pub archive_options: ArchiveOptions,
    pub index_principal: Option<Principal>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub enum LedgerArg {
    Init(Box<InitArgs>),
    Upgrade(Option<()>),
}

pub async fn install_ledger(
    env: &PocketIc,
    controller: Principal,
    wasm: Vec<u8>,
    init_args: InitArgs,
) -> CanisterId {
    let canister_id = env
        .create_canister_with_settings(
            None,
            Some(CanisterSettings {
                controllers: Some(vec![controller]),
                ..CanisterSettings::default()
            }),
        )
        .await;
    env.add_cycles(canister_id, u128::MAX).await;
    let ledger_arg = LedgerArg::Init(Box::new(init_args));
    env.install_canister(
        canister_id,
        wasm,
        Encode!(&ledger_arg).unwrap(),
        Some(controller),
    )
    .await;
    canister_id
}

fn test_archive_options(controller: Principal) -> ArchiveOptions {
    ArchiveOptions {
        num_blocks_to_archive: 1000,
        max_transactions_per_response: None,
        trigger_threshold: 2000,
        max_message_size_bytes: None,
        cycles_for_archive_creation: None,
        node_max_memory_size_bytes: None,
        controller_id: controller,
        more_controller_ids: None,
    }
}

pub fn cksol_init_args(controller: Principal) -> InitArgs {
    InitArgs {
        minting_account: Account {
            owner: controller,
            subaccount: None,
        },
        fee_collector_account: None,
        transfer_fee: Nat::from(5_000u64),
        decimals: Some(9),
        max_memo_length: Some(256),
        token_symbol: "ckSOL".to_string(),
        token_name: "ckSOL".to_string(),
        metadata: vec![],
        initial_balances: vec![],
        feature_flags: None,
        archive_options: test_archive_options(controller),
        index_principal: None,
    }
}

pub struct LedgerClient<'a> {
    env: &'a PocketIc,
    canister_id: CanisterId,
}

impl<'a> LedgerClient<'a> {
    pub fn new(env: &'a PocketIc, canister_id: CanisterId) -> Self {
        Self { env, canister_id }
    }

    pub async fn icrc1_decimals(&self) -> u8 {
        let response = self
            .env
            .query_call(
                self.canister_id,
                Principal::anonymous(),
                "icrc1_decimals",
                Encode!().unwrap(),
            )
            .await
            .expect("Failed to query icrc1_decimals");
        Decode!(&response, u8).expect("Failed to decode icrc1_decimals response")
    }

    pub async fn icrc1_fee(&self) -> Nat {
        let response = self
            .env
            .query_call(
                self.canister_id,
                Principal::anonymous(),
                "icrc1_fee",
                Encode!().unwrap(),
            )
            .await
            .expect("Failed to query icrc1_fee");
        Decode!(&response, Nat).expect("Failed to decode icrc1_fee response")
    }
}

pub fn ckbtc_init_args(controller: Principal) -> InitArgs {
    InitArgs {
        minting_account: Account {
            owner: controller,
            subaccount: None,
        },
        fee_collector_account: None,
        transfer_fee: Nat::from(10u64),
        decimals: Some(8),
        max_memo_length: Some(256),
        token_symbol: "ckBTC".to_string(),
        token_name: "ckBTC".to_string(),
        metadata: vec![],
        initial_balances: vec![],
        feature_flags: None,
        archive_options: test_archive_options(controller),
        index_principal: None,
    }
}
