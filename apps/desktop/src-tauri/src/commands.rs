//! `invoke` handlers. Every command is a thin, typed pass-through to the manager.

// Tauri injects `State` by value; commands can't take it by reference.
#![allow(clippy::needless_pass_by_value)]

use nexus_core::{DownloadId, DownloadManager, DownloadRequest, DownloadSnapshot, NexusError};
use serde::{Serialize, Serializer};
use tauri::State;

/// Errors cross the bridge as plain messages; the UI shows them verbatim.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct CommandError(#[from] NexusError);

impl Serialize for CommandError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

type CommandResult<T> = Result<T, CommandError>;

#[tauri::command]
pub fn list_downloads(manager: State<'_, DownloadManager>) -> Vec<DownloadSnapshot> {
    manager.list()
}

#[tauri::command]
pub async fn add_download(
    manager: State<'_, DownloadManager>,
    request: DownloadRequest,
) -> CommandResult<DownloadSnapshot> {
    Ok(manager.add(request).await?)
}

#[tauri::command]
pub async fn pause_download(
    manager: State<'_, DownloadManager>,
    id: DownloadId,
) -> CommandResult<()> {
    Ok(manager.pause(id).await?)
}

#[tauri::command]
pub fn resume_download(manager: State<'_, DownloadManager>, id: DownloadId) -> CommandResult<()> {
    Ok(manager.resume(id)?)
}

#[tauri::command]
pub async fn remove_download(
    manager: State<'_, DownloadManager>,
    id: DownloadId,
    delete_files: bool,
) -> CommandResult<()> {
    Ok(manager.remove(id, delete_files).await?)
}

/// `None` removes the cap. Takes effect mid-download.
#[tauri::command]
pub fn set_speed_limit(manager: State<'_, DownloadManager>, bytes_per_sec: Option<u64>) {
    manager.set_speed_limit(bytes_per_sec);
}

#[tauri::command]
pub fn get_speed_limit(manager: State<'_, DownloadManager>) -> Option<u64> {
    manager.speed_limit()
}
