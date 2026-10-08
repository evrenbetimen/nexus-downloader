use thiserror::Error;

use crate::types::DownloadId;

/// Every failure the engine can surface to callers.
#[derive(Debug, Error)]
pub enum NexusError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("disk error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("could not encode request context: {0}")]
    Json(#[from] serde_json::Error),
    #[error("background task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("download {0} not found")]
    NotFound(DownloadId),
    #[error("server responded with HTTP {0}")]
    BadStatus(u16),
    #[error("server ignored the byte-range request")]
    RangeNotSupported,
    #[error("connection closed before the segment finished")]
    Truncated,
    #[error("invalid header {0:?}")]
    InvalidHeader(String),
    #[error("unsupported URL scheme {0:?}; only http and https can be downloaded")]
    UnsupportedScheme(String),
    #[error("download was cancelled")]
    Cancelled,
}

impl NexusError {
    /// Whether retrying the same request may succeed (network blips, 5xx, throttling).
    #[must_use]
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Http(e) => !e.is_builder() && !e.is_redirect(),
            Self::BadStatus(code) => *code == 408 || *code == 429 || *code >= 500,
            Self::Truncated => true,
            _ => false,
        }
    }
}

pub type Result<T, E = NexusError> = std::result::Result<T, E>;
