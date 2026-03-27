//! Client to interact with the DEX canister

#![forbid(unsafe_code)]
#![forbid(missing_docs)]

use async_trait::async_trait;
use candid::utils::ArgumentEncoder;
use candid::{CandidType, Principal};
use dex_types::{DummyRequest, DummyResponse};
use ic_cdk::call::{Call, CallFailed, RejectCode};
use serde::de::DeserializeOwned;

/// Abstract the canister runtime so that the client code can be reused:
/// * in production using `ic_cdk`,
/// * in unit tests by mocking this trait,
/// * in integration tests by implementing this trait for `PocketIc`.
#[async_trait]
pub trait Runtime {
    /// Defines how asynchronous inter-canister calls are made.
    async fn call<In, Out>(
        &self,
        id: Principal,
        method: &str,
        args: In,
        cycles: u128,
    ) -> Result<Out, (RejectCode, String)>
    where
        In: ArgumentEncoder + Send + 'static,
        Out: CandidType + DeserializeOwned + 'static;
}

/// Client to interact with the DEX canister.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DexClient<R: Runtime> {
    runtime: R,
    dex_canister: Principal,
}

impl DexClient<IcRuntime> {
    /// Instantiate a new client to be used by a canister on the Internet Computer.
    ///
    /// To use another runtime, see [`Self::new`].
    pub fn new_for_ic(dex_canister: Principal) -> Self {
        Self {
            runtime: IcRuntime {},
            dex_canister,
        }
    }
}

impl<R: Runtime> DexClient<R> {
    /// Instantiate a new client with a specific runtime.
    ///
    /// To use the client inside a canister, see [`DexClient<IcRuntime>::new_for_ic`].
    pub fn new(runtime: R, dex_canister: Principal) -> Self {
        Self {
            runtime,
            dex_canister,
        }
    }

    /// Call `greet` on the DEX canister.
    pub async fn greet(&self, request: DummyRequest) -> DummyResponse {
        self.runtime
            .call(self.dex_canister, "greet", (request,), 10_000)
            .await
            .unwrap()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
struct IcRuntime {}

#[async_trait]
impl Runtime for IcRuntime {
    async fn call<In, Out>(
        &self,
        id: Principal,
        method: &str,
        args: In,
        cycles: u128,
    ) -> Result<Out, (RejectCode, String)>
    where
        In: ArgumentEncoder + Send + 'static,
        Out: CandidType + DeserializeOwned + 'static,
    {
        let response = Call::bounded_wait(id, method)
            .with_args(&args)
            .with_cycles(cycles)
            .await
            .map_err(call_failed_to_reject)?;
        response
            .candid_tuple::<(Out,)>()
            .map(|(res,)| res)
            .map_err(|e| (RejectCode::CanisterError, e.to_string()))
    }
}

fn call_failed_to_reject(err: CallFailed) -> (RejectCode, String) {
    match err {
        CallFailed::CallRejected(rejected) => (
            rejected.reject_code().unwrap_or(RejectCode::SysUnknown),
            rejected.reject_message().to_string(),
        ),
        other => (RejectCode::CanisterError, other.to_string()),
    }
}
