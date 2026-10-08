use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier of a download, shared with the UI and the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadId(pub Uuid);

impl DownloadId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DownloadId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for DownloadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Connecting,
    Downloading,
    Paused,
    Completed,
    Failed,
}

impl DownloadStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Connecting => "connecting",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => Self::Queued,
            "connecting" => Self::Connecting,
            "downloading" => Self::Downloading,
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            _ => return None,
        })
    }

    /// A task is currently running (or about to) for this download.
    #[must_use]
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Connecting | Self::Downloading)
    }
}

/// Sidebar categories, derived from the file extension like IDM does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileCategory {
    Compressed,
    Documents,
    Music,
    Programs,
    Videos,
    Other,
}

impl FileCategory {
    #[must_use]
    pub fn from_file_name(name: &str) -> Self {
        let ext = name
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" => {
                Self::Compressed
            }
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "odt"
            | "epub" | "csv" | "md" => Self::Documents,
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "opus" | "m4a" | "wma" => Self::Music,
            "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "apk" | "msix" => {
                Self::Programs
            }
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "wmv" | "flv" | "m4v" | "ts" | "m3u8" => {
                Self::Videos
            }
            _ => Self::Other,
        }
    }
}

/// Browser session state cloned from the extension so authenticated and
/// CDN-protected URLs download with the same identity the page used.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestContext {
    /// Raw `Cookie` header value (`name=value; name2=value2`).
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    /// Any additional headers (e.g. `Authorization`, `Origin`).
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub url: String,
    /// Target directory; the manager's default download directory when `None`.
    #[serde(default)]
    pub directory: Option<PathBuf>,
    /// Overrides the name derived from `Content-Disposition` or the URL.
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub context: RequestContext,
}

/// One contiguous byte range `[start, end)` of the target file and how much of
/// it has been flushed to disk. The full list is the download's byte map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentState {
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

impl SegmentState {
    #[must_use]
    pub fn cursor(&self) -> u64 {
        self.start + self.downloaded
    }

    #[must_use]
    pub fn remaining(&self) -> u64 {
        self.end.saturating_sub(self.cursor())
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.cursor() >= self.end
    }
}

/// Full description of a download, sent to the UI on load and on add.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSnapshot {
    pub id: DownloadId,
    pub url: String,
    pub file_name: String,
    pub path: PathBuf,
    pub category: FileCategory,
    pub status: DownloadStatus,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub resumable: bool,
    pub error: Option<String>,
    /// Unix epoch milliseconds.
    pub created_at: i64,
}

/// High-frequency progress for one running download (sent ~60 times a second).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressSample {
    pub id: DownloadId,
    pub status: DownloadStatus,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    /// Smoothed throughput in bytes per second.
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
    pub connections: u32,
    pub segments: Vec<SegmentState>,
}

/// Low-frequency lifecycle notifications.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ManagerEvent {
    Added { download: DownloadSnapshot },
    Updated { download: DownloadSnapshot },
    Removed { id: DownloadId },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categorises_by_extension() {
        assert_eq!(
            FileCategory::from_file_name("a.ZIP"),
            FileCategory::Compressed
        );
        assert_eq!(
            FileCategory::from_file_name("movie.final.mkv"),
            FileCategory::Videos
        );
        assert_eq!(
            FileCategory::from_file_name("setup.exe"),
            FileCategory::Programs
        );
        assert_eq!(FileCategory::from_file_name("README"), FileCategory::Other);
    }

    #[test]
    fn status_round_trips() {
        for s in [
            DownloadStatus::Queued,
            DownloadStatus::Connecting,
            DownloadStatus::Downloading,
            DownloadStatus::Paused,
            DownloadStatus::Completed,
            DownloadStatus::Failed,
        ] {
            assert_eq!(DownloadStatus::parse(s.as_str()), Some(s));
        }
    }
}
