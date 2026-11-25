use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FormatError {
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    UnsupportedFormat(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for FormatError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

pub type FormatResult<T> = Result<T, FormatError>;
