//! Browser integration: lets the NexusDL extension hand captured downloads to
//! the desktop app, IDM-style.
//!
//! ```text
//! extension ──native messaging (stdio)──▶ nexusdl (host mode) ──TCP 127.0.0.1──▶ running app
//! ```
//!
//! The browser starts the app's own executable as the native messaging host
//! ([`host`]). That short-lived process forwards each message over a
//! token-protected loopback channel ([`ipc`]) to the running app, launching it
//! first if needed. [`register`] writes the host manifests browsers look for.

pub mod host;
pub mod ipc;
pub mod native;
pub mod protocol;
pub mod register;

/// Name browsers use to find the host manifest (`chrome.runtime.sendNativeMessage`).
pub const HOST_NAME: &str = "dev.nexusdl.host";
/// Chromium-family extension ID, fixed by the public key in the extension manifest.
pub const CHROMIUM_EXTENSION_ID: &str = "aiahehffjnedhfbjllinegdigmhpkghl";
/// Firefox add-on ID (`browser_specific_settings.gecko.id`).
pub const FIREFOX_EXTENSION_ID: &str = "nexusdl@nexusdl.dev";
