//! Native messaging host mode: the browser starts the NexusDL executable with
//! its stdio connected to the extension, and this module relays each message to
//! the running app, starting the app first when a download needs it.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tracing::warn;

use crate::FIREFOX_EXTENSION_ID;
use crate::ipc::{self, SendError};
use crate::native::{read_message, write_message};
use crate::protocol::{BrowserMessage, HostReply};

/// How long a download waits for a freshly launched app to start listening.
pub const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(250);

/// Whether the process was started by a browser as a native messaging host.
///
/// Chromium passes the caller's origin (`chrome-extension://<id>/`, plus
/// `--parent-window=` on Windows); Firefox passes the manifest path and the
/// add-on ID. Nothing else starts NexusDL with those arguments.
#[must_use]
pub fn is_host_invocation(args: &[OsString]) -> bool {
    args.iter().skip(1).any(|arg| {
        arg.to_str()
            .is_some_and(|a| a.starts_with("chrome-extension://") || a == FIREFOX_EXTENSION_ID)
    })
}

/// Where and how the host reaches the app.
pub struct HostConfig<'a> {
    /// The app's data directory, which holds the IPC endpoint file.
    pub data_dir: &'a Path,
    /// Starts the desktop app in the background.
    pub launch: &'a dyn Fn() -> io::Result<()>,
    pub startup_timeout: Duration,
}

impl std::fmt::Debug for HostConfig<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostConfig")
            .field("data_dir", &self.data_dir)
            .field("startup_timeout", &self.startup_timeout)
            .finish_non_exhaustive()
    }
}

/// Answers messages until the browser closes the pipe.
pub fn run(
    reader: &mut impl Read,
    writer: &mut impl Write,
    config: &HostConfig<'_>,
) -> io::Result<()> {
    while let Some(raw) = read_message::<serde_json::Value>(reader)? {
        let reply = match serde_json::from_value::<BrowserMessage>(raw) {
            Ok(message) => handle(&message, config),
            Err(e) => HostReply::error(false, format!("unsupported message: {e}")),
        };
        write_message(writer, &reply)?;
    }
    Ok(())
}

fn handle(message: &BrowserMessage, config: &HostConfig<'_>) -> HostReply {
    match message {
        // A ping only reports state; the popup must not start the app.
        BrowserMessage::Ping => match ipc::send(config.data_dir, message) {
            Ok(()) => HostReply::ok(true),
            Err(SendError::Unreachable(_)) => HostReply::ok(false),
            Err(SendError::Rejected(e)) => HostReply::error(true, e),
        },
        BrowserMessage::Download(download) => match download.validate() {
            Ok(()) => deliver(message, config),
            Err(e) => HostReply::error(false, e),
        },
    }
}

fn deliver(message: &BrowserMessage, config: &HostConfig<'_>) -> HostReply {
    match ipc::send(config.data_dir, message) {
        Ok(()) => return HostReply::ok(true),
        Err(SendError::Rejected(e)) => return HostReply::error(true, e),
        Err(SendError::Unreachable(_)) => {}
    }
    if let Err(e) = (config.launch)() {
        return HostReply::error(false, format!("could not start NexusDL: {e}"));
    }
    let deadline = Instant::now() + config.startup_timeout;
    loop {
        std::thread::sleep(POLL);
        match ipc::send(config.data_dir, message) {
            Ok(()) => return HostReply::ok(true),
            Err(SendError::Rejected(e)) => return HostReply::error(true, e),
            Err(SendError::Unreachable(e)) if Instant::now() >= deadline => {
                return HostReply::error(false, format!("NexusDL did not start in time ({e})"));
            }
            Err(SendError::Unreachable(_)) => {}
        }
    }
}

/// The path browsers and the host should start: inside an `AppImage` the running
/// binary lives in a temporary mount, so use the image itself.
#[must_use]
pub fn app_executable() -> Option<PathBuf> {
    if let Some(image) = std::env::var_os("APPIMAGE").filter(|p| !p.is_empty()) {
        return Some(image.into());
    }
    std::env::current_exe().ok()
}

/// Starts `exe` fully detached from the host: no inherited stdio (that is the
/// browser's pipe), its own process group, and outside the browser's job
/// object on Windows, so closing the host doesn't take the app with it.
pub fn spawn_detached(exe: &Path) -> io::Result<()> {
    let mut command = Command::new(exe);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        command.creation_flags(
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB,
        );
        // Browsers that forbid breakaway reject the flag; start inside the job then.
        if command.spawn().is_ok() {
            return Ok(());
        }
        warn!("starting NexusDL outside the browser job failed; retrying inside it");
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map(drop)
}

