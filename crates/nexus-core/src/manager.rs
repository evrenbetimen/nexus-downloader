//! [`DownloadManager`]: owns every download, its task and its persisted state.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use reqwest::header::{self, HeaderMap};
use reqwest::{Client, StatusCode};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::config::EngineConfig;
use crate::error::{NexusError, Result};
use crate::http;
use crate::limiter::BandwidthLimiter;
use crate::segment::SegmentTable;
use crate::speed::{RampController, SpeedMeter};
use crate::store::{DownloadRecord, Store};
use crate::types::{
    DownloadId, DownloadRequest, DownloadSnapshot, DownloadStatus, FileCategory, ManagerEvent,
    ProgressSample, RequestContext,
};

const PART_SUFFIX: &str = ".nxpart";

/// Thread-safe handle to the download engine. Cloning is cheap.
#[derive(Debug, Clone)]
pub struct DownloadManager {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    config: EngineConfig,
    client: Client,
    store: Arc<Store>,
    limiter: Arc<BandwidthLimiter>,
    downloads: RwLock<HashMap<DownloadId, Arc<Entry>>>,
    events: broadcast::Sender<ManagerEvent>,
    runtime: Handle,
    /// Downloads that were running when the previous session ended.
    interrupted: Mutex<Vec<DownloadId>>,
    shutting_down: AtomicBool,
}

#[derive(Debug)]
struct Meta {
    file_name: String,
    directory: PathBuf,
    final_url: Option<String>,
    total_bytes: Option<u64>,
    resumable: bool,
    status: DownloadStatus,
    error: Option<String>,
    /// The file name is final (user-chosen, or already resolved by a probe).
    name_fixed: bool,
}

#[derive(Debug)]
struct Running {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

#[derive(Debug)]
struct Entry {
    id: DownloadId,
    url: String,
    context: RequestContext,
    created_at: i64,
    meta: Mutex<Meta>,
    /// Byte map of a resumable download; `None` until probed or when the
    /// server doesn't support ranges.
    table: Mutex<Option<SegmentTable>>,
    /// Progress of a single-stream (non-resumable) download.
    streamed: AtomicU64,
    connections: AtomicU32,
    speed: Mutex<SpeedMeter>,
    task: Mutex<Option<Running>>,
    removed: AtomicBool,
}

impl Entry {
    fn meta(&self) -> std::sync::MutexGuard<'_, Meta> {
        self.meta.lock().expect("meta poisoned")
    }

