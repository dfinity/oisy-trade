use async_trait::async_trait;
use candid::Principal;
use candid::utils::ArgumentEncoder;
use ic_cdk::call::{Call, CallFailed, Response};

pub const IC_RUNTIME: IcRuntime = IcRuntime;

/// A point in time, in nanoseconds since the Unix epoch, as reported by the IC
/// system API.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, minicbor::Encode, minicbor::Decode,
)]
pub struct Timestamp(#[n(0)] u64);

impl Timestamp {
    /// The Unix epoch (zero nanoseconds).
    pub const EPOCH: Self = Self(0);
    /// The maximum-representable `Timestamp` sentinel (max encoded width);
    /// not a meaningful IC time.
    pub const MAX: Self = Self(u64::MAX);

    pub const fn new(nanos: u64) -> Self {
        Self(nanos)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }
}

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

    /// Checks if the given principal is a controller of the canister.
    fn is_controller(&self, principal: &Principal) -> bool;

    /// Returns the number of instructions consumed since the beginning of the current message execution.
    fn instruction_counter(&self) -> u64;

    /// Returns the current time in nanoseconds since the Unix epoch.
    fn time(&self) -> Timestamp;

    /// Arms a zero-delay timer that runs one matching round, so orders an
    /// endpoint just enqueued are processed without waiting for the periodic
    /// timer.
    ///
    /// Endpoints arm it through this method rather than calling
    /// `ic_cdk_timers` directly so a test can count the kickoffs: whether an
    /// endpoint arms matching is part of its contract (a cancel must not, or it
    /// would drag unrelated pending orders into a round that the equivalent
    /// single-order calls never triggered) and no state or event-log assertion
    /// can observe it.
    fn schedule_matching(&self);
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

    fn is_controller(&self, principal: &Principal) -> bool {
        ic_cdk::api::is_controller(principal)
    }

    fn instruction_counter(&self) -> u64 {
        ic_cdk::api::instruction_counter()
    }

    fn time(&self) -> Timestamp {
        Timestamp::new(ic_cdk::api::time())
    }

    fn schedule_matching(&self) {
        // TODO DEFI-2823: coalesce zero-delay matching timers so a burst of
        // kickoffs plus `drive_matching`'s self-reschedule chain doesn't queue
        // O(N) redundant timers per burst.
        ic_cdk_timers::set_timer(std::time::Duration::ZERO, async {
            crate::drive_matching();
        });
    }
}
