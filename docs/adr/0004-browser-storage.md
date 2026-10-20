# ADR-0004: Browser storage — SQLite over OPFS

- **Status:** proposed
- **Date:** 2026-10-20
- **Spike:** S4
- **Owner:** Core

## Question

Can the browser keep a real database: sqlite-wasm-rs with the OPFS SAH-pool VFS
in a dedicated worker, holding 1,000 notes across reloads in Chrome, Firefox and
Safari?

## Context

- `rusqlite` 0.40.2, `sqlite-wasm-rs` 0.5.5,
  `sqlite-wasm-vfs` 0.2.0 (`sahpool`), `wasm-bindgen` 0.2.129.
- Rust 1.99.1, `wasm32-unknown-unknown`; clang 18 compiles the bundled SQLite C.
- The probe is `crates/notes-store/examples/opfs_spike.rs`, loaded from
  `crates/notes-store/web/worker.js`; `scripts/opfs-spike.mjs` drives it.
- Browsers: Chromium 156.0.8078.4, Firefox 157.0, WebKit 27.2 (Playwright
  1.64.1). WebKit stands in for Safari.
- `rusqlite` 0.40 pins `sqlite-wasm-rs ^0.5.1`, while `sqlite-wasm-vfs` 0.3
  requires 0.6. The working pair is therefore rusqlite 0.40 + VFS 0.2. The
  workspace pins `sqlite-wasm-rs` to 0.5 so only one copy of the C library and
  one VFS registry are linked.

## Decision

Use SQLite in the browser through `rusqlite`, with the OPFS SAH-pool VFS
installed inside a dedicated Web Worker. The same `rusqlite` API then serves
native and browser backends; only VFS installation is target-specific. The
IndexedDB-backed `NoteStore` fallback is not needed.

The VFS is installed with `clear_on_init = false`, the database stays in
rollback-journal mode (the pool does not support shared-memory WAL), and one
worker owns one connection at a time. OPFS requires a dedicated worker, not the
main thread.

## Evidence

`scripts/opfs-spike.mjs` seeds 1,000 rows, reloads the page and compares the
row count, total text length and first/last rows. Each browser uses a fresh
persistent profile, which also avoids WebKit's refusal to provide OPFS handles
in an ephemeral context.

| Browser | Seed + read | Reload + read | Rows after reload | DB bytes | Journal |
| --- | ---: | ---: | ---: | ---: | --- |
| Chromium 156 | 760 ms | 646 ms | 1,000 | 122,880 | delete |
| Firefox 157 | 339 ms | 175 ms | 1,000 | 122,880 | delete |
| WebKit 27.2 | 188 ms | 163 ms | 1,000 | 122,880 | delete |

All three report 87,000 characters of Unicode text (emoji and CJK included),
identical first and last rows, and no page or console errors. Timings include
worker startup, VFS installation and wasm instantiation, so they are upper
bounds for the database work itself.

The probe's release wasm is 1,783,762 bytes raw, **510,738 bytes brotli** and
679,936 bytes gzip, including SQLite, the OPFS VFS and rusqlite. A production
build will link the same store code and add it to the existing 0.37 MiB web
bundle; the 3 MB brotli budget has ample room.

## Consequences

- S4 passes on Chromium, Firefox and WebKit. No fallback is triggered.
- All browser database work runs in a worker. The UI talks to it over messages;
  the `NoteStore` trait stays synchronous inside the worker.
- The SAH-pool VFS cannot share a database between connections or use WAL. The
  store must keep one connection per worker and use the default rollback
  journal, as the probe does.
- `sqlite-wasm-rs` 0.5 is older than the 0.6 release. Revisit the pin when
  `rusqlite` moves to 0.6; do not mix 0.5 and 0.6 in one binary.
- The web bundle pays for SQLite once. The 3 MB brotli budget still has room,
  but `scripts/web-budget.mjs` must include the store when the real app links it.
- OPFS persistence is per origin and per browser profile. Private/ephemeral
  contexts may refuse handles, so the app needs a clear storage-unavailable
  error and the exported Markdown zip remains the backup path.
