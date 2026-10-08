# QA and QC

## Automated (runs in CI on every PR and before every release)

| Layer | Command | What it covers |
| --- | --- | --- |
| Static | `cargo fmt --check`, `cargo clippy -- -D warnings` (pedantic), `pnpm typecheck` (strict) | Style, lints, types |
| Engine unit | `cargo test -p nexus-core --lib` | Segment splitting, byte map accounting, limiter, SQLite store, HTTP parsing |
| Engine end-to-end | `cargo test -p nexus-core --tests` | Real downloads against a local server that can drop connections, return 503/416, ignore ranges, require cookies |
| UI unit | `pnpm test:web` | Formatting, speed-limit scale, store reducers |
| UI end-to-end | `pnpm test:e2e` | The built UI in Chromium against the simulated engine |
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
