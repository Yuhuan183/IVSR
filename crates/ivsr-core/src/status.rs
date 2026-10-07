use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Availability of an external runtime (engine binary, ffmpeg, ...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ToolStatus {
    Ready { location: PathBuf, version: Option<String> },
    Missing { hint: String },
    Broken { reason: String },
}

impl ToolStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, ToolStatus::Ready { .. })
    }

    /// Human-readable reason when not ready.
    pub fn problem(&self) -> Option<&str> {
        match self {
            ToolStatus::Ready { .. } => None,
            ToolStatus::Missing { hint } => Some(hint),
            ToolStatus::Broken { reason } => Some(reason),
        }
    }
}
