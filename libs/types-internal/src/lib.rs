//! Internal types used by the DEX canister.
//!
//! Types in this module are unstable and breaking changes do not break the canister API.

#![forbid(unsafe_code)]

use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
pub struct InitArg {
    /// Controls who may call update endpoints.
    pub mode: Mode,
}

/// Argument for canister upgrade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct UpgradeArg {
    /// If set, changes who may call update endpoints.
    pub mode: Option<Mode>,
}

/// Controls who may call update endpoints on the DEX canister.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, CandidType)]
pub enum Mode {
    /// Anyone may call update endpoints.
    #[default]
    GeneralAvailability,
    /// Only the listed principals may call update endpoints.
    RestrictedTo(BTreeSet<Principal>),
}

impl Mode {
    pub fn restricted_to<I: IntoIterator<Item = Principal>>(principals: I) -> Self {
        Self::RestrictedTo(principals.into_iter().collect())
    }
}
