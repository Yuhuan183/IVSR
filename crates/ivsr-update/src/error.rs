#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("update source is not configured")]
    NotConfigured,

    #[error("HTTP {status} from {url}")]
    Http { status: u16, url: String },

    #[error("network error: {0}")]
    Network(String),

    #[error("unexpected response: {0}")]
    Parse(String),

    #[error("no release found{0}")]
    NoRelease(String),

    #[error("no asset in release {release} matches this platform ({platform})")]
    NoAsset { release: String, platform: String },

    #[error("checksum mismatch for {file}: expected {expected}, got {actual}")]
    Checksum { file: String, expected: String, actual: String },

    #[error("size mismatch for {file}: expected {expected} bytes, got {actual}")]
    Size { file: String, expected: u64, actual: u64 },

    #[error("archive error: {0}")]
    Archive(String),

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("cancelled")]
    Cancelled,
}

impl Error {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io { context: context.into(), source }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
