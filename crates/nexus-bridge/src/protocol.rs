//! Messages exchanged between the extension, the host and the app.

use serde::{Deserialize, Serialize};
use url::Url;

/// What the extension sends through `runtime.sendNativeMessage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BrowserMessage {
    /// Health check from the extension popup; never launches the app.
    Ping,
    /// A download the extension intercepted (or the user sent from the context menu).
    Download(CapturedDownload),
}

/// A browser download with the session state needed to fetch it again outside
/// the browser.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CapturedDownload {
    pub url: String,
    /// Name the browser picked, if it already knew one.
    pub file_name: Option<String>,
    /// Size the browser reported, when known.
    pub file_size: Option<u64>,
    pub referrer: Option<String>,
    /// `Cookie` header value for `url`.
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

impl CapturedDownload {
    /// Rejects anything but absolute `http(s)` URLs. `blob:`, `data:` and
    /// `file:` downloads only exist inside the browser and must stay there.
    pub fn validate(&self) -> Result<(), String> {
        let url = Url::parse(&self.url).map_err(|e| format!("invalid URL: {e}"))?;
        match url.scheme() {
            "http" | "https" => Ok(()),
            other => Err(format!("unsupported URL scheme `{other}`")),
        }
    }
}

/// The host's answer to the extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostReply {
    pub ok: bool,
    /// NexusDL version, so the extension can show what it talks to.
    pub version: String,
    /// Whether the desktop app answered (a ping does not start it).
    pub app_running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl HostReply {
    #[must_use]
    pub fn ok(app_running: bool) -> Self {
        Self {
            ok: true,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            app_running,
            error: None,
        }
    }

    #[must_use]
    pub fn error(app_running: bool, error: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(error.into()),
            ..Self::ok(app_running)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extension_messages() {
        let msg: BrowserMessage = serde_json::from_str(
            r#"{"type":"download","url":"https://cdn.example/a.zip","fileSize":42,"cookies":"s=1","referrer":"https://example/","userAgent":"UA"}"#,
        )
        .unwrap();
        assert_eq!(
            msg,
            BrowserMessage::Download(CapturedDownload {
                url: "https://cdn.example/a.zip".into(),
                file_name: None,
                file_size: Some(42),
                referrer: Some("https://example/".into()),
                cookies: Some("s=1".into()),
                user_agent: Some("UA".into()),
            })
        );
        let ping: BrowserMessage = serde_json::from_str(r#"{"type":"ping"}"#).unwrap();
        assert_eq!(ping, BrowserMessage::Ping);
    }

    #[test]
    fn only_http_urls_are_accepted() {
        let with = |url: &str| CapturedDownload {
            url: url.into(),
            ..CapturedDownload::default()
        };
        assert!(with("https://example.com/f.iso").validate().is_ok());
        assert!(with("http://10.0.0.2:8080/f").validate().is_ok());
        for bad in [
            "blob:https://example.com/1",
            "data:,x",
            "file:///etc/passwd",
            "nope",
        ] {
            assert!(with(bad).validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn replies_serialize_for_the_extension() {
        let json = serde_json::to_value(HostReply::error(false, "boom")).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["appRunning"], false);
        assert_eq!(json["error"], "boom");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert!(
            serde_json::to_value(HostReply::ok(true))
                .unwrap()
                .get("error")
                .is_none()
        );
    }
}
