//! Internal types used by the DEX canister.
//!
//! Types in this module are unstable and breaking changes do not break the canister API.

#![forbid(unsafe_code)]

use candid::CandidType;
use serde::{Deserialize, Serialize};

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
pub struct InitArg {}

/// Argument for canister upgrade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub struct UpgradeArg {}
