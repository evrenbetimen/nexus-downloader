# NexusDL

A modern, IDM-class download manager. Rust core engine, Tauri v2 shell, React 19 UI.

## Layout

```
.
├── Cargo.toml                 # Rust workspace (shared deps, lints, release profile)
├── package.json               # pnpm workspace scripts
├── crates/
│   └── nexus-core/            # Download engine (no UI dependencies)
│       ├── src/manager.rs     # DownloadManager: lifecycle, workers, checkpoints
│       ├── src/segment.rs     # Byte map + work-stealing segment splitter
│       ├── src/limiter.rs     # Global token-bucket bandwidth limiter
│       ├── src/store.rs       # SQLite persistence (downloads + segment byte maps)
│       ├── src/http.rs        # Pooled client, session-cloned headers, probe
│       └── tests/engine.rs    # End-to-end tests against a local range server
└── apps/
    └── desktop/               # Tauri v2 app
        ├── src-tauri/         # Rust shell: commands + 60 fps event bridge
        └── src/               # React 19 + TypeScript + Tailwind v4 UI
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
pnpm check        # typecheck, UI unit tests, web build, clippy, Rust tests
pnpm test:e2e     # UI end-to-end tests in Chromium
```

See [docs/QA.md](docs/QA.md) for the full QA/QC suite and the manual release
checklist.

## Release

Run the **Release** workflow (Actions → Release → Run workflow) with a tag
matching the app version, e.g. `v0.1.0`, or push that tag. It re-runs the full
CI suite, then builds Windows (`.msi`, `.exe`), macOS (universal `.dmg`) and
Linux (`.deb`, `.rpm`, `.AppImage`) installers and attaches them to a GitHub
release. Installers are currently unsigned.