    fn table(&self) -> std::sync::MutexGuard<'_, Option<SegmentTable>> {
        self.table.lock().expect("segment table poisoned")
    }

    /// Runs `f` on the byte map. Only called while segmented workers run.
    fn with_table<R>(&self, f: impl FnOnce(&mut SegmentTable) -> R) -> R {
        f(self.table().as_mut().expect("segment table initialised"))
    }

    fn status(&self) -> DownloadStatus {
        self.meta().status
    }

    fn downloaded(&self) -> u64 {
        self.table().as_ref().map_or_else(
            || self.streamed.load(Ordering::Relaxed),
            SegmentTable::flushed_bytes,
        )
    }

    fn final_path(&self) -> PathBuf {
        let meta = self.meta();
        meta.directory.join(&meta.file_name)
    }

    fn part_path(&self) -> PathBuf {
        let meta = self.meta();
        meta.directory
            .join(format!("{}{PART_SUFFIX}", meta.file_name))
    }

    fn record(&self) -> DownloadRecord {
        let meta = self.meta();
        DownloadRecord {
            id: self.id,
            url: self.url.clone(),
            final_url: meta.final_url.clone(),
            file_name: meta.file_name.clone(),
            directory: meta.directory.clone(),
            total_bytes: meta.total_bytes,
            resumable: meta.resumable,
            status: meta.status,
            error: meta.error.clone(),
            context: self.context.clone(),
            created_at: self.created_at,
        }
    }

    fn snapshot(&self) -> DownloadSnapshot {
        let downloaded_bytes = self.downloaded();
        let meta = self.meta();
        DownloadSnapshot {
            id: self.id,
            url: self.url.clone(),
            file_name: meta.file_name.clone(),
            path: meta.directory.join(&meta.file_name),
            category: FileCategory::from_file_name(&meta.file_name),
            status: meta.status,
            total_bytes: meta.total_bytes,
            downloaded_bytes,
            resumable: meta.resumable,
            error: meta.error.clone(),
            created_at: self.created_at,
        }
    }

    fn sample(&self, now: Instant) -> ProgressSample {
        let (downloaded_bytes, segments) = match self.table().as_ref() {
            Some(t) => (t.flushed_bytes(), t.flushed_states()),
            None => (self.streamed.load(Ordering::Relaxed), Vec::new()),
        };
        let speed_bps = self
            .speed
            .lock()
            .expect("speed meter poisoned")
            .record(now, downloaded_bytes);
        let meta = self.meta();
        let eta_secs = meta
            .total_bytes
            .filter(|_| speed_bps > 0)
            .map(|total| total.saturating_sub(downloaded_bytes).div_ceil(speed_bps));
        ProgressSample {
            id: self.id,
            status: meta.status,
            total_bytes: meta.total_bytes,
            downloaded_bytes,
            speed_bps,
            eta_secs,
            connections: self.connections.load(Ordering::Relaxed),
            segments,
        }
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl DownloadManager {
    /// Opens the database and restores every known download. Downloads that
    /// were running when the last session ended come back paused; call
    /// [`Self::resume_interrupted`] to restart them.
    ///
    /// Must be called from within a Tokio runtime, which runs all transfers.
    pub fn new(config: EngineConfig) -> Result<Self> {
        let store = match &config.database_path {
            Some(path) => Store::open(path)?,
            None => Store::open_in_memory()?,
        };
        let client = http::build_client(&config)?;
        let limiter = BandwidthLimiter::new(config.speed_limit);
        let mut downloads = HashMap::new();
        let mut interrupted = Vec::new();
        for record in store.load_all()? {
            let segments = store.load_segments(record.id)?;
            let mut status = record.status;
            if status.is_active() {
                interrupted.push(record.id);
                status = DownloadStatus::Paused;
            }
            let table = (record.resumable && !segments.is_empty())
                .then(|| SegmentTable::from_states(segments, config.min_segment_size));
            let entry = Entry {
                id: record.id,
                url: record.url,
                context: record.context,
                created_at: record.created_at,
                meta: Mutex::new(Meta {
                    file_name: record.file_name,
                    directory: record.directory,
                    final_url: record.final_url,
                    total_bytes: record.total_bytes,
                    resumable: record.resumable,
                    status,
                    error: record.error,
                    name_fixed: true,
                }),
                table: Mutex::new(table),
                streamed: AtomicU64::new(0),
                connections: AtomicU32::new(0),
                speed: Mutex::default(),
                task: Mutex::new(None),
                removed: AtomicBool::new(false),
            };
            downloads.insert(record.id, Arc::new(entry));
        }
        let (events, _) = broadcast::channel(1024);
        Ok(Self {
            inner: Arc::new(Inner {
                config,
                client,
                store: Arc::new(store),
                limiter: Arc::new(limiter),
                downloads: RwLock::new(downloads),
                events,
                runtime: Handle::current(),
                interrupted: Mutex::new(interrupted),
                shutting_down: AtomicBool::new(false),
            }),
        })
    }

    /// Lifecycle events (added, status changes, removed).
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ManagerEvent> {
        self.inner.events.subscribe()
    }

    #[must_use]
    pub fn list(&self) -> Vec<DownloadSnapshot> {
        let mut all: Vec<_> = self.inner.read().values().map(|e| e.snapshot()).collect();
        all.sort_by_key(|d| d.created_at);
        all
    }

    pub fn get(&self, id: DownloadId) -> Result<DownloadSnapshot> {
        Ok(self.inner.entry(id)?.snapshot())
    }

    /// Live progress of every running download. Cheap enough to poll at 60 Hz.
    #[must_use]
    pub fn progress(&self) -> Vec<ProgressSample> {
        let now = Instant::now();
        self.inner
            .read()
            .values()
            .filter(|e| e.status().is_active())
            .map(|e| e.sample(now))
            .collect()
    }

    /// Queues a new download and starts it immediately.
    pub async fn add(&self, request: DownloadRequest) -> Result<DownloadSnapshot> {
        let url = http::validate_url(&request.url)?;
        // Fail fast on headers the server would never see correctly.
        http::context_headers(&request.context)?;
        let user_named = request
            .file_name
            .as_deref()
            .map(http::sanitize_file_name)
            .filter(|n| !n.is_empty());
        let file_name = user_named
            .clone()
            .or_else(|| http::file_name_from_url(url.as_str()))
            .unwrap_or_else(|| "download".to_owned());
        let entry = Arc::new(Entry {
            id: DownloadId::new(),
            url: url.to_string(),
            context: request.context,
            created_at: now_millis(),
            meta: Mutex::new(Meta {
                file_name,
                directory: request
                    .directory
                    .unwrap_or_else(|| self.inner.config.download_dir.clone()),
                final_url: None,
                total_bytes: None,
                resumable: false,
                status: DownloadStatus::Queued,
                error: None,
                name_fixed: user_named.is_some(),
            }),
            table: Mutex::new(None),
            streamed: AtomicU64::new(0),
            connections: AtomicU32::new(0),
            speed: Mutex::default(),
            task: Mutex::new(None),
            removed: AtomicBool::new(false),
        });
        self.inner.persist(&entry).await?;
        self.inner
            .downloads
            .write()
            .expect("downloads poisoned")
            .insert(entry.id, entry.clone());
        let snapshot = entry.snapshot();
        let _ = self.inner.events.send(ManagerEvent::Added {
            download: snapshot.clone(),
        });
        self.inner.start(&entry);
        Ok(snapshot)
    }

    /// Stops a download, keeping its byte map so it can resume later.
    /// Returns once the final state is on disk.
    pub async fn pause(&self, id: DownloadId) -> Result<()> {
        let entry = self.inner.entry(id)?;
        self.inner.stop(&entry).await;
        Ok(())
    }

    pub fn resume(&self, id: DownloadId) -> Result<()> {
        let entry = self.inner.entry(id)?;
        if entry.status() != DownloadStatus::Completed {
            self.inner.start(&entry);
        }
        Ok(())
    }

    /// Restarts downloads that were running when the previous session ended.
    pub fn resume_interrupted(&self) {
        let ids = std::mem::take(&mut *self.inner.interrupted.lock().expect("poisoned"));
        for id in ids {
            if let Err(e) = self.resume(id) {
                warn!(%id, "could not resume interrupted download: {e}");
            }
        }
    }

    /// Forgets a download, optionally deleting what was downloaded.
    pub async fn remove(&self, id: DownloadId, delete_files: bool) -> Result<()> {
        let entry = self.inner.entry(id)?;
        self.inner.stop(&entry).await;
        entry.removed.store(true, Ordering::Release);
        self.inner
            .downloads
            .write()
            .expect("downloads poisoned")
            .remove(&id);
        let store = self.inner.store.clone();
        let files = delete_files.then(|| [entry.part_path(), entry.final_path()]);
        tokio::task::spawn_blocking(move || -> Result<()> {
            store.delete(id)?;
            for path in files.into_iter().flatten() {
                match std::fs::remove_file(&path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                    _ => {}
                }
            }
            Ok(())
        })
        .await??;
        let _ = self.inner.events.send(ManagerEvent::Removed { id });
        Ok(())
    }

    /// Global bandwidth cap in bytes per second (`None` = unlimited). Applies
    /// to running connections from their next chunk on.
    pub fn set_speed_limit(&self, bytes_per_sec: Option<u64>) {
        self.inner.limiter.set_limit(bytes_per_sec);
    }

    #[must_use]
    pub fn speed_limit(&self) -> Option<u64> {
        self.inner.limiter.limit()
    }

    /// Stops every transfer and saves its state. Running downloads are
    /// recorded as interrupted so the next session picks them up again.
    pub async fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::Release);
        let entries: Vec<_> = self.inner.read().values().cloned().collect();
        for entry in entries {
            self.inner.stop(&entry).await;
        }
    }
}

