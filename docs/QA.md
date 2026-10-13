# QA and QC

## Automated (runs in CI on every PR and before every release)

| Layer | Command | What it covers |
| --- | --- | --- |
| Static | `cargo fmt --check`, `cargo clippy -- -D warnings` (pedantic), `pnpm typecheck` (strict) | Style, lints, types |
| Engine unit | `cargo test -p nexus-core --lib` | Segment splitting, byte map accounting, limiter, SQLite store, HTTP parsing |
| Engine end-to-end | `cargo test -p nexus-core --tests` | Real downloads against a local server that can drop connections, return 503/416, ignore ranges, require cookies |
| UI unit | `pnpm test:web` | Formatting, speed-limit scale, store reducers |
| UI end-to-end | `pnpm --filter @nexusdl/desktop test:e2e` | The built UI in Chromium against the simulated engine |
| Browser bridge | `cargo test -p nexus-bridge` (Linux, Windows, macOS) | Native messaging framing, host launch and relay, loopback channel auth, manifest registration |
| Extension unit | `pnpm --filter @nexusdl/extension test` | Capture policy, hand-over and fallback logic |
| Extension in Chromium | `pnpm --filter @nexusdl/extension test:e2e` | The built extension in real Chromium with a stub host: capture, cookies, cancel, fallback, popup |
| Security | `cargo audit`, `pnpm audit` | Known vulnerabilities in dependencies |
| Coverage | `cargo llvm-cov -p nexus-core`, `pnpm --filter @nexusdl/desktop test:coverage` | Line coverage report |

## Manual release checklist

Run on each installer before calling a release stable.

- [ ] Installer opens and installs; app launches with an empty list.
- [ ] Add a large public file (for example a Linux ISO). It uses several
      connections, the segment map fills, and the finished file's checksum
      matches the published one.
- [ ] Pause, quit the app, relaunch, resume: the download continues from where
      it stopped and the checksum still matches.
- [ ] Kill the app (Task Manager / `kill -9`) mid-download, relaunch: it
      resumes on its own.
- [ ] Pull the network cable mid-download for 30 seconds: it recovers.
- [ ] Move the speed slider during a download: total speed follows within a
      second or two.
- [ ] Download from a server without range support (e.g. some dynamic
      download links): it completes on one connection.
- [ ] Add the same URL twice: two separate files, both correct.
- [ ] Delete removes the row; the file stays on disk.
- [ ] 1,000+ downloads in the list: scrolling stays smooth.

### Browser extension

- [ ] Open the app once, then install the extension (see
      [EXTENSION.md](EXTENSION.md)). The popup says "Connected to NexusDL".
- [ ] Click a download link for a large file: the browser's download
      disappears and NexusDL shows "Download from browser". Start it; the
      checksum matches.
- [ ] Quit NexusDL and click another link: the app starts by itself and
      shows the dialog.
- [ ] Download from a site that needs you to be signed in: it works in
      NexusDL too.
- [ ] Right-click a link → "Download with NexusDL".
- [ ] Uninstall or rename the app and click a link: the browser downloads
      the file itself and the toolbar button shows "!".
