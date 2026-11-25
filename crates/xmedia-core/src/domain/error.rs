use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum DomainError {
    #[error("validation error: {0}")]
    Validation(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("too many requests: {0}")]
    TooManyRequests(String),

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("requested range not satisfiable (total_size={total_size})")]
    RangeNotSatisfiable { total_size: u64 },

    #[error("storage error: {0}")]
    Storage(String),

    #[error("persistence error: {0}")]
    Persistence(String),
}

pub type DomainResult<T> = Result<T, DomainError>;

impl From<xmedia_formats::FormatError> for DomainError {
    fn from(err: xmedia_formats::FormatError) -> Self {
        match err {
            xmedia_formats::FormatError::Validation(msg) => Self::Validation(msg),
            xmedia_formats::FormatError::UnsupportedFormat(msg) => Self::UnsupportedFormat(msg),
            xmedia_formats::FormatError::NotFound(msg) => Self::NotFound(msg),
            xmedia_formats::FormatError::Conflict(msg) => Self::Conflict(msg),
            xmedia_formats::FormatError::Io(msg) => Self::Storage(msg),
        }
    }
}