impl Inner {
    fn read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<DownloadId, Arc<Entry>>> {
        self.downloads.read().expect("downloads poisoned")
    }

    fn entry(&self, id: DownloadId) -> Result<Arc<Entry>> {
        self.read()
            .get(&id)
            .cloned()
            .ok_or(NexusError::NotFound(id))
    }

    fn start(self: &Arc<Self>, entry: &Arc<Entry>) {
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let mut task = entry.task.lock().expect("task poisoned");
        if task.as_ref().is_some_and(|t| !t.handle.is_finished()) {
            return;
        }
        {
            let mut meta = entry.meta();
            meta.status = DownloadStatus::Queued;
            meta.error = None;
        }
        let cancel = CancellationToken::new();
        let handle = self
            .runtime
            .spawn(run(self.clone(), entry.clone(), cancel.clone()));
        *task = Some(Running { cancel, handle });
    }

    async fn stop(&self, entry: &Entry) {
        let running = entry.task.lock().expect("task poisoned").take();
        if let Some(running) = running {
            running.cancel.cancel();
            if let Err(e) = running.handle.await {
                warn!(id = %entry.id, "download task ended abnormally: {e}");
            }
        }
    }

    /// Saves the download row and, for resumable downloads, its byte map.
    /// The data file is fsynced first so the saved map never claims bytes
    /// that a power cut could still lose.
    async fn persist(&self, entry: &Arc<Entry>) -> Result<()> {
        if entry.removed.load(Ordering::Acquire) {
            return Ok(());
        }
        let record = entry.record();
        let segments = entry.table().as_ref().map(SegmentTable::flushed_states);
        let part = entry.part_path();
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            if let Some(segments) = segments {
                match std::fs::OpenOptions::new().write(true).open(&part) {
                    Ok(file) => file.sync_data()?,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                store.upsert(&record)?;
                store.save_segments(record.id, &segments)?;
            } else {
                store.upsert(&record)?;
            }
            Ok(())
        })
        .await?
    }

    async fn set_status(&self, entry: &Arc<Entry>, status: DownloadStatus, error: Option<String>) {
        {
            let mut meta = entry.meta();
            meta.status = status;
            meta.error = error;
        }
        if let Err(e) = self.persist(entry).await {
            warn!(id = %entry.id, "failed to persist download state: {e}");
        }
        if !entry.removed.load(Ordering::Acquire) {
            let _ = self.events.send(ManagerEvent::Updated {
                download: entry.snapshot(),
            });
        }
    }
}

