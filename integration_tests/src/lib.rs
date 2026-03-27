use async_trait::async_trait;
use candid::utils::ArgumentEncoder;
use candid::{decode_args, encode_args, CandidType, Encode, Principal};
use ic_cdk::call::RejectCode;
use pocket_ic::{nonblocking::PocketIc, CanisterId, CanisterSettings, PocketIcBuilder};
use serde::de::DeserializeOwned;
use dex_client::{DexClient, Runtime};
use std::path::PathBuf;

pub struct Setup {
    env: PocketIc,
    caller: Principal,
    _controller: Principal,
    canister_id: CanisterId,
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
            Encode!().unwrap(),
            Some(controller),
        )
        .await;
        let caller = DEFAULT_CALLER_TEST_ID;

        Self {
            env,
            caller,
            _controller: controller,
            canister_id,
        }
    }

    pub fn client(&self) -> DexClient<PocketIcRuntime> {
        DexClient::new(self.new_pocket_ic(), self.canister_id)
    }

    fn new_pocket_ic(&self) -> PocketIcRuntime {
        PocketIcRuntime {
            env: &self.env,
            caller: self.caller,
        }
    }

    pub async fn drop(self) {
        self.env.drop().await
    }
}

fn dex_wasm() -> Vec<u8> {
    ic_test_utilities_load_wasm::load_wasm(
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../canister"),
        "dex_canister",
        &[],
    )
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
