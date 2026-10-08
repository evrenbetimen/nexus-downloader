//! Tauri shell: hosts the [`DownloadManager`] and bridges it to the webview.

mod commands;
mod events;

use nexus_core::{DownloadManager, EngineConfig};
use tauri::{Manager, RunEvent};
use tracing_subscriber::EnvFilter;

/// # Panics
/// Panics if the Tauri application cannot be built (invalid config or a
/// missing webview runtime), which leaves nothing to recover.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,nexus_core=debug")),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let download_dir = app
                .path()
                .download_dir()
                .unwrap_or_else(|_| data_dir.join("downloads"));
            let config = EngineConfig {
                database_path: Some(data_dir.join("nexusdl.sqlite3")),
                download_dir,
                ..EngineConfig::default()
            };
            // The manager captures the current Tokio runtime, so build it inside Tauri's.
            let manager = tauri::async_runtime::block_on(async { DownloadManager::new(config) })?;
            manager.resume_interrupted();
            events::spawn_bridges(app.handle(), &manager);
            app.manage(manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_downloads,
            commands::add_download,
            commands::pause_download,
            commands::resume_download,
            commands::remove_download,
            commands::set_speed_limit,
            commands::get_speed_limit,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build the NexusDL application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                // Checkpoint every running download so the next launch resumes it.
                let manager = app.state::<DownloadManager>().inner().clone();
                tauri::async_runtime::block_on(manager.shutdown());
            }
        });
}
