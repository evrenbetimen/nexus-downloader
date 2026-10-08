//! NexusDL core engine.
//!
//! [`DownloadManager`] drives dynamic multi-segment HTTP downloads over a
//! shared connection pool, throttles them through a global token bucket and
//! checkpoints each download's byte map to SQLite so it survives crashes.

pub mod config;
pub mod error;
pub mod http;
pub mod limiter;
pub mod manager;
pub mod segment;
pub mod speed;
pub mod store;
pub mod types;

pub use config::{EngineConfig, MAX_CONNECTIONS};
pub use error::{NexusError, Result};
pub use manager::DownloadManager;
pub use types::*;