/// Entry point for the executable's host mode, wired to the real stdio.
pub fn run_stdio(data_dir: &Path, exe: &Path) -> io::Result<()> {
    let launch = || spawn_detached(exe);
    let config = HostConfig {
        data_dir,
        launch: &launch,
        startup_timeout: STARTUP_TIMEOUT,
    };
    let result = run(&mut io::stdin().lock(), &mut io::stdout().lock(), &config);
    if let Err(e) = &result {
        warn!("native messaging host stopped: {e}");
    }
    result
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::ipc::IpcServer;
    use crate::protocol::CapturedDownload;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn recognizes_browser_launches() {
        assert!(is_host_invocation(&os(&[
            "nexusdl",
            "chrome-extension://aiahehffjnedhfbjllinegdigmhpkghl/"
        ])));
        assert!(is_host_invocation(&os(&[
            "nexusdl.exe",
            "chrome-extension://aiahehffjnedhfbjllinegdigmhpkghl/",
            "--parent-window=0"
        ])));
        assert!(is_host_invocation(&os(&[
            "nexusdl",
            "/home/u/.mozilla/native-messaging-hosts/dev.nexusdl.host.json",
            "nexusdl@nexusdl.dev"
        ])));
        assert!(!is_host_invocation(&os(&["nexusdl"])));
        assert!(!is_host_invocation(&os(&["chrome-extension://x/"])));
    }

    fn frames(messages: &[serde_json::Value]) -> Cursor<Vec<u8>> {
        let mut wire = Vec::new();
        for m in messages {
            write_message(&mut wire, m).unwrap();
        }
        Cursor::new(wire)
    }

    fn replies(wire: Vec<u8>) -> Vec<HostReply> {
        let mut reader = Cursor::new(wire);
        std::iter::from_fn(|| read_message(&mut reader).unwrap()).collect()
    }

    fn run_host(
        dir: &Path,
        launch: &dyn Fn() -> io::Result<()>,
        input: &[serde_json::Value],
    ) -> Vec<HostReply> {
        let config = HostConfig {
            data_dir: dir,
            launch,
            startup_timeout: Duration::from_secs(5),
        };
        let mut out = Vec::new();
        run(&mut frames(input), &mut out, &config).unwrap();
        replies(out)
    }

    #[test]
    fn ping_without_app_does_not_launch_it() {
        let dir = tempfile::tempdir().unwrap();
        let launches = AtomicUsize::new(0);
        let launch = || {
            launches.fetch_add(1, Ordering::Relaxed);
            Ok(())
        };
        let out = run_host(
            dir.path(),
            &launch,
            &[
                serde_json::json!({"type": "ping"}),
                serde_json::json!({"type": "nope"}),
            ],
        );
        assert_eq!(out[0], HostReply::ok(false));
        assert!(!out[1].ok);
        assert_eq!(launches.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn invalid_downloads_are_refused_before_launching() {
        let dir = tempfile::tempdir().unwrap();
        let launch = || -> io::Result<()> { panic!("must not launch") };
        let out = run_host(
            dir.path(),
            &launch,
            &[serde_json::json!({"type": "download", "url": "blob:https://x/1"})],
        );
        assert!(out[0].error.as_deref().unwrap().contains("scheme"));
    }

    #[test]
    fn launches_the_app_and_delivers_once_it_listens() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let server = Arc::new(Mutex::new(None));

        // "Launching" starts the IPC server a moment later, like a real app.
        let launch = {
            let (dir, received, server) = (
                dir.path().to_owned(),
                Arc::clone(&received),
                Arc::clone(&server),
            );
            let handle = runtime.handle().clone();
            move || {
                let (dir, received, server) =
                    (dir.clone(), Arc::clone(&received), Arc::clone(&server));
                handle.spawn(async move {
                    tokio::time::sleep(Duration::from_millis(600)).await;
                    let ipc = IpcServer::start(&dir, move |m| {
                        received.lock().unwrap().push(m);
                        Ok(())
                    })
                    .await
                    .unwrap();
                    *server.lock().unwrap() = Some(ipc);
                });
                Ok(())
            }
        };
        let download = serde_json::json!({
            "type": "download",
            "url": "https://cdn.example/big.iso",
            "cookies": "sid=7",
            "referrer": "https://example/",
        });
        let out = run_host(dir.path(), &launch, &[download.clone(), download]);
        assert_eq!(out, vec![HostReply::ok(true), HostReply::ok(true)]);
        let received = received.lock().unwrap();
        assert_eq!(received.len(), 2);
        assert_eq!(
            received[0],
            BrowserMessage::Download(CapturedDownload {
                url: "https://cdn.example/big.iso".into(),
                cookies: Some("sid=7".into()),
                referrer: Some("https://example/".into()),
                ..CapturedDownload::default()
            })
        );
        drop(received);
        let _ = server.lock().unwrap().take();
    }

    #[test]
    fn reports_launch_failures_and_timeouts() {
        let dir = tempfile::tempdir().unwrap();
        let download = serde_json::json!({"type": "download", "url": "https://x/a.zip"});

        let broken = || Err(io::Error::new(io::ErrorKind::NotFound, "gone"));
        let out = run_host(dir.path(), &broken, std::slice::from_ref(&download));
        assert!(out[0].error.as_deref().unwrap().contains("could not start"));

        let config = HostConfig {
            data_dir: dir.path(),
            launch: &|| Ok(()),
            startup_timeout: Duration::from_millis(300),
        };
        let mut out = Vec::new();
        run(&mut frames(&[download]), &mut out, &config).unwrap();
        assert!(
            replies(out)[0]
                .error
                .as_deref()
                .unwrap()
                .contains("did not start")
        );
    }

    #[test]
    fn spawned_apps_do_not_inherit_the_browser_pipe() {
        // `true` exits at once; spawning must succeed without touching our stdio.
        #[cfg(unix)]
        spawn_detached(Path::new("/bin/true")).unwrap();
        assert!(spawn_detached(Path::new("/definitely/not/here")).is_err());
    }
}
