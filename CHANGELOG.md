# Changelog

## v0.1.0 — first preview

First public build of NexusDL. It is a preview: the browser extension, the
floating mini-widget and HTTP/3 are not in this release yet.

### Engine
- Dynamic multi-segment downloads: up to 32 connections, added while
  throughput keeps rising, with work stealing between connections.
- Shared HTTP/2 connection pool.
- Pause and resume, with progress checkpointed to SQLite after an fsync.
  Downloads interrupted by a quit or crash resume on the next launch.
- Disk space pre-allocated before downloading.
- Per-download browser session (cookies, user agent, referrer, headers).
- Global speed limit that applies mid-download.
- Single-connection fallback for servers without range support.
- Automatic retries with backoff for dropped connections and 5xx / 429
  responses, including the first request.
- Empty files, duplicate file names and unsafe file names are handled.

### App
- Download grid that stays smooth with 10,000+ entries, sortable columns,
  category and queue sidebar, search.
- Toolbar with Add URL, Resume, Pause, Delete and a speed-limit slider.
- Live segment map of the selected download.
- Drop a link on the window to add it.

### Known limitations
- Installers are not code-signed: Windows SmartScreen and macOS Gatekeeper
  will warn on first launch (macOS: right-click the app, then Open).
- Cookies passed with a download are stored unencrypted in the local
  database so the download can resume.
- No limit yet on how many downloads run at once.
