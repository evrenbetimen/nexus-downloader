//! SQLite persistence for downloads and their segment byte maps.
//!
//! The byte map is checkpointed while downloading (after an fsync of the data
//! file), so after a crash or power loss a download resumes from bytes that
//! are known to be on disk.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::error::Result;
use crate::types::{DownloadId, DownloadStatus, RequestContext, SegmentState};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS downloads (
    id           TEXT PRIMARY KEY,
    url          TEXT NOT NULL,
    final_url    TEXT,
    file_name    TEXT NOT NULL,
    directory    TEXT NOT NULL,
    total_bytes  INTEGER,
    resumable    INTEGER NOT NULL DEFAULT 0,
    status       TEXT NOT NULL,
    error        TEXT,
    context      TEXT NOT NULL,
    created_at   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS segments (
    download_id  TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    idx          INTEGER NOT NULL,
    start        INTEGER NOT NULL,
    end          INTEGER NOT NULL,
    downloaded   INTEGER NOT NULL,
    PRIMARY KEY (download_id, idx)
);
";

/// A download row as stored on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRecord {
    pub id: DownloadId,
    pub url: String,
    pub final_url: Option<String>,
    pub file_name: String,
    pub directory: PathBuf,
    pub total_bytes: Option<u64>,
    pub resumable: bool,
    pub status: DownloadStatus,
    pub error: Option<String>,
    pub context: RequestContext,
    pub created_at: i64,
}

#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("store poisoned")
    }

    pub fn upsert(&self, r: &DownloadRecord) -> Result<()> {
        let context = serde_json::to_string(&r.context)?;
        self.conn().execute(
            "INSERT INTO downloads
                (id, url, final_url, file_name, directory, total_bytes, resumable, status, error, context, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
                final_url = excluded.final_url,
                file_name = excluded.file_name,
                directory = excluded.directory,
                total_bytes = excluded.total_bytes,
                resumable = excluded.resumable,
                status = excluded.status,
                error = excluded.error,
                context = excluded.context",
            params![
                r.id.0.to_string(),
                r.url,
                r.final_url,
                r.file_name,
                r.directory.to_string_lossy(),
                r.total_bytes.map(|v| v as i64),
                r.resumable,
                r.status.as_str(),
                r.error,
                context,
                r.created_at,
            ],
        )?;
        Ok(())
    }

    /// Replaces the stored byte map of a download atomically.
    pub fn save_segments(&self, id: DownloadId, segments: &[SegmentState]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let key = id.0.to_string();
        tx.execute("DELETE FROM segments WHERE download_id = ?1", [&key])?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO segments (download_id, idx, start, end, downloaded) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (idx, s) in segments.iter().enumerate() {
                stmt.execute(params![
                    key,
                    idx as i64,
                    s.start as i64,
                    s.end as i64,
                    s.downloaded as i64
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete(&self, id: DownloadId) -> Result<()> {
        self.conn()
            .execute("DELETE FROM downloads WHERE id = ?1", [id.0.to_string()])?;
        Ok(())
    }

    pub fn load_segments(&self, id: DownloadId) -> Result<Vec<SegmentState>> {
        let conn = self.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT start, end, downloaded FROM segments WHERE download_id = ?1 ORDER BY idx",
        )?;
        let rows = stmt.query_map([id.0.to_string()], |row| {
            Ok(SegmentState {
                start: row.get::<_, i64>(0)? as u64,
                end: row.get::<_, i64>(1)? as u64,
                downloaded: row.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn load_all(&self) -> Result<Vec<DownloadRecord>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, url, final_url, file_name, directory, total_bytes, resumable, status, error, context, created_at
             FROM downloads ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let status: String = row.get(7)?;
            let context: String = row.get(9)?;
            Ok(DownloadRecord {
                id: DownloadId(Uuid::parse_str(&id).unwrap_or_default()),
                url: row.get(1)?,
                final_url: row.get(2)?,
                file_name: row.get(3)?,
                directory: PathBuf::from(row.get::<_, String>(4)?),
                total_bytes: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
                resumable: row.get(6)?,
                status: DownloadStatus::parse(&status).unwrap_or(DownloadStatus::Paused),
                error: row.get(8)?,
                context: serde_json::from_str(&context).unwrap_or_default(),
                created_at: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn exists(&self, id: DownloadId) -> Result<bool> {
        Ok(self
            .conn()
            .query_row(
                "SELECT 1 FROM downloads WHERE id = ?1",
                [id.0.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> DownloadRecord {
        DownloadRecord {
            id: DownloadId::new(),
            url: "https://example.com/a.zip".into(),
            final_url: None,
            file_name: "a.zip".into(),
            directory: PathBuf::from("/tmp"),
            total_bytes: Some(1000),
            resumable: true,
            status: DownloadStatus::Downloading,
            error: None,
            context: RequestContext {
                cookies: Some("sid=1".into()),
                ..RequestContext::default()
            },
            created_at: 42,
        }
    }

    #[test]
    fn round_trips_downloads_and_byte_map() {
        let store = Store::open_in_memory().unwrap();
        let mut r = record();
        store.upsert(&r).unwrap();
        r.status = DownloadStatus::Paused;
        store.upsert(&r).unwrap();
        let segs = vec![
            SegmentState {
                start: 0,
                end: 500,
                downloaded: 120,
            },
            SegmentState {
                start: 500,
                end: 1000,
                downloaded: 499,
            },
        ];
        store.save_segments(r.id, &segs).unwrap();

        assert_eq!(store.load_all().unwrap(), vec![r.clone()]);
        assert_eq!(store.load_segments(r.id).unwrap(), segs);

        store.delete(r.id).unwrap();
        assert!(!store.exists(r.id).unwrap());
        assert!(store.load_segments(r.id).unwrap().is_empty());
    }
}
