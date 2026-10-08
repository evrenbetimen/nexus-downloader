//! End-to-end tests against a tiny local HTTP/1.1 server with range support.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{Behaviour, config, payload, serve, wait_for};
use nexus_core::{DownloadManager, DownloadRequest, DownloadStatus, RequestContext};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_in_parallel_segments() {
    let body = payload(8 * 1024 * 1024 + 123);
    let server = serve(
        body.clone(),
        Behaviour {
            pace: Duration::from_millis(1),
            ..Behaviour::default()
        },
    )
    .await;
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
    let server = serve(
        body.clone(),
        Behaviour {
            require_cookie: true,
            ..Behaviour::default()
        },
    )
    .await;
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
    let server = serve(
        payload(1024),
        Behaviour {
            require_cookie: true,
            ..Behaviour::default()
        },
    )
    .await;
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
    let server = serve(body.clone(), Behaviour::default()).await;
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
    let server = serve(body.clone(), Behaviour::default()).await;
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
