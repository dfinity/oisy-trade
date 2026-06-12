//! Log types used by the OISY TRADE canister.

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
        if s.eq_ignore_ascii_case("info") {
            Ok(Priority::Info)
        } else if s.eq_ignore_ascii_case("debug") {
            Ok(Priority::Debug)
        } else {
            Err(format!(
                "unrecognized priority '{s}'; expected one of: info | debug"
            ))
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
