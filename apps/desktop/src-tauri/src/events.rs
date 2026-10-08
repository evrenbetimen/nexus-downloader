//! Push channels from the engine to the webview.

use std::time::Duration;

use nexus_core::{DownloadManager, ProgressSample};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::MissedTickBehavior;
use tracing::warn;

/// Batched progress of all running downloads, at most once per display frame.
pub const PROGRESS_EVENT: &str = "download://progress";
/// Added / updated / removed notifications.
pub const LIFECYCLE_EVENT: &str = "download://lifecycle";
/// The UI missed lifecycle events and should reload the full list.
pub const RESYNC_EVENT: &str = "download://resync";

const FRAME: Duration = Duration::from_micros(16_667);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressFrame {
    downloads: Vec<ProgressSample>,
    total_speed_bps: u64,
}

pub fn spawn_bridges(app: &AppHandle, manager: &DownloadManager) {
    let (handle, engine) = (app.clone(), manager.clone());
    tauri::async_runtime::spawn(async move {
        let mut frame = tokio::time::interval(FRAME);
        frame.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut was_idle = true;
        loop {
            frame.tick().await;
            let downloads = engine.progress();
            // Emit one empty frame when activity stops, then stay silent.
            if downloads.is_empty() && was_idle {
                continue;
            }
            was_idle = downloads.is_empty();
            let total_speed_bps = downloads.iter().map(|d| d.speed_bps).sum();
            if let Err(e) = handle.emit(
                PROGRESS_EVENT,
                ProgressFrame {
                    downloads,
                    total_speed_bps,
                },
            ) {
                warn!("progress emit failed: {e}");
            }
        }
    });

    let (handle, mut events) = (app.clone(), manager.subscribe());
    tauri::async_runtime::spawn(async move {
        loop {
            let result = match events.recv().await {
                Ok(event) => handle.emit(LIFECYCLE_EVENT, event),
                Err(RecvError::Lagged(_)) => handle.emit(RESYNC_EVENT, ()),
                Err(RecvError::Closed) => break,
            };
            if let Err(e) = result {
                warn!("lifecycle emit failed: {e}");
            }
        }
    });
}
