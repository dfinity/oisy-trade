use async_trait::async_trait;
use candid::Principal;
use candid::utils::ArgumentEncoder;
use ic_cdk::call::{Call, CallFailed, Response};

pub const IC_RUNTIME: IcRuntime = IcRuntime;

/// Abstract IC-specific methods that are only available when the canister is running on the IC.
#[async_trait]
pub trait Runtime {
    async fn call_unbounded_wait<A>(
        &self,
        canister_id: Principal,
        method: &str,
        args: A,
    ) -> Result<Response, CallFailed>
    where
        A: ArgumentEncoder + Send;

    /// Gets the identity of the caller, which may be a canister id or a user id.
    fn msg_caller(&self) -> Principal;

    /// Gets canister's own identity.
    fn canister_self(&self) -> Principal;
}

#[derive(Copy, Clone)]
pub struct IcRuntime;

#[async_trait]
impl Runtime for IcRuntime {
    async fn call_unbounded_wait<A>(
        &self,
        canister_id: Principal,
        method: &str,
        args: A,
    ) -> Result<Response, CallFailed>
    where
        A: ArgumentEncoder + Send,
    {
        Call::unbounded_wait(canister_id, method)
            .with_args(&args)
            .await
    }

    fn msg_caller(&self) -> Principal {
        ic_cdk::api::msg_caller()
    }

    fn canister_self(&self) -> Principal {
        ic_cdk::api::canister_self()
    }
}
