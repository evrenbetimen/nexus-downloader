//! Loopback channel from the native messaging host to the running app.
//!
//! The app listens on an ephemeral `127.0.0.1` port and publishes the port and
//! a random token in `bridge.json` inside its data directory, readable only by
//! the current user. A request is one JSON line carrying the token; the answer
//! is one JSON line. Web pages can't use the channel: they can't read the
//! token, and anything they can send to a loopback port is HTTP, which never
//! parses as a request.

use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream as AsyncTcpStream};
use tokio::task::JoinHandle;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::native::MAX_MESSAGE;
use crate::protocol::BrowserMessage;

/// File in the app's data directory that tells the host where to connect.
pub const ENDPOINT_FILE: &str = "bridge.json";

const IO_TIMEOUT: Duration = Duration::from_secs(5);
/// A line holds one message plus the envelope.
const MAX_LINE: u64 = MAX_MESSAGE as u64 + 1024;

#[derive(Debug, Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
    pid: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    token: String,
    message: BrowserMessage,
}

#[derive(Debug, Serialize, Deserialize)]
struct Ack {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Handles messages for the app. An `Err` is shown to the extension.
pub trait Handler: Fn(BrowserMessage) -> Result<(), String> + Send + Sync + 'static {}
impl<F: Fn(BrowserMessage) -> Result<(), String> + Send + Sync + 'static> Handler for F {}

/// The app side of the channel. Dropping it stops listening and removes the
/// endpoint file (unless a newer instance has replaced it).
#[derive(Debug)]
pub struct IpcServer {
    endpoint_path: PathBuf,
    token: String,
    addr: SocketAddr,
    accept: JoinHandle<()>,
}

impl IpcServer {
    /// Starts listening and publishes the endpoint in `data_dir`. Must be
    /// called inside a Tokio runtime.
    pub async fn start(data_dir: &Path, handler: impl Handler) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let endpoint_path = data_dir.join(ENDPOINT_FILE);
        write_private(
            &endpoint_path,
            &serde_json::to_vec(&Endpoint {
                port: addr.port(),
                token: token.clone(),
                pid: std::process::id(),
            })?,
        )?;

        let handler = Arc::new(handler);
        let expected = token.clone();
        let accept = tokio::spawn(async move {
            loop {
                let stream = match listener.accept().await {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        warn!("bridge accept failed: {e}");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let (handler, expected) = (Arc::clone(&handler), expected.clone());
                tokio::spawn(async move {
                    match tokio::time::timeout(IO_TIMEOUT, serve(stream, &expected, &*handler))
                        .await
                    {
                        Ok(Err(e)) => debug!("bridge connection failed: {e}"),
                        Err(_) => debug!("bridge connection timed out"),
                        Ok(Ok(())) => {}
                    }
                });
            }
        });
        Ok(Self {
            endpoint_path,
            token,
            addr,
            accept,
        })
    }

    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.accept.abort();
        let ours = read_endpoint(&self.endpoint_path).is_ok_and(|e| e.token == self.token);
        if ours {
            let _ = fs::remove_file(&self.endpoint_path);
        }
    }
}

async fn serve(stream: AsyncTcpStream, expected: &str, handler: &dyn Handler) -> io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    BufReader::new(read)
        .take(MAX_LINE)
        .read_line(&mut line)
        .await?;
    let ack = match serde_json::from_str::<Envelope>(&line) {
        Ok(envelope) if envelope.token == expected => match handler(envelope.message) {
            Ok(()) => Ack {
                ok: true,
                error: None,
            },
            Err(error) => Ack {
                ok: false,
                error: Some(error),
            },
        },
        Ok(_) => Ack {
            ok: false,
            error: Some("unauthorized".into()),
        },
        // Not our protocol (e.g. a browser probing the port): say nothing.
        Err(_) => return Ok(()),
    };
    let mut out = serde_json::to_vec(&ack)?;
    out.push(b'\n');
    write.write_all(&out).await?;
    write.shutdown().await
}

