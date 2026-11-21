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