async fn run(inner: Arc<Inner>, entry: Arc<Entry>, cancel: CancellationToken) {
    entry.speed.lock().expect("speed meter poisoned").reset();
    let outcome = tokio::select! {
        r = drive(&inner, &entry) => r,
        () = cancel.cancelled() => Err(NexusError::Cancelled),
    };
    entry.connections.store(0, Ordering::Relaxed);
    let (status, error) = match outcome {
        Ok(()) => (DownloadStatus::Completed, None),
        Err(NexusError::Cancelled) if inner.shutting_down.load(Ordering::Acquire) => {
            (DownloadStatus::Queued, None)
        }
        Err(NexusError::Cancelled) => (DownloadStatus::Paused, None),
        Err(e) => {
            warn!(id = %entry.id, "download failed: {e}");
            (DownloadStatus::Failed, Some(e.to_string()))
        }
    };
    inner.set_status(&entry, status, error).await;
}

async fn drive(inner: &Arc<Inner>, entry: &Arc<Entry>) -> Result<()> {
    let config = &inner.config;
    inner
        .set_status(entry, DownloadStatus::Connecting, None)
        .await;
    let headers = Arc::new(http::context_headers(&entry.context)?);

    // Drop ownership marks and in-flight bytes left by a previous run.
    let has_map = {
        let mut table = entry.table();
        if let Some(t) = table.as_mut() {
            *t = SegmentTable::from_states(t.flushed_states(), config.min_segment_size);
        }
        table.is_some()
    };
    let has_part = tokio::fs::try_exists(entry.part_path())
        .await
        .unwrap_or(false);
    if !has_map || !has_part {
        let probe = http::probe(&inner.client, &entry.url, &headers).await?;
        debug!(id = %entry.id, ?probe, "probed");
        let mut meta = entry.meta();
        meta.final_url = Some(probe.final_url);
        meta.total_bytes = probe.total_bytes;
        meta.resumable = probe.resumable;
        if !meta.name_fixed {
            if let Some(name) = probe.file_name {
                meta.file_name = name;
            }
            meta.file_name = unique_file_name(&meta.directory, &meta.file_name);
            meta.name_fixed = true;
        }
        drop(meta);
        *entry.table() = None;
    }

    let (url, total, resumable, directory) = {
        let meta = entry.meta();
        (
            Arc::<str>::from(meta.final_url.as_deref().unwrap_or(&entry.url)),
            meta.total_bytes,
            meta.resumable,
            meta.directory.clone(),
        )
    };
    tokio::fs::create_dir_all(&directory).await?;
    let part = entry.part_path();

    if let Some(total) = total.filter(|_| resumable) {
        if entry.table().is_none() {
            preallocate(&part, total).await?;
            *entry.table() = Some(SegmentTable::new(total, config.min_segment_size));
        }
        inner
            .set_status(entry, DownloadStatus::Downloading, None)
            .await;
        segmented(inner, entry, url, headers, part.clone()).await?;
    } else {
        inner
            .set_status(entry, DownloadStatus::Downloading, None)
            .await;
        streamed(inner, entry, &url, &headers, &part, total).await?;
    }

    let final_path = entry.final_path();
    tokio::fs::rename(&part, &final_path).await?;
    Ok(())
}

