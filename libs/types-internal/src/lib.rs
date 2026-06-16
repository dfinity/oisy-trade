//! Internal types used by the OISY TRADE canister.
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

/// Argument passed to the OISY TRADE canister on init or upgrade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
pub enum OisyTradeArg {
    /// Argument used when the canister is first installed.
    Init(InitArg),
    /// Argument used when the canister is upgraded.
    Upgrade(Option<UpgradeArg>),
}

/// Conservative production default for [`InitArg::max_orders_per_chunk`].
/// Referenced by `ExecutionPolicy::default()` and every test/bench fixture
/// that needs the same value.
pub const DEFAULT_MAX_ORDERS_PER_CHUNK: u32 = 1_000;

/// Conservative production default for [`InitArg::instruction_budget`]:
/// 1B instructions per chunk, leaving generous headroom for event
/// serialization, settling, and stable-memory writes within the IC's
/// per-message instruction cap.
pub const DEFAULT_INSTRUCTION_BUDGET: u64 = 1_000_000_000;

/// Argument for canister initialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, CandidType)]
#[cfg_attr(feature = "event", derive(minicbor::Encode, minicbor::Decode))]
pub struct InitArg {
    /// Controls who may call update endpoints.
    #[cfg_attr(feature = "event", n(0))]
    pub mode: Mode,
    /// Maximum pending orders matched per chunked-execution chunk.
    #[cfg_attr(feature = "event", n(1))]
    pub max_orders_per_chunk: u32,
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
    pub max_orders_per_chunk: Option<u32>,
    /// If set, overrides the current `instruction_budget`.
    #[cfg_attr(feature = "event", n(2))]
    pub instruction_budget: Option<u64>,
}

/// Controls who may call update endpoints on the OISY TRADE canister.
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
