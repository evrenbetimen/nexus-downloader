//! Browser integration: receives downloads captured by the extension and hands
//! them to the webview, which confirms them in the add dialog.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use nexus_bridge::ipc::IpcServer;
use nexus_bridge::protocol::{BrowserMessage, CapturedDownload};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{info, warn};

/// Tells the webview that captured downloads are waiting in [`Inbox`].
pub const CAPTURED_EVENT: &str = "download://captured";

/// Must match `identifier` in `tauri.conf.json`: the host process runs without
/// Tauri and finds the IPC endpoint through it.
pub const APP_IDENTIFIER: &str = "dev.nexusdl.app";

/// The app data directory as Tauri computes it, usable without Tauri.
#[must_use]
pub fn data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|dir| dir.join(APP_IDENTIFIER))
}

/// Captured downloads not yet shown to the user. A queue rather than event
/// payloads, because a capture that launched the app arrives before the
/// webview is listening.
#[derive(Debug, Default)]
pub struct Inbox(Mutex<VecDeque<CapturedDownload>>);

impl Inbox {
    pub fn take(&self) -> Vec<CapturedDownload> {
        self.0.lock().expect("inbox poisoned").drain(..).collect()
    }
}

/// Keeps the IPC channel open for the app's lifetime.
#[derive(Debug)]
pub struct Bridge {
    _server: IpcServer,
}

/// Registers the native messaging host and starts listening for the extension.
pub fn start(app: &AppHandle, data_dir: &Path) {
    app.manage(Inbox::default());

    if let Some(exe) = nexus_bridge::host::app_executable() {
        let data_dir = data_dir.to_owned();
        tauri::async_runtime::spawn_blocking(move || {
            for result in nexus_bridge::register::install(&exe, &data_dir) {
                match result {
                    Ok(r) => info!(
                        "browser host registered for {} at {}",
                        r.browser, r.location
                    ),
                    Err(e) => warn!("browser host registration failed: {e}"),
                }
            }
        });
    } else {
        warn!("cannot locate the NexusDL executable; browser capture disabled");
    }

    let handle = app.clone();
    let server = tauri::async_runtime::block_on(IpcServer::start(data_dir, move |message| {
        receive(&handle, message)
    }));
    match server {
        Ok(server) => {
            app.manage(Bridge { _server: server });
        }
        Err(e) => warn!("browser bridge unavailable: {e}"),
    }
}

fn receive(app: &AppHandle, message: BrowserMessage) -> Result<(), String> {
    let BrowserMessage::Download(download) = message else {
        return Ok(());
    };
    download.validate()?;
    info!("download captured from the browser: {}", download.url);
    app.state::<Inbox>()
        .0
        .lock()
        .expect("inbox poisoned")
        .push_back(download);
    if let Some(window) = app.get_webview_window("main") {
        // Bring the confirmation dialog in front, like IDM does.
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    app.emit(CAPTURED_EVENT, ()).map_err(|e| e.to_string())
}