/// Reserves the full file size up front (fallocate on Linux, `F_PREALLOCATE`
/// on macOS, `SetFileInformationByHandle` on Windows) so the file is laid out
/// contiguously and a full disk fails now rather than at 97 %.
async fn preallocate(path: &Path, len: u64) -> Result<()> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)?;
        if let Err(e) = fs2::FileExt::allocate(&file, len) {
            debug!("allocate unsupported here ({e}); falling back to a sparse file");
        }
        file.set_len(len)?;
        Ok(())
    })
    .await?
}

/// `name.ext` → `name (1).ext` when the file (or its partial download) exists.
fn unique_file_name(dir: &Path, name: &str) -> String {
    let taken = |n: &str| dir.join(n).exists() || dir.join(format!("{n}{PART_SUFFIX}")).exists();
    if !taken(name) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    let mut i = 1u64;
    loop {
        let candidate = format!("{stem} ({i}){ext}");
        if !taken(&candidate) {
            return candidate;
        }
        i += 1;
    }
}

#[derive(Debug, Clone)]
struct WorkerCtx {
    entry: Arc<Entry>,
    client: Client,
    limiter: Arc<BandwidthLimiter>,
    url: Arc<str>,
    headers: Arc<HeaderMap>,
    part: PathBuf,
    buffer_size: usize,
    max_retries: u32,
}

