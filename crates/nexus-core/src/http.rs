//! HTTP plumbing: the shared pooled client, session-cloned requests and the
//! initial probe that discovers size, range support and file name.

use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, RequestBuilder, StatusCode};
use url::Url;

use crate::config::EngineConfig;
use crate::error::{NexusError, Result};
use crate::types::RequestContext;

/// One client for the whole engine: its pool keeps TCP/TLS connections alive
/// and multiplexes HTTP/2 streams, so new segments skip the handshake.
pub fn build_client(config: &EngineConfig) -> Result<Client> {
    Ok(Client::builder()
        .user_agent(&config.user_agent)
        .pool_max_idle_per_host(config.max_connections_per_download)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_nodelay(true)
        .tcp_keepalive(Duration::from_secs(30))
        .http2_adaptive_window(true)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()?)
}

pub fn validate_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim())?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(NexusError::UnsupportedScheme(other.to_owned())),
    }
}

/// Applies the browser's cookies, user agent, referrer and extra headers.
pub fn context_headers(ctx: &RequestContext) -> Result<HeaderMap> {
    fn value(v: &str) -> Result<HeaderValue> {
        HeaderValue::from_str(v).map_err(|_| NexusError::InvalidHeader(v.to_owned()))
    }
    let mut headers = HeaderMap::new();
    if let Some(cookies) = ctx.cookies.as_deref().filter(|c| !c.is_empty()) {
        headers.insert(header::COOKIE, value(cookies)?);
    }
    if let Some(ua) = ctx.user_agent.as_deref() {
        headers.insert(header::USER_AGENT, value(ua)?);
    }
    if let Some(referrer) = ctx.referrer.as_deref() {
        headers.insert(header::REFERER, value(referrer)?);
    }
    for (name, v) in &ctx.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| NexusError::InvalidHeader(name.clone()))?;
        headers.insert(name, value(v)?);
    }
    Ok(headers)
}

pub fn get(client: &Client, url: &str, headers: &HeaderMap) -> RequestBuilder {
    client
        .get(url)
        .headers(headers.clone())
        // Compressed transfer would make byte offsets meaningless.
        .header(header::ACCEPT_ENCODING, "identity")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub final_url: String,
    pub total_bytes: Option<u64>,
    pub resumable: bool,
    pub file_name: Option<String>,
}

/// Asks for the first byte only: a `206` with `Content-Range` proves the server
/// supports ranges and tells us the full size in a single round trip.
pub async fn probe(client: &Client, url: &str, headers: &HeaderMap) -> Result<ProbeResult> {
    let mut resp = get(client, url, headers)
        .header(header::RANGE, "bytes=0-0")
        .send()
        .await?;
    if resp.status() == StatusCode::RANGE_NOT_SATISFIABLE {
        // Empty files have no byte 0; ask again for the whole (empty) body.
        resp = get(client, url, headers).send().await?;
    }
    let status = resp.status();
    if !status.is_success() {
        return Err(NexusError::BadStatus(status.as_u16()));
    }
    let final_url = resp.url().to_string();
    let file_name = resp
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(file_name_from_disposition);
    let (total_bytes, resumable) = if status == StatusCode::PARTIAL_CONTENT {
        let total = resp
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(total_from_content_range);
        (total, total.is_some())
    } else {
        (resp.content_length(), false)
    };
    Ok(ProbeResult {
        final_url,
        total_bytes,
        resumable,
        file_name,
    })
}

/// `bytes 0-0/12345` → `12345`.
fn total_from_content_range(value: &str) -> Option<u64> {
    value.rsplit_once('/')?.1.trim().parse().ok()
}

fn file_name_from_disposition(value: &str) -> Option<String> {
    let mut plain = None;
    for part in value.split(';').map(str::trim) {
        if let Some(encoded) = part.strip_prefix("filename*=") {
            // RFC 5987: charset'lang'percent-encoded
            let encoded = encoded.rsplit('\'').next()?;
            let decoded = percent_decode(encoded);
            if !decoded.is_empty() {
                return Some(sanitize_file_name(&decoded));
            }
        } else if let Some(name) = part.strip_prefix("filename=") {
            plain = Some(sanitize_file_name(name.trim_matches('"')));
        }
    }
    plain.filter(|n| !n.is_empty())
}

/// Last non-empty path segment of the URL, percent-decoded.
#[must_use]
pub fn file_name_from_url(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let last = url.path_segments()?.rev().find(|s| !s.is_empty())?;
    let name = sanitize_file_name(&percent_decode(last));
    (!name.is_empty()).then_some(name)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Strips path separators and characters Windows refuses in file names.
#[must_use]
pub fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    cleaned.trim().trim_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_range_total() {
        assert_eq!(total_from_content_range("bytes 0-0/12345"), Some(12345));
        assert_eq!(total_from_content_range("bytes 0-0/*"), None);
    }

    #[test]
    fn parses_content_disposition() {
        assert_eq!(
            file_name_from_disposition("attachment; filename=\"report.pdf\"").as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            file_name_from_disposition(
                "attachment; filename=\"x\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"
            )
            .as_deref(),
            Some("résumé.pdf")
        );
        assert_eq!(
            file_name_from_disposition("attachment; filename=\"../../etc/passwd\"").as_deref(),
            Some("_.._etc_passwd")
        );
    }

    #[test]
    fn derives_name_from_url() {
        assert_eq!(
            file_name_from_url("https://cdn.example.com/files/My%20Movie.mkv?sig=abc").as_deref(),
            Some("My Movie.mkv")
        );
        assert_eq!(file_name_from_url("https://example.com/"), None);
    }

    #[test]
    fn builds_context_headers() {
        let ctx = RequestContext {
            cookies: Some("a=1; b=2".into()),
            user_agent: Some("Mozilla/5.0".into()),
            referrer: Some("https://example.com/page".into()),
            headers: vec![("Authorization".into(), "Bearer t".into())],
        };
        let h = context_headers(&ctx).unwrap();
        assert_eq!(h[header::COOKIE], "a=1; b=2");
        assert_eq!(h[header::REFERER], "https://example.com/page");
        assert_eq!(h[header::AUTHORIZATION], "Bearer t");
        assert!(
            context_headers(&RequestContext {
                cookies: Some("bad\nvalue".into()),
                ..RequestContext::default()
            })
            .is_err()
        );
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url(" https://example.com/a ").is_ok());
    }
}
