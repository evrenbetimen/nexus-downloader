//! End-to-end tests against a tiny local HTTP/1.1 server with range support.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use nexus_core::{DownloadManager, DownloadRequest, DownloadStatus, EngineConfig, RequestContext};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[derive(Debug)]
struct Server {
    addr: SocketAddr,
    range_requests: Arc<AtomicUsize>,
}

/// Serves `body` at any path. Honors `Range`, delays each 64 KiB write by
/// `pace` to keep downloads running long enough to pause, and answers 403
/// unless the request carries `Cookie: session=ok` when `require_cookie`.
async fn serve(body: Arc<Vec<u8>>, pace: Duration, require_cookie: bool) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let range_requests = Arc::new(AtomicUsize::new(0));
    let counter = range_requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            let counter = counter.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                loop {
                    let mut range = None;
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
                            let (a, b) = v.split_once('-').unwrap();
                            let a: usize = a.parse().unwrap();
                            let b: usize = if b.is_empty() {
                                body.len() - 1
                            } else {
                                b.parse().unwrap()
                            };
                            range = Some((a, b.min(body.len() - 1)));
                        }
                        if lower == "cookie: session=ok" {
                            cookie_ok = true;
                        }
                    }
                    if require_cookie && !cookie_ok {
                        let _ = write
                            .write_all(b"HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\n\r\n")
                            .await;
                        continue;
                    }
                    let (status, slice, extra) = match range {
                        Some((a, b)) => {
                            counter.fetch_add(1, Ordering::Relaxed);
                            (
                                "206 Partial Content",
                                &body[a..=b],
                                format!("content-range: bytes {a}-{b}/{}\r\n", body.len()),
                            )
                        }
                        None => ("200 OK", &body[..], String::new()),
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\ncontent-length: {}\r\naccept-ranges: bytes\r\n\
                         content-disposition: attachment; filename=\"payload.bin\"\r\n{extra}\r\n",
                        slice.len()
                    );
                    if write.write_all(head.as_bytes()).await.is_err() {
                        return;
                    }
                    for chunk in slice.chunks(64 * 1024) {
                        if !pace.is_zero() {
                            tokio::time::sleep(pace).await;
                        }
                        if write.write_all(chunk).await.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    Server {
        addr,
        range_requests,
    }
}

fn payload(len: usize) -> Arc<Vec<u8>> {
    Arc::new((0..len).map(|i| (i * 31 % 251) as u8).collect())
}

