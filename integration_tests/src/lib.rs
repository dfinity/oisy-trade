pub mod icrc_ledger;

pub use icrc_ledger::LedgerClient;

use async_trait::async_trait;
use candid::utils::ArgumentEncoder;
use candid::{CandidType, Decode, Encode, Nat, Principal, decode_args, encode_args};
use canlog::{Log, LogEntry};
use dex_client::{DexClient, Runtime};
use dex_types_internal::{DexArg, InitArg};
use dex_types_internal::log::Priority;
use ic_cdk::call::RejectCode;
use ic_http_types::{HttpRequest, HttpResponse};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{CanisterId, CanisterSettings, PocketIcBuilder, nonblocking::PocketIc};
use serde::de::DeserializeOwned;
use std::path::PathBuf;

pub struct Setup {
    env: Option<PocketIc>,
    caller: Principal,
    controller: Principal,
    dex_id: CanisterId,
    base_ledger_id: CanisterId,
    quote_ledger_id: CanisterId,
}

impl Setup {
    pub async fn new() -> Self {
        const DEFAULT_CALLER_TEST_ID: Principal = Principal::from_slice(&[0x9d, 0xf7, 0x01]);
        const DEFAULT_CONTROLLER_TEST_ID: Principal = Principal::from_slice(&[0x9d, 0xf7, 0x02]);

        let env = PocketIcBuilder::new()
            .with_fiduciary_subnet()
            .build_async()
            .await;
        let controller = DEFAULT_CONTROLLER_TEST_ID;
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
        env.install_canister(
            canister_id,
            dex_wasm(),
            Encode!(&DexArg::Init(InitArg {})).unwrap(),
            Some(controller),
        )
        .await;

        let ledger_wasm = ledger_wasm();
        let base_ledger_id = icrc_ledger::install_ledger(
            &env,
            controller,
            ledger_wasm.clone(),
            icrc_ledger::cksol_init_args(controller),
        )
        .await;
        let quote_ledger_id = icrc_ledger::install_ledger(
            &env,
            controller,
            ledger_wasm,
            icrc_ledger::ckbtc_init_args(controller),
        )
        .await;

        let caller = DEFAULT_CALLER_TEST_ID;

        Self {
            env: Some(env),
            caller,
            controller,
            dex_id: canister_id,
            base_ledger_id,
            quote_ledger_id,
        }
    }

    pub fn dex_client(&self) -> DexClient<PocketIcRuntime<'_>> {
        DexClient::new(self.new_pocket_ic(), self.dex_id)
    }

    pub fn base_token_ledger(&self) -> LedgerClient<'_> {
        LedgerClient::new(self.env.as_ref().unwrap(), self.base_ledger_id)
    }

    pub fn quote_token_ledger(&self) -> LedgerClient<'_> {
        LedgerClient::new(self.env.as_ref().unwrap(), self.quote_ledger_id)
    }

    pub fn controller(&self) -> Principal {
        self.controller
    }

    pub fn dex_id(&self) -> CanisterId {
        self.dex_id
    }

    pub fn base_ledger_id(&self) -> CanisterId {
        self.base_ledger_id
    }

    pub fn quote_ledger_id(&self) -> CanisterId {
        self.quote_ledger_id
    }

    pub fn env(&self) -> &PocketIc {
        self.env.as_ref().unwrap()
    }

    pub async fn mint_base_tokens(&self, to: Principal, amount: Nat) -> Nat {
        self.base_token_ledger()
            .icrc1_transfer(
                self.controller,
                Account {
                    owner: to,
                    subaccount: None,
                },
                amount,
            )
            .await
    }

    pub async fn mint_quote_tokens(&self, to: Principal, amount: Nat) -> Nat {
        self.quote_token_ledger()
            .icrc1_transfer(
                self.controller,
                Account {
                    owner: to,
                    subaccount: None,
                },
                amount,
            )
            .await
    }

    pub fn dex_client_with_caller(&self, caller: Principal) -> DexClient<PocketIcRuntime<'_>> {
        DexClient::new(
            PocketIcRuntime {
                env: self.env.as_ref().unwrap(),
                caller,
            },
            self.dex_id,
        )
    }

    fn new_pocket_ic(&self) -> PocketIcRuntime<'_> {
        PocketIcRuntime {
            env: self.env.as_ref().unwrap(),
            caller: self.caller,
        }
    }

    pub async fn retrieve_logs(&self, priority: &Priority) -> Vec<LogEntry<Priority>> {
        let request = HttpRequest {
            method: "GET".to_string(),
            url: format!("/logs?priority={priority}"),
            headers: vec![],
            body: Default::default(),
        };
        let response: HttpResponse = self
            .env()
            .query_call(
                self.dex_id,
                Principal::anonymous(),
                "http_request",
                Encode!(&request).unwrap(),
            )
            .await
            .map(|bytes| Decode!(&bytes, HttpResponse).unwrap())
            .expect("Failed to query http_request");
        serde_json::from_slice::<Log<Priority>>(&response.body)
            .expect("Failed to deserialize logs")
            .entries
    }

    pub async fn drop(self) {
        let mut setup = self;
        if let Some(env) = setup.env.take() {
            env.drop().await
        }
    }
}

impl Drop for Setup {
    fn drop(&mut self) {
        if self.env.is_some() {
            panic!("Setup was not dropped properly. Call Setup::drop().await to clean up.");
        }
    }
}

fn dex_wasm() -> Vec<u8> {
    ic_test_utilities_load_wasm::load_wasm(
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../canister"),
        "dex_canister",
        &[],
    )
}

pub fn ledger_wasm() -> Vec<u8> {
    let path = std::env::var("IC_ICRC1_LEDGER_WASM_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
                .join("../wasms/ic-icrc1-ledger.wasm.gz")
        });
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read ledger WASM at {}: {}\nRun `just download-external-wasms` first.",
            path.display(),
            e
        )
    })
}

#[derive(Clone)]
pub struct PocketIcRuntime<'a> {
    env: &'a PocketIc,
    caller: Principal,
}

#[async_trait]
impl<'a> Runtime for PocketIcRuntime<'a> {
    async fn call<In, Out>(
        &self,
        id: Principal,
        method: &str,
        args: In,
        _cycles: u128,
    ) -> Result<Out, (RejectCode, String)>
    where
        In: ArgumentEncoder + Send + 'static,
        Out: CandidType + DeserializeOwned + 'static,
    {
        let args_raw = encode_args(args).expect("Failed to encode arguments.");
        match self
            .env
            .update_call(id, self.caller, method, args_raw)
            .await
        {
            Ok(bytes) => decode_args(&bytes).map(|(res,)| res).map_err(|e| {
                (
                    RejectCode::CanisterError,
                    format!(
                        "failed to decode canister response as {}: {}",
                        std::any::type_name::<Out>(),
                        e
                    ),
                )
            }),
            Err(e) => {
                let rejection_code = match e.reject_code {
                    pocket_ic::RejectCode::SysFatal => RejectCode::SysFatal,
                    pocket_ic::RejectCode::SysTransient => RejectCode::SysTransient,
                    pocket_ic::RejectCode::DestinationInvalid => RejectCode::DestinationInvalid,
                    pocket_ic::RejectCode::CanisterReject => RejectCode::CanisterReject,
                    pocket_ic::RejectCode::CanisterError => RejectCode::CanisterError,
                    pocket_ic::RejectCode::SysUnknown => RejectCode::SysUnknown,
                };
                Err((rejection_code, e.reject_message))
            }
        }
    }
}
