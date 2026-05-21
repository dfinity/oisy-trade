//! Internal types used by the DEX canister.
//!
//! Types in this module are unstable and breaking changes do not break the canister API.

#![forbid(unsafe_code)]

use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[cfg(feature = "event")]
pub mod cbor;
pub mod event;
#[cfg(feature = "log")]
pub mod log;

/// Argument passed to the DEX canister on init or upgrade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum DexArg {
    /// Argument used when the canister is first installed.
    Init(InitArg),
    /// Argument used when the canister is upgraded.
    Upgrade(Option<UpgradeArg>),
}

/// Argument for canister initialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
#[cfg_attr(feature = "event", derive(minicbor::Encode, minicbor::Decode))]
pub struct InitArg {
    /// Controls who may call update endpoints.
    #[cfg_attr(feature = "event", n(0))]
    pub mode: Mode,
    /// Bounds the chunked matching pipeline's per-message work. `None`
    /// resolves to [`ExecutionPolicy::PRODUCTION_DEFAULT`].
    #[cfg_attr(feature = "event", n(1))]
    pub execution_policy: Option<ExecutionPolicy>,
}

/// Argument for canister upgrade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
#[cfg_attr(feature = "event", derive(minicbor::Encode, minicbor::Decode))]
pub struct UpgradeArg {
    /// If set, changes who may call update endpoints.
    #[cfg_attr(feature = "event", n(0))]
    pub mode: Option<Mode>,
    /// If set, replaces the chunked-matching execution policy. Persists
    /// across subsequent upgrades unless overridden again.
    #[cfg_attr(feature = "event", n(1))]
    pub execution_policy: Option<ExecutionPolicy>,
}

/// Bounds for a single matching chunk. Both fields are consulted by the
/// canister's chunked execution loop to decide when to yield and
/// self-reschedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, CandidType)]
#[cfg_attr(feature = "event", derive(minicbor::Encode, minicbor::Decode))]
pub struct ExecutionPolicy {
    /// Maximum number of pending orders matched per chunk.
    #[cfg_attr(feature = "event", n(0))]
    pub max_orders_per_chunk: u64,
    /// Maximum instructions consumed before the chunk yields. ~5% of the
    /// IC's 20B per-message cap is a comfortable production setting.
    #[cfg_attr(feature = "event", n(1))]
    pub instruction_budget: u64,
}

impl ExecutionPolicy {
    /// Conservative production policy: 1 000 orders per chunk, 1B
    /// instructions per chunk.
    pub const PRODUCTION_DEFAULT: Self = Self {
        max_orders_per_chunk: 1_000,
        instruction_budget: 1_000_000_000,
    };
}

/// Controls who may call update endpoints on the DEX canister.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, CandidType)]
#[cfg_attr(feature = "event", derive(minicbor::Encode, minicbor::Decode))]
pub enum Mode {
    /// Anyone may call update endpoints.
    #[default]
    #[cfg_attr(feature = "event", n(0))]
    GeneralAvailability,
    /// Only the listed principals may call update endpoints.
    #[cfg_attr(feature = "event", n(1))]
    RestrictedTo(
        #[cfg_attr(
            feature = "event",
            n(0),
            cbor(with = "crate::cbor::btreeset_principal")
        )]
        BTreeSet<Principal>,
    ),
}

impl Mode {
    pub fn restricted_to<I: IntoIterator<Item = Principal>>(principals: I) -> Self {
        Self::RestrictedTo(principals.into_iter().collect())
    }
}
