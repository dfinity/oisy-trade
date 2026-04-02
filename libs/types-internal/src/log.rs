//! Log types used by the DEX canister.

use canlog::{GetLogFilter, LogFilter, LogPriorityLevels};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Formatter;
use std::str::FromStr;

/// The priority level of a log entry.
#[derive(LogPriorityLevels, Serialize, Deserialize, PartialEq, Debug, Copy, Clone)]
pub enum Priority {
    /// Informational log entries.
    #[log_level(capacity = 1000, name = "INFO")]
    Info,
    /// Debug log entries.
    #[log_level(capacity = 1000, name = "DEBUG")]
    Debug,
}

impl GetLogFilter for Priority {
    fn get_log_filter() -> LogFilter {
        LogFilter::ShowAll
    }
}

impl FromStr for Priority {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "info" => Ok(Priority::Info),
            "debug" => Ok(Priority::Debug),
            _ => Err(format!(
                "unrecognized priority '{s}'; expected one of: info | debug"
            )),
        }
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Priority::Info => write!(f, "INFO"),
            Priority::Debug => write!(f, "DEBUG"),
        }
    }
}
