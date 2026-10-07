use std::path::PathBuf;

/// Errors produced anywhere in the processing pipeline.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("operation cancelled")]
    Cancelled,

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("invalid parameter `{key}`: {reason}")]
    InvalidParam { key: String, reason: String },

    #[error("engine `{engine}` is not available: {reason}")]
    EngineUnavailable { engine: String, reason: String },

    #[error("model `{model}` not found for engine `{engine}`")]
    ModelNotFound { engine: String, model: String },

    #[error("{tool} failed: {message}")]
    Tool { tool: String, message: String },

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("{0}")]
    Invalid(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io { context: context.into(), source }
    }

    pub fn io_at(action: &str, path: &std::path::Path, source: std::io::Error) -> Self {
        Error::Io { context: format!("{action} {}", path.display()), source }
    }

    pub fn tool(tool: impl Into<String>, message: impl Into<String>) -> Self {
        Error::Tool { tool: tool.into(), message: message.into() }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Error::Cancelled)
    }
}

/// Extension to attach a path to `std::io::Result` values.
pub trait IoContext<T> {
    fn at(self, action: &str, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, action: &str, path: impl Into<PathBuf>) -> Result<T> {
        let path = path.into();
        self.map_err(|e| Error::io_at(action, &path, e))
    }
}
