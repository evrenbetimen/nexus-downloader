# Changelog

## v0.2.0 — browser capture

### Browser extension (Chrome, Edge, Brave, Vivaldi, Firefox)
- Downloads started in the browser open in NexusDL, like IDM: the browser's
  copy is cancelled and NexusDL asks for confirmation in its own window.
- The browser session goes with the download: cookies, referrer and user
  agent, so signed-in and hotlink-protected links work.
- Captures common archive, disk image, installer, video, audio and PDF types,
  matched by file name, URL or MIME type. The list and a minimum size can be
  changed in the toolbar popup, and capture can be switched off.
- Private windows and in-browser (`blob:`, `data:`) downloads stay in the
  browser.
- If NexusDL is missing or refuses a download, the browser downloads it
  itself and the toolbar button shows why.
- "Download with NexusDL" in the link, video and audio context menu.
- The popup shows whether the app is installed and running.

### App
- The app registers itself with installed browsers on every launch (per
  user, no admin rights needed).
- A captured download starts the app if it isn't running and brings its
  window to the front.

### Known limitations
- The extension is not in the Chrome Web Store or on addons.mozilla.org yet.
  Chromium browsers load it unpacked (developer mode); Firefox only loads
  the unsigned build temporarily, until it restarts.
- POST-based downloads are re-requested with GET and may fail.

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
