#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] ivsr_core::Error),

    #[error(transparent)]
    Update(#[from] ivsr_update::Error),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("unknown engine `{0}`")]
    UnknownEngine(String),

    #[error("{0}")]
    Input(String),
}

impl Error {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Error::Core(ivsr_core::Error::Cancelled) | Error::Update(ivsr_update::Error::Cancelled))
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
