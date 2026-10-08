//! QA edge cases: misbehaving servers, lifecycle races and file handling.

mod common;

use std::time::Duration;

use common::{Behaviour, config, payload, serve, wait_for};
use nexus_core::{
    DownloadManager, DownloadRequest, DownloadSnapshot, DownloadStatus, EngineConfig,
    RequestContext,
};

fn request(url: String) -> DownloadRequest {
    DownloadRequest {
        url,
        directory: None,
        file_name: None,
        context: RequestContext::default(),
    }
}

async fn download(manager: &DownloadManager, url: String) -> DownloadSnapshot {
    let id = manager.add(request(url)).await.unwrap().id;
    wait_for(manager, id, DownloadStatus::Completed).await;
    manager.get(id).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn falls_back_to_one_stream_without_range_support() {
    let body = payload(2 * 1024 * 1024 + 7);
    let server = serve(
        body.clone(),
        Behaviour {
            ignore_range: true,
            ..Behaviour::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let done = download(&manager, server.url("plain.bin")).await;
    assert!(!done.resumable);
    assert_eq!(done.downloaded_bytes, body.len() as u64);
    assert_eq!(std::fs::read(&done.path).unwrap(), *body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_connections_dropped_mid_segment() {
    let body = payload(4 * 1024 * 1024);
    let server = serve(
        body.clone(),
        Behaviour {
            drop_first: 6,
            ..Behaviour::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let done = download(&manager, server.url("flaky.bin")).await;
    assert_eq!(std::fs::read(&done.path).unwrap(), *body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retries_transient_server_errors_including_the_probe() {
    let body = payload(1024 * 1024);
    let server = serve(
        body.clone(),
        Behaviour {
            fail_first: 3,
            ..Behaviour::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let done = download(&manager, server.url("busy.bin")).await;
    assert_eq!(std::fs::read(&done.path).unwrap(), *body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_empty_files() {
    let server = serve(payload(0), Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let done = download(&manager, server.url("empty.txt")).await;
    assert_eq!(done.downloaded_bytes, 0);
    assert_eq!(std::fs::read(&done.path).unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn never_exceeds_the_connection_cap() {
    let body = payload(8 * 1024 * 1024);
    let server = serve(
        body.clone(),
        Behaviour {
            pace: Duration::from_millis(2),
            ..Behaviour::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(EngineConfig {
        max_connections_per_download: 3,
        initial_connections: 8,
        ..config(dir.path())
    })
    .unwrap();
    let id = manager
        .add(request(server.url("capped.bin")))
        .await
        .unwrap()
        .id;
    let mut peak = 0;
    while manager.get(id).unwrap().status != DownloadStatus::Completed {
        for p in manager.progress() {
            peak = peak.max(p.connections);
        }
        assert_ne!(manager.get(id).unwrap().status, DownloadStatus::Failed);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(peak > 1, "never ran segments in parallel");
    assert!(peak <= 3, "peaked at {peak} connections");
    assert_eq!(std::fs::read(manager.get(id).unwrap().path).unwrap(), *body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_downloads_of_one_url_get_distinct_files() {
    let body = payload(3 * 1024 * 1024);
    let server = serve(body.clone(), Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let (a, b, c) = tokio::join!(
        download(&manager, server.url("same.bin")),
        download(&manager, server.url("same.bin")),
        download(&manager, server.url("same.bin")),
    );
    let mut names = vec![a.file_name, b.file_name, c.file_name];
    names.sort();
    assert_eq!(names, ["payload (1).bin", "payload (2).bin", "payload.bin"]);
    for path in [a.path, b.path, c.path] {
        assert_eq!(std::fs::read(path).unwrap(), *body);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn name_comes_from_url_without_content_disposition() {
    let server = serve(
        payload(10),
        Behaviour {
            no_disposition: true,
            ..Behaviour::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let done = download(&manager, server.url("dir/My%20Song.mp3?token=x")).await;
    assert_eq!(done.file_name, "My Song.mp3");
    assert_eq!(done.category, nexus_core::FileCategory::Music);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_deletes_files_and_state() {
    let body = payload(1024 * 1024);
    let server = serve(body.clone(), Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let manager = DownloadManager::new(config(dir.path())).unwrap();
        let done = download(&manager, server.url("gone.bin")).await;
        manager.remove(done.id, true).await.unwrap();
        assert!(!done.path.exists());
        assert!(manager.list().is_empty());
        done.id
    };
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    assert!(
        manager.get(id).is_err(),
        "removed download came back after restart"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rapid_pause_resume_cycles_keep_the_file_intact() {
    let body = payload(4 * 1024 * 1024);
    let server = serve(body.clone(), Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    manager.set_speed_limit(Some(4 * 1024 * 1024));
    let id = manager
        .add(request(server.url("cycle.bin")))
        .await
        .unwrap()
        .id;
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_millis(60)).await;
        manager.pause(id).await.unwrap();
        assert_eq!(manager.get(id).unwrap().status, DownloadStatus::Paused);
        manager.resume(id).unwrap();
        manager.resume(id).unwrap(); // double resume must be harmless
    }
    manager.set_speed_limit(None);
    wait_for(&manager, id, DownloadStatus::Completed).await;
    assert_eq!(std::fs::read(manager.get(id).unwrap().path).unwrap(), *body);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_invalid_requests_up_front() {
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    for url in ["ftp://example.com/a", "not a url", "file:///etc/passwd"] {
        assert!(
            manager.add(request(url.into())).await.is_err(),
            "{url} accepted"
        );
    }
    let mut bad_header = request("http://127.0.0.1:9/a".into());
    bad_header.context.user_agent = Some("evil\r\nX-Injected: 1".into());
    assert!(manager.add(bad_header).await.is_err());
    assert!(manager.list().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unreachable_host_fails_with_a_message() {
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(EngineConfig {
        max_retries: 1,
        ..config(dir.path())
    })
    .unwrap();
    // Port 9 (discard) is closed on loopback, so the connection is refused.
    let id = manager
        .add(request("http://127.0.0.1:9/x.bin".into()))
        .await
        .unwrap()
        .id;
    wait_for(&manager, id, DownloadStatus::Failed).await;
    assert!(manager.get(id).unwrap().error.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_supplied_names_are_sanitised() {
    let server = serve(payload(10), Behaviour::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let manager = DownloadManager::new(config(dir.path())).unwrap();
    let mut req = request(server.url("a.bin"));
    req.file_name = Some("../../escape.bin".into());
    let id = manager.add(req).await.unwrap().id;
    wait_for(&manager, id, DownloadStatus::Completed).await;
    let done = manager.get(id).unwrap();
    assert!(
        done.path.starts_with(dir.path().join("downloads")),
        "{:?}",
        done.path
    );
}