/// Why a message didn't reach the app.
#[derive(Debug)]
pub enum SendError {
    /// No app is listening (not running, or a stale endpoint file).
    Unreachable(io::Error),
    /// The app received the message and refused it.
    Rejected(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(e) => write!(f, "NexusDL is not running ({e})"),
            Self::Rejected(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for SendError {}

/// Delivers one message to the app whose endpoint is published in `data_dir`.
pub fn send(data_dir: &Path, message: &BrowserMessage) -> Result<(), SendError> {
    let endpoint = read_endpoint(&data_dir.join(ENDPOINT_FILE)).map_err(SendError::Unreachable)?;
    let ack = exchange(&endpoint, message).map_err(SendError::Unreachable)?;
    if ack.ok {
        Ok(())
    } else {
        Err(SendError::Rejected(
            ack.error.unwrap_or_else(|| "rejected".into()),
        ))
    }
}

fn exchange(endpoint: &Endpoint, message: &BrowserMessage) -> io::Result<Ack> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, endpoint.port));
    let mut stream = TcpStream::connect_timeout(&addr, IO_TIMEOUT)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut line = serde_json::to_vec(&Envelope {
        token: endpoint.token.clone(),
        message: message.clone(),
    })?;
    line.push(b'\n');
    stream.write_all(&line)?;
    let mut reply = String::new();
    std::io::BufReader::new(stream.take(MAX_LINE)).read_line(&mut reply)?;
    // Whatever answers on a reused port won't speak our protocol.
    serde_json::from_str(&reply).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn read_endpoint(path: &Path) -> io::Result<Endpoint> {
    serde_json::from_slice(&fs::read(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Writes `contents` atomically, readable only by the current user on Unix.
/// (On Windows the per-user data directory already restricts access.)
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::protocol::CapturedDownload;

    fn download(url: &str) -> BrowserMessage {
        BrowserMessage::Download(CapturedDownload {
            url: url.into(),
            cookies: Some("sid=1".into()),
            ..CapturedDownload::default()
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn delivers_messages_and_reports_rejections() {
        let dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let server = IpcServer::start(dir.path(), move |msg| {
            if let BrowserMessage::Download(d) = &msg
                && d.url.contains("/rejected")
            {
                return Err("nope".into());
            }
            log.lock().unwrap().push(msg);
            Ok(())
        })
        .await
        .unwrap();

        let path = dir.path().to_owned();
        tokio::task::spawn_blocking(move || {
            send(&path, &BrowserMessage::Ping).unwrap();
            send(&path, &download("https://x/a.zip")).unwrap();
            match send(&path, &download("https://x/rejected")) {
                Err(SendError::Rejected(reason)) => assert_eq!(reason, "nope"),
                other => panic!("{other:?}"),
            }
        })
        .await
        .unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            vec![BrowserMessage::Ping, download("https://x/a.zip")]
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join(ENDPOINT_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        drop(server);
        assert!(!dir.path().join(ENDPOINT_FILE).exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn refuses_a_wrong_token_and_foreign_protocols() {
        let dir = tempfile::tempdir().unwrap();
        let server = IpcServer::start(dir.path(), |_| Ok(())).await.unwrap();
        let port = server.local_addr().port();

        let replies = tokio::task::spawn_blocking(move || {
            let forged = Endpoint {
                port,
                token: "guess".into(),
                pid: 0,
            };
            let forged = exchange(&forged, &BrowserMessage::Ping).unwrap();

            // What a web page could send to the port.
            let mut http = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            http.write_all(b"POST / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n{}")
                .unwrap();
            // The server hangs up without answering (a reset is fine too).
            let mut answer = Vec::new();
            let _ = http.read_to_end(&mut answer);
            (forged, answer)
        })
        .await
        .unwrap();
        assert!(!replies.0.ok);
        assert_eq!(replies.0.error.as_deref(), Some("unauthorized"));
        assert!(replies.1.is_empty(), "HTTP probes get no answer");
    }

    #[test]
    fn missing_or_stale_endpoints_are_unreachable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            send(dir.path(), &BrowserMessage::Ping),
            Err(SendError::Unreachable(_))
        ));

        // A crashed app leaves its file behind; nothing listens on the port.
        let port = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        write_private(
            &dir.path().join(ENDPOINT_FILE),
            &serde_json::to_vec(&Endpoint {
                port,
                token: "t".into(),
                pid: 1,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            send(dir.path(), &BrowserMessage::Ping),
            Err(SendError::Unreachable(_))
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_newer_instance_keeps_its_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let old = IpcServer::start(dir.path(), |_| Ok(())).await.unwrap();
        let new = IpcServer::start(dir.path(), |_| Ok(())).await.unwrap();
        drop(old);
        let path = dir.path().to_owned();
        tokio::task::spawn_blocking(move || send(&path, &BrowserMessage::Ping).unwrap())
            .await
            .unwrap();
        drop(new);
    }
}
