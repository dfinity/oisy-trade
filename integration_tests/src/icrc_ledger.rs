use candid::{CandidType, Decode, Encode, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{TransferArg, TransferError};
use icrc_ledger_types::icrc2::approve::{ApproveArgs, ApproveError};
use pocket_ic::nonblocking::PocketIc;
use pocket_ic::{CanisterId, CanisterSettings};
use serde::{Deserialize, Serialize};

pub const BASE_LEDGER_FEE: u64 = 5_000u64;
pub const QUOTE_LEDGER_FEE: u64 = 10u64;
/// Ledger transfer fees of the tokens used in the `for-users.md` walkthrough:
/// ckDevnetSOL (9 decimals) and ckSepoliaETH (18 decimals).
pub const DEVNET_SOL_LEDGER_FEE: u64 = 50u64;
pub const SEPOLIA_ETH_LEDGER_FEE: u64 = 10_000_000_000u64;

/// Symbol, decimals and transfer fee of an ICRC ledger under test.
#[derive(Clone, Debug)]
pub struct LedgerConfig {
    pub symbol: String,
    pub decimals: u8,
    pub transfer_fee: u64,
}

impl LedgerConfig {
    pub fn cksol() -> Self {
        Self {
            symbol: "ckSOL".to_string(),
            decimals: 9,
            transfer_fee: BASE_LEDGER_FEE,
        }
    }

    pub fn ckbtc() -> Self {
        Self {
            symbol: "ckBTC".to_string(),
            decimals: 8,
            transfer_fee: QUOTE_LEDGER_FEE,
        }
    }

    pub fn ck_devnet_sol() -> Self {
        Self {
            symbol: "ckDevnetSOL".to_string(),
            decimals: 9,
            transfer_fee: DEVNET_SOL_LEDGER_FEE,
        }
    }

    pub fn ck_sepolia_eth() -> Self {
        Self {
            symbol: "ckSepoliaETH".to_string(),
            decimals: 18,
            transfer_fee: SEPOLIA_ETH_LEDGER_FEE,
        }
    }
}

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
    pub feature_flags: Option<FeatureFlags>,
    pub archive_options: ArchiveOptions,
    pub index_principal: Option<Principal>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct FeatureFlags {
    pub icrc2: bool,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub enum ChangeFeeCollector {
    Unset,
    SetTo(Account),
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct ChangeArchiveOptions {
    pub num_blocks_to_archive: Option<u64>,
    pub max_transactions_per_response: Option<u64>,
    pub trigger_threshold: Option<u64>,
    pub max_message_size_bytes: Option<u64>,
    pub cycles_for_archive_creation: Option<u64>,
    pub node_max_memory_size_bytes: Option<u64>,
    pub controller_id: Option<Principal>,
    pub more_controller_ids: Option<Vec<Principal>>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct UpgradeArgs {
    pub metadata: Option<
        Vec<(
            String,
            icrc_ledger_types::icrc::generic_metadata_value::MetadataValue,
        )>,
    >,
    pub token_symbol: Option<String>,
    pub token_name: Option<String>,
    pub transfer_fee: Option<Nat>,
    pub change_fee_collector: Option<ChangeFeeCollector>,
    pub max_memo_length: Option<u16>,
    pub feature_flags: Option<FeatureFlags>,
    pub change_archive_options: Option<ChangeArchiveOptions>,
    pub index_principal: Option<Principal>,
}

#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub enum LedgerArg {
    Init(Box<InitArgs>),
    Upgrade(Option<Box<UpgradeArgs>>),
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

pub fn ledger_init_args(controller: Principal, config: &LedgerConfig) -> InitArgs {
    InitArgs {
        minting_account: Account {
            owner: controller,
            subaccount: None,
        },
        fee_collector_account: None,
        transfer_fee: Nat::from(config.transfer_fee),
        decimals: Some(config.decimals),
        max_memo_length: Some(256),
        token_symbol: config.symbol.clone(),
        token_name: config.symbol.clone(),
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

    pub async fn icrc1_transfer(
        &self,
        caller: Principal,
        to: Account,
        amount: impl Into<Nat>,
    ) -> Nat {
        let amount = amount.into();
        let args = TransferArg {
            from_subaccount: None,
            to,
            fee: None,
            created_at_time: None,
            memo: None,
            amount,
        };
        let response = self
            .env
            .update_call(
                self.canister_id,
                caller,
                "icrc1_transfer",
                Encode!(&args).unwrap(),
            )
            .await
            .expect("Failed to call icrc1_transfer");
        Decode!(&response, Result<Nat, TransferError>)
            .expect("Failed to decode icrc1_transfer response")
            .expect("icrc1_transfer failed")
    }

    pub async fn icrc2_approve(
        &self,
        caller: Principal,
        spender: Account,
        amount: impl Into<Nat>,
    ) -> Nat {
        let amount = amount.into();
        let args = ApproveArgs {
            from_subaccount: None,
            spender,
            amount,
            expected_allowance: None,
            expires_at: None,
            fee: None,
            memo: None,
            created_at_time: None,
        };
        let response = self
            .env
            .update_call(
                self.canister_id,
                caller,
                "icrc2_approve",
                Encode!(&args).unwrap(),
            )
            .await
            .expect("Failed to call icrc2_approve");
        Decode!(&response, Result<Nat, ApproveError>)
            .expect("Failed to decode icrc2_approve response")
            .expect("icrc2_approve failed")
    }

    pub async fn icrc1_balance_of(&self, account: impl Into<Account>) -> Nat {
        let account = account.into();
        let response = self
            .env
            .query_call(
                self.canister_id,
                Principal::anonymous(),
                "icrc1_balance_of",
                Encode!(&account).unwrap(),
            )
            .await
            .expect("Failed to call icrc1_balance_of");
        Decode!(&response, Nat).expect("Failed to decode icrc1_balance_of response")
    }
}
