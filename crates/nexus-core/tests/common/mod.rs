//! Test support: a small HTTP/1.1 file server with switchable misbehaviours.

#![allow(dead_code, clippy::too_many_lines)]

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use nexus_core::{DownloadId, DownloadManager, DownloadStatus, EngineConfig};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[derive(Debug, Clone, Default)]
pub struct Behaviour {
    /// Delay before each 64 KiB write.
    pub pace: Duration,
    /// Answer 403 unless the request carries `Cookie: session=ok`.
    pub require_cookie: bool,
    /// Ignore `Range` and always send the whole body with 200.
    pub ignore_range: bool,
    /// Answer this many requests with 503 before behaving.
    pub fail_first: usize,
    /// Cut this many ranged responses off after 100 KiB.
    pub drop_first: usize,
    /// Send no `Content-Disposition` header.
    pub no_disposition: bool,
}

#[derive(Debug, Clone)]
pub struct Server {
    pub addr: SocketAddr,
    pub requests: Arc<AtomicUsize>,
    pub range_requests: Arc<AtomicUsize>,
    pub max_concurrent: Arc<AtomicUsize>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}/{path}", self.addr)
    }
}

pub async fn serve(body: Arc<Vec<u8>>, behaviour: Behaviour) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = Server {
        addr: listener.local_addr().unwrap(),
        requests: Arc::default(),
        range_requests: Arc::default(),
        max_concurrent: Arc::default(),
    };
    let active = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(AtomicUsize::new(behaviour.fail_first));
    let drops = Arc::new(AtomicUsize::new(behaviour.drop_first));
    let s = server.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (body, b, s, active, failures, drops) = (
                body.clone(),
                behaviour.clone(),
                s.clone(),
                active.clone(),
                failures.clone(),
                drops.clone(),
            );
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                loop {
                    let mut range: Option<(usize, Option<usize>)> = None;
                    let mut cookie_ok = false;
                    let Ok(Some(_request_line)) = lines.next_line().await else {
                        return;
                    };
                    while let Ok(Some(line)) = lines.next_line().await {
                        if line.is_empty() {
                            break;
                        }
                        let lower = line.to_ascii_lowercase();
                        if let Some(v) = lower.strip_prefix("range: bytes=") {
                            let (a, z) = v.split_once('-').unwrap();
                            range = Some((a.parse().unwrap(), z.parse().ok()));
                        }
                        if lower == "cookie: session=ok" {
                            cookie_ok = true;
                        }
                    }
                    s.requests.fetch_add(1, Ordering::Relaxed);
                    let simple =
                        |status: &str| format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\n\r\n");
                    if b.require_cookie && !cookie_ok {
                        let _ = write.write_all(simple("403 Forbidden").as_bytes()).await;
                        continue;
                    }
                    if take_one(&failures) {
                        let _ = write
                            .write_all(simple("503 Service Unavailable").as_bytes())
                            .await;
                        continue;
                    }
                    let range = range.filter(|_| !b.ignore_range);
                    let mut cut_short = false;
                    let (status, slice, extra) = match range {
                        Some((a, _)) if a >= body.len() => {
                            let head = format!(
                                "HTTP/1.1 416 Range Not Satisfiable\r\ncontent-range: bytes */{}\r\ncontent-length: 0\r\n\r\n",
                                body.len()
                            );
                            let _ = write.write_all(head.as_bytes()).await;
                            continue;
                        }
                        Some((a, z)) => {
                            let z = z.unwrap_or(body.len() - 1).min(body.len() - 1);
                            s.range_requests.fetch_add(1, Ordering::Relaxed);
                            cut_short = a > 0 && take_one(&drops);
                            (
                                "206 Partial Content",
                                &body[a..=z],
                                format!("content-range: bytes {a}-{z}/{}\r\n", body.len()),
                            )
                        }
                        None => ("200 OK", &body[..], String::new()),
                    };
                    let disposition = if b.no_disposition {
                        ""
                    } else {
                        "content-disposition: attachment; filename=\"payload.bin\"\r\n"
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\ncontent-length: {}\r\naccept-ranges: bytes\r\n{disposition}{extra}\r\n",
                        slice.len()
                    );
                    if write.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    s.max_concurrent.fetch_max(now, Ordering::SeqCst);
                    let mut sent = 0;
                    let mut ok = true;
                    for chunk in slice.chunks(64 * 1024) {
                        if !b.pace.is_zero() {
                            tokio::time::sleep(b.pace).await;
                        }
                        if cut_short && sent >= 100 * 1024 {
                            ok = false;
                            break;
                        }
                        if write.write_all(chunk).await.is_err() {
                            ok = false;
                            break;
                        }
                        sent += chunk.len();
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    if !ok {
                        return; // closes the connection mid-body
                    }
                }
            });
        }
    });
    server
}

/// Decrements `counter` if it is positive; true when it did.
fn take_one(counter: &AtomicUsize) -> bool {
    let mut current = counter.load(Ordering::Relaxed);
    while current > 0 {
        match counter.compare_exchange(current, current - 1, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
    false
}

pub fn payload(len: usize) -> Arc<Vec<u8>> {
    Arc::new((0..len).map(|i| (i * 31 % 251) as u8).collect())
}

pub fn config(dir: &Path) -> EngineConfig {
    EngineConfig {
        database_path: Some(dir.join("state.sqlite3")),
        download_dir: dir.join("downloads"),
        min_segment_size: 256 * 1024,
        write_buffer_size: 64 * 1024,
        ramp_interval: Duration::from_millis(50),
        checkpoint_interval: Duration::from_millis(100),
        ..EngineConfig::default()
    }
}

/// Waits for `status`, panicking early if the download fails instead.
pub async fn wait_for(manager: &DownloadManager, id: DownloadId, status: DownloadStatus) {
    for _ in 0..800 {
        let s = manager.get(id).unwrap();
        if s.status == status {
            return;
        }
        if status != DownloadStatus::Failed {
            assert_ne!(
                s.status,
                DownloadStatus::Failed,
                "download failed: {:?}",
                s.error
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "timed out waiting for {status:?}: {:?}",
        manager.get(id).unwrap()
    );
}