/// Runs the connection pool for one download until every byte is on disk.
async fn segmented(
    inner: &Arc<Inner>,
    entry: &Arc<Entry>,
    url: Arc<str>,
    headers: Arc<HeaderMap>,
    part: PathBuf,
) -> Result<()> {
    let config = &inner.config;
    let ctx = WorkerCtx {
        entry: entry.clone(),
        client: inner.client.clone(),
        limiter: inner.limiter.clone(),
        url,
        headers,
        part,
        buffer_size: config.write_buffer_size.max(16 * 1024),
        max_retries: config.max_retries,
    };
    let max = config.max_connections();
    // Dropping the set (e.g. on pause) aborts every worker; in-flight writes
    // are excluded from the saved byte map so that is always safe.
    let mut workers = JoinSet::new();
    for _ in 0..config.initial_connections.clamp(1, max) {
        if !entry.with_table(|t| t.can_accept_worker()) {
            break;
        }
        workers.spawn(worker(ctx.clone()));
    }

    let mut ramp = RampController::default();
    let mut ramp_tick = tokio::time::interval(config.ramp_interval);
    ramp_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    ramp_tick.tick().await;
    let mut checkpoint = tokio::time::interval(config.checkpoint_interval);
    checkpoint.set_missed_tick_behavior(MissedTickBehavior::Skip);
    checkpoint.tick().await;
    let mut last = (Instant::now(), entry.downloaded());

    while !workers.is_empty() {
        tokio::select! {
            Some(joined) = workers.join_next() => joined??,
            _ = ramp_tick.tick() => {
                let now = (Instant::now(), entry.downloaded());
                let rate = now.1.saturating_sub(last.1) as f64
                    / now.0.duration_since(last.0).as_secs_f64().max(1e-3);
                last = now;
                if workers.len() < max
                    && entry.with_table(|t| t.can_accept_worker())
                    && ramp.should_add(rate)
                {
                    workers.spawn(worker(ctx.clone()));
                }
            }
            _ = checkpoint.tick() => {
                if let Err(e) = inner.persist(entry).await {
                    warn!(id = %entry.id, "checkpoint failed: {e}");
                }
            }
        }
    }

    if entry.with_table(|t| t.is_complete()) {
        Ok(())
    } else {
        Err(NexusError::Truncated)
    }
}

/// One connection: keeps fetching segments (stealing work by splitting the
/// largest remaining one) until nothing worth splitting is left.
async fn worker(ctx: WorkerCtx) -> Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&ctx.part)
        .await?;
    while let Some(idx) = ctx.entry.with_table(SegmentTable::acquire) {
        ctx.entry.connections.fetch_add(1, Ordering::Relaxed);
        let result = fetch_with_retry(&ctx, &mut file, idx).await;
        ctx.entry.connections.fetch_sub(1, Ordering::Relaxed);
        ctx.entry.with_table(|t| t.release(idx));
        result?;
    }
    Ok(())
}

