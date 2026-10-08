use std::path::PathBuf;
use std::time::Duration;

/// Hard ceiling on parallel connections for a single download.
pub const MAX_CONNECTIONS: usize = 32;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// SQLite file holding downloads and byte maps; in-memory when `None`.
    pub database_path: Option<PathBuf>,
    /// Where downloads go when a request names no directory.
    pub download_dir: PathBuf,
    /// Upper bound for the dynamic connection count (clamped to [`MAX_CONNECTIONS`]).
    pub max_connections_per_download: usize,
    /// Connections opened right away before throughput-based ramp-up kicks in.
    pub initial_connections: usize,
    /// A segment is only split if both halves are at least this large.
    pub min_segment_size: u64,
    /// Bytes buffered per connection before each positional write.
    pub write_buffer_size: usize,
    /// How often the controller considers opening another connection.
    pub ramp_interval: Duration,
    /// How often the byte map is fsynced and saved while downloading.
    pub checkpoint_interval: Duration,
    /// Retries per segment on transient network errors.
    pub max_retries: u32,
    pub user_agent: String,
    /// Global bandwidth cap in bytes per second.
    pub speed_limit: Option<u64>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            database_path: None,
            download_dir: std::env::temp_dir(),
            max_connections_per_download: MAX_CONNECTIONS,
            initial_connections: 4,
            min_segment_size: 1024 * 1024,
            write_buffer_size: 256 * 1024,
            ramp_interval: Duration::from_millis(750),
            checkpoint_interval: Duration::from_secs(1),
            max_retries: 8,
            user_agent: concat!("NexusDL/", env!("CARGO_PKG_VERSION")).to_owned(),
            speed_limit: None,
        }
    }
}

impl EngineConfig {
    pub(crate) fn max_connections(&self) -> usize {
        self.max_connections_per_download.clamp(1, MAX_CONNECTIONS)
    }
}
