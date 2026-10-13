# NexusDL

A modern, IDM-class download manager. Rust core engine, Tauri v2 shell, React 19 UI.

## Layout

```
.
├── Cargo.toml                 # Rust workspace (shared deps, lints, release profile)
├── package.json               # pnpm workspace scripts
├── crates/
│   ├── nexus-bridge/          # Browser integration: native messaging host, its
│   │                          # registration, loopback channel to the app
│   └── nexus-core/            # Download engine (no UI dependencies)
│       ├── src/manager.rs     # DownloadManager: lifecycle, workers, checkpoints
│       ├── src/segment.rs     # Byte map + work-stealing segment splitter
│       ├── src/limiter.rs     # Global token-bucket bandwidth limiter
│       ├── src/store.rs       # SQLite persistence (downloads + segment byte maps)
│       ├── src/http.rs        # Pooled client, session-cloned headers, probe
│       └── tests/engine.rs    # End-to-end tests against a local range server
└── apps/
    ├── desktop/               # Tauri v2 app
    │   ├── src-tauri/         # Rust shell: commands + 60 fps event bridge
    │   └── src/               # React 19 + TypeScript + Tailwind v4 UI
    └── extension/             # Browser extension (Chrome, Edge, Brave, Firefox)
```

## Engine

- **Dynamic segmentation.** A download starts with a few connections. A ramp
  controller adds one connection per tick while throughput keeps rising by at
  least 5 %, up to 32. Every new or idle connection halves the segment with the
  most bytes left, so fast connections steal work from slow ones until pieces
  drop below the minimum split size.
- **Connection reuse.** One `reqwest` client (rustls, HTTP/2 with adaptive
  window, keep-alive pool sized to 32 per host) serves every request, so new
  segments ride existing TCP/TLS connections or HTTP/2 streams.
- **Crash-safe resume.** Each segment's flushed byte count is checkpointed to
  SQLite every second, after an `fsync` of the data file, and again on pause or
  quit. Bytes still being written are excluded, so a restart never trusts data
  that isn't on disk. Downloads running at quit or crash resume on next launch.
- **Pre-allocation.** The full file size is reserved up front (`fallocate` and
  friends via `fs2`) to avoid fragmentation and fail early on a full disk.
- **Session cloning.** Each download carries a `RequestContext` (cookie header,
  user agent, referrer, extra headers) applied to every request, so URLs
  captured from an authenticated browser session download without a 403.
- **Throttling.** A global token bucket caps total bandwidth; changing the
  limit applies to running connections from their next chunk.

## Browser capture

Like IDM, NexusDL takes over downloads you start in the browser and asks for
confirmation in its own window, keeping the browser session (cookies,
referrer, user agent) so signed-in and hotlink-protected links work.

```
extension ──native messaging──▶ nexusdl (host mode) ──127.0.0.1 + token──▶ running app
```

- The browser starts the NexusDL executable itself as the native messaging
  host. That short-lived process relays to the running app over a loopback
  socket whose port and token live in a file only the user can read, and
  starts the app first when it isn't running.
- The app registers the host with every Chromium browser and Firefox it finds
  (per user, no admin rights) on each launch, so open the app once after
  installing it.
- The extension captures listed file types (by name, URL or MIME type),
  skips private windows and `blob:`/`data:` downloads, and gives a download
  back to the browser if the app is missing or refuses it. Right-click a link
  for "Download with NexusDL".

See [docs/EXTENSION.md](docs/EXTENSION.md) to install it.

## UI

- Zustand store split into rarely-changing download records and a small
  `live` map that is replaced at most once per frame, so only running rows
  re-render.
- TanStack Table v9 + TanStack Virtual grid (tested with 10,000 rows).
- Canvas segment map driven by `requestAnimationFrame`, reading segment state
  outside React.
- Running `pnpm web:dev` in a plain browser uses a simulated engine so the UI
  can be worked on without the native shell.

## Develop

Prerequisites: Rust (stable), Node 22+, pnpm 10, and the
[Tauri system dependencies](https://v2.tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm dev          # desktop app with hot reload
pnpm web:dev      # UI only, in the browser, simulated engine
pnpm check        # typecheck, unit tests, web + extension build, clippy, Rust tests
pnpm test:e2e     # UI and extension end-to-end tests in Chromium
pnpm ext:build    # extension into apps/extension/dist/{chromium,firefox}
```

See [docs/QA.md](docs/QA.md) for the full QA/QC suite and the manual release
checklist.

## Release

Run the **Release** workflow (Actions → Release → Run workflow) with a tag
matching the app version, e.g. `v0.1.0`, or push that tag. It re-runs the full
CI suite, then builds Windows (`.msi`, `.exe`), macOS (universal `.dmg`) and
Linux (`.deb`, `.rpm`, `.AppImage`) installers and the browser extension
(`NexusDL-extension-chromium-*.zip`, `NexusDL-extension-firefox-*.zip`) and
attaches them to a GitHub release. Installers are currently unsigned.