async fn fetch_with_retry(ctx: &WorkerCtx, file: &mut tokio::fs::File, idx: usize) -> Result<()> {
    let mut attempt = 0;
    loop {
        let before = ctx.entry.with_table(|t| t.cursor(idx));
        match fetch_segment(ctx, file, idx).await {
            Ok(()) => return Ok(()),
            Err(e) if e.is_transient() && attempt < ctx.max_retries => {
                if ctx.entry.with_table(|t| t.cursor(idx)) > before {
                    attempt = 0;
                }
                attempt += 1;
                let backoff =
                    Duration::from_millis(250 << attempt.min(6)).min(Duration::from_secs(10));
                debug!(id = %ctx.entry.id, idx, attempt, "segment error, retrying in {backoff:?}: {e}");
                tokio::time::sleep(backoff).await;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn fetch_segment(ctx: &WorkerCtx, file: &mut tokio::fs::File, idx: usize) -> Result<()> {
    let (start, end) = ctx.entry.with_table(|t| (t.cursor(idx), t.end(idx)));
    if start >= end {
        return Ok(());
    }
    let resp = http::get(&ctx.client, &ctx.url, &ctx.headers)
        .header(header::RANGE, format!("bytes={start}-{}", end - 1))
        .send()
        .await?;
    match resp.status() {
        StatusCode::PARTIAL_CONTENT => {}
        StatusCode::OK => return Err(NexusError::RangeNotSupported),
        status => return Err(NexusError::BadStatus(status.as_u16())),
    }
    file.seek(SeekFrom::Start(start)).await?;

    let mut buf = Vec::with_capacity(ctx.buffer_size);
    let mut body = resp.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        ctx.limiter.acquire(chunk.len()).await;
        buf.extend_from_slice(&chunk);
        // Another connection may have stolen our tail meanwhile.
        let reached_end = ctx
            .entry
            .with_table(|t| t.cursor(idx) + buf.len() as u64 >= t.end(idx));
        if buf.len() >= ctx.buffer_size || reached_end {
            flush(ctx, file, idx, &mut buf).await?;
            if reached_end {
                return Ok(());
            }
        }
    }
    flush(ctx, file, idx, &mut buf).await?;
    if ctx.entry.with_table(|t| t.cursor(idx) < t.end(idx)) {
        return Err(NexusError::Truncated);
    }
    Ok(())
}

async fn flush(
    ctx: &WorkerCtx,
    file: &mut tokio::fs::File,
    idx: usize,
    buf: &mut Vec<u8>,
) -> Result<()> {
    if buf.is_empty() {
        return Ok(());
    }
    let accepted = ctx.entry.with_table(|t| t.reserve(idx, buf.len() as u64));
    let written = async {
        file.write_all(&buf[..accepted as usize]).await?;
        file.flush().await
    }
    .await;
    buf.clear();
    match written {
        Ok(()) => {
            ctx.entry.with_table(|t| t.commit(idx, accepted));
            Ok(())
        }
        Err(e) => {
            ctx.entry.with_table(|t| t.rollback(idx, accepted));
            Err(e.into())
        }
    }
}

/// Fallback for servers without range support: one connection, restarted
/// from zero on resume.
async fn streamed(
    inner: &Inner,
    entry: &Entry,
    url: &str,
    headers: &HeaderMap,
    part: &Path,
    total: Option<u64>,
) -> Result<()> {
    let resp = http::get(&inner.client, url, headers).send().await?;
    if !resp.status().is_success() {
        return Err(NexusError::BadStatus(resp.status().as_u16()));
    }
    let mut file = tokio::fs::File::create(part).await?;
    entry.streamed.store(0, Ordering::Relaxed);
    entry.connections.store(1, Ordering::Relaxed);
    let buffer_size = inner.config.write_buffer_size.max(16 * 1024);
    let mut buf = Vec::with_capacity(buffer_size);
    let mut body = resp.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        inner.limiter.acquire(chunk.len()).await;
        buf.extend_from_slice(&chunk);
        if buf.len() >= buffer_size {
            file.write_all(&buf).await?;
            entry
                .streamed
                .fetch_add(buf.len() as u64, Ordering::Relaxed);
            buf.clear();
        }
    }
    file.write_all(&buf).await?;
    entry
        .streamed
        .fetch_add(buf.len() as u64, Ordering::Relaxed);
    file.flush().await?;
    file.sync_data().await?;
    if total.is_some_and(|t| t != entry.streamed.load(Ordering::Relaxed)) {
        return Err(NexusError::Truncated);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_unique_names() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unique_file_name(dir.path(), "a.zip"), "a.zip");
        std::fs::write(dir.path().join("a.zip"), b"").unwrap();
        std::fs::write(dir.path().join("a (1).zip.nxpart"), b"").unwrap();
        assert_eq!(unique_file_name(dir.path(), "a.zip"), "a (2).zip");
        std::fs::write(dir.path().join("README"), b"").unwrap();
        assert_eq!(unique_file_name(dir.path(), "README"), "README (1)");
    }
}
