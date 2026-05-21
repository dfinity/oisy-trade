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
    /// Maximum pending orders matched per chunked-execution chunk.
    #[cfg_attr(feature = "event", n(1))]
    pub max_orders_per_chunk: u64,
    /// Maximum instructions consumed before a chunk yields.
    #[cfg_attr(feature = "event", n(2))]
    pub instruction_budget: u64,
}

/// Argument for canister upgrade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
#[cfg_attr(feature = "event", derive(minicbor::Encode, minicbor::Decode))]
pub struct UpgradeArg {
    /// If set, changes who may call update endpoints.
    #[cfg_attr(feature = "event", n(0))]
    pub mode: Option<Mode>,
    /// If set, overrides the current `max_orders_per_chunk`.
    #[cfg_attr(feature = "event", n(1))]
    pub max_orders_per_chunk: Option<u64>,
    /// If set, overrides the current `instruction_budget`.
    #[cfg_attr(feature = "event", n(2))]
    pub instruction_budget: Option<u64>,
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