fn config(dir: &std::path::Path) -> EngineConfig {
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

async fn wait_for(manager: &DownloadManager, id: nexus_core::DownloadId, status: DownloadStatus) {
    for _ in 0..600 {
        let s = manager.get(id).unwrap();
        if s.status == status {
            return;
        }
        assert_ne!(
            s.status,
            DownloadStatus::Failed,
            "download failed: {:?}",
            s.error
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {status:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_in_parallel_segments() {
    let body = payload(8 * 1024 * 1024 + 123);
    let server = serve(body.clone(), Duration::from_millis(1), false).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();

    let snapshot = manager
        .add(DownloadRequest {
            url: format!("http://{}/files/ignored-name.bin", server.addr),
            directory: None,
            file_name: None,
            context: RequestContext::default(),
        })
        .await
        .unwrap();
    wait_for(&manager, snapshot.id, DownloadStatus::Completed).await;

    let done = manager.get(snapshot.id).unwrap();
    assert_eq!(
        done.file_name, "payload.bin",
        "name from Content-Disposition"
    );
    assert_eq!(done.total_bytes, Some(body.len() as u64));
    assert_eq!(done.downloaded_bytes, body.len() as u64);
    assert_eq!(std::fs::read(&done.path).unwrap(), *body);
    // One probe plus several segment fetches proves the file was split.
    assert!(server.range_requests.load(Ordering::Relaxed) > 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pauses_and_resumes_across_restarts() {
    let body = payload(4 * 1024 * 1024);
    let server = serve(body.clone(), Duration::ZERO, true).await;
    let dir = tempfile::tempdir().unwrap();
    let request = DownloadRequest {
        url: format!("http://{}/video.mp4", server.addr),
        directory: None,
        file_name: Some("clip.mp4".into()),
        context: RequestContext {
            cookies: Some("session=ok".into()),
            ..RequestContext::default()
        },
    };

    let id = {
        let manager = DownloadManager::new(config(dir.path())).unwrap();
        // Throttle so the pause lands mid-transfer.
        manager.set_speed_limit(Some(2 * 1024 * 1024));
        let id = manager.add(request).await.unwrap().id;
        wait_for(&manager, id, DownloadStatus::Downloading).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        manager.pause(id).await.unwrap();
        let paused = manager.get(id).unwrap();
        assert_eq!(paused.status, DownloadStatus::Paused);
        assert!(paused.downloaded_bytes > 0 && paused.downloaded_bytes < body.len() as u64);
        manager.shutdown().await;
        id
    };

    // A fresh manager (as after an app restart) restores the byte map.
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let restored = manager.get(id).unwrap();
    assert_eq!(restored.status, DownloadStatus::Paused);
    assert!(restored.downloaded_bytes > 0);
    manager.resume(id).unwrap();
    wait_for(&manager, id, DownloadStatus::Completed).await;
    let done = manager.get(id).unwrap();
    assert_eq!(done.file_name, "clip.mp4");
    assert_eq!(std::fs::read(&done.path).unwrap(), *body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_session_cookie_fails_cleanly() {
    let server = serve(payload(1024), Duration::ZERO, true).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let id = manager
        .add(DownloadRequest {
            url: format!("http://{}/a.bin", server.addr),
            directory: None,
            file_name: None,
            context: RequestContext::default(),
        })
        .await
        .unwrap()
        .id;
    for _ in 0..200 {
        if manager.get(id).unwrap().status == DownloadStatus::Failed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let failed = manager.get(id).unwrap();
    assert_eq!(failed.status, DownloadStatus::Failed);
    assert!(failed.error.unwrap().contains("403"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupted_downloads_resume_on_next_launch() {
    let body = payload(4 * 1024 * 1024);
    let server = serve(body.clone(), Duration::ZERO, false).await;
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let manager = DownloadManager::new(config(dir.path())).unwrap();
        manager.set_speed_limit(Some(2 * 1024 * 1024));
        let id = manager
            .add(DownloadRequest {
                url: format!("http://{}/big.iso", server.addr),
                directory: None,
                file_name: None,
                context: RequestContext::default(),
            })
            .await
            .unwrap()
            .id;
        wait_for(&manager, id, DownloadStatus::Downloading).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        // App quits mid-download.
        manager.shutdown().await;
        id
    };
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    assert_eq!(manager.get(id).unwrap().status, DownloadStatus::Paused);
    manager.resume_interrupted();
    wait_for(&manager, id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(manager.get(id).unwrap().path).unwrap(), *body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn speed_limit_throttles_mid_download() {
    let body = payload(3 * 1024 * 1024);
    let server = serve(body.clone(), Duration::ZERO, false).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    manager.set_speed_limit(Some(1024 * 1024));
    let started = std::time::Instant::now();
    let id = manager
        .add(DownloadRequest {
            url: format!("http://{}/limited.bin", server.addr),
            directory: None,
            file_name: None,
            context: RequestContext::default(),
        })
        .await
        .unwrap()
        .id;
    wait_for(&manager, id, DownloadStatus::Completed).await;
    // 3 MiB at 1 MiB/s (with a 256 KiB burst) cannot finish in under ~2.5 s.
    assert!(
        started.elapsed() > Duration::from_millis(2500),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(std::fs::read(manager.get(id).unwrap().path).unwrap(), *body);
}
