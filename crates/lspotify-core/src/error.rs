use std::time::Duration;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    Authentication,
    Authorization,
    InvalidInput,
    Network,
    NotFound,
    RateLimited,
    Restricted,
    Storage,
    Unavailable,
    Unexpected,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
    pub retry_after: Option<Duration>,
    pub correlation_id: Option<String>,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), retry_after: None, correlation_id: None }
    }

    pub fn rate_limited(message: impl Into<String>, retry_after: Duration) -> Self {
        Self {
            kind: ErrorKind::RateLimited,
            message: message.into(),
            retry_after: Some(retry_after),
            correlation_id: None,
        }
    }

    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }
}
