//! Progress reporting contracts between the pipeline and its observers.

use serde::{Deserialize, Serialize};

use crate::CancelToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Preparing,
    Decoding,
    Upscaling,
    Encoding,
    Finalizing,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Preparing => "preparing",
            Stage::Decoding => "decoding",
            Stage::Upscaling => "upscaling",
            Stage::Encoding => "encoding",
            Stage::Finalizing => "finalizing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Progress {
    pub stage: Stage,
    /// Completion of the whole job, 0.0..=1.0.
    pub overall: f64,
    /// Units processed in the current stage (frames for video), when countable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
}

/// Observer of a running job. Implementations must be cheap: the pipeline may
/// call `progress` many times per second; throttling is the observer's concern.
pub trait Reporter: Send + Sync {
    fn progress(&self, progress: Progress);
    fn log(&self, level: LogLevel, message: &str);
}

/// A reporter that discards everything.
pub struct NullReporter;

impl Reporter for NullReporter {
    fn progress(&self, _: Progress) {}
    fn log(&self, _: LogLevel, _: &str) {}
}

/// Context handed to a single tool invocation (engine run, frame extraction).
/// `progress` receives the completion fraction of that invocation, 0.0..=1.0.
pub struct TaskContext<'a> {
    pub cancel: &'a CancelToken,
    pub progress: &'a dyn Fn(f64),
    pub log: &'a dyn Fn(LogLevel, &str),
}
