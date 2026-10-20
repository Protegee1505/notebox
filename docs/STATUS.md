# Status

Updated: 2026-10-20
Phase: 5 (history and trash); P1–P4 complete, P5 local complete; the
cross-device acceptance still waits on the browser sync transport

## Shipped

- `notes-app`: the P5 history panel over the editor. The timeline lists
  versions newest first with day, relative time, change count and device; the
  detail shows a unified diff against the current note or a read-only Markdown
  preview; restore writes the old body as a new edit. The palette has a
  "Note history" command. Versions group one device's changes when the gap is
  under five minutes, as PLAN specifies, so a short sitting is one version.
  Screenshots at 360, 900 and 1400 px in both themes are in
  `docs/screenshots/` (`p5-history-*`).
- `notes-app`: the P5 trash view. A trashed note can be restored from its card
  or the editor header, which returns to the active notes view. "Empty trash"
  expands an inline confirmation with the note count before erasing, and the
  empty state explains that deletes wait in the trash. Trash screenshots are
  `p5-trash-*` at 360, 900 and 1400 px in both themes.
- `notes-store`: schema v3 stores one random Loro peer id per database and
  applies it to every note and workspace document. Before this, reopening a
  note created a fresh peer, so autosaves looked like a new device and history
  split into fake versions. The file-reopen and autosave-grouping tests cover
  the fix.
- `notes-store`: a focused two-store test restores an older version on the
  second store, syncs the update back, and checks identical text and version
  ids. This proves restore is an append-only edit that travels through sync;
  the UI-level acceptance is still blocked on P7.
- P5 browser verification: a Playwright flow passes 26 checks (seed, autosave,
  timeline, diff, preview, restore, version retention, trash, confirm, cancel,
  restore and empty) with no console errors at 1400 px.
- `notes-app`: the P4 shell — design tokens with light, dark and system
  themes; a three-pane desktop layout that becomes two panes at 1100 px and
  one pane at 700 px; a top bar; a sidebar with views, notebooks, tags, theme
  and storage state; a Keep-style card grid/list; and a full-width settings
  screen. Screenshots at 360, 900 and 1400 px in both themes are in
  `docs/screenshots/` (`p4-web-*`, `p4-settings-*`, `p4-login-*`).
- `notes-app`: Markdown editor with bold, italic, strike, heading, checklist,
  bullet/numbered lists, quote, inline code, code block, link, find in note,
  rendered preview, autosave, undo/redo, word and character counts, tags,
  pin, archive, trash, restore, note colours and toast messages.
- `notes-app`: command palette (Ctrl K), keyboard shortcut sheet, global
  search, workspace shortcuts (Ctrl N/S/F/E/Z/1/2/3/,) and a visible Commands
  entry so touch devices can reach the palette.
- `notes-app`: local login screen (form complete; sync is wired in P7) and
  settings for appearance, layout, account, notebooks, storage, zip export,
  shortcuts and emptying the trash.
- `notes-app`: all UI actions go through `AppContext` and the `NoteStore`
  trait. Native uses the SQLite file in the XDG data directory; the web build
  uses `SqliteStore::open_in_memory()` and labels the store "Browser memory"
  until the OPFS worker lands.
- P4 fixes found in the browser: `futures-timer` now enables its
  `wasm-bindgen` backend instead of panicking on `std::time`; `web-time`
  supplies the clock; the Dioxus `logger` feature is off because its
  `tracing-subscriber` path also panics on wasm; palette and find inputs take
  focus explicitly; find no longer steals focus on the first keystroke; and
  the mobile top bar, wrapping toolbar and status row were repaired.
- P4 browser verification: a Playwright flow passes 15 checks (seed, create,
  autosave, preview, Markdown, palette filtering, search, find) with no
  console errors, and every screenshot viewport has no horizontal overflow.
- P4 desktop verification: `cargo build -p notes-app --features desktop` and a
  20-second Xvfb boot smoke (`exit 124`) pass locally; the local runner now has
  `libxdo-dev`, `xvfb` and `dbus-x11` installed.
- CI: the web job installs `clang` for the SQLite wasm build, and the gates
  job includes it with the desktop libraries. Run 39415175131 passed all eight
  jobs (gates, web, desktop, windows, wasm, android, server, s4-opfs); the web
  job measured 1,478,616 bytes brotli at 1.41 MiB against the 3.00 MiB limit.
- Cargo workspace and six crate skeletons; CI gates run on every push.
- `notes-proto`: UUIDv4 note and notebook ids, `Notebook`, and a versioned
  `Envelope<T>` shared by clients and the Worker.
- `notes-proto`: the v1 sync API — `AuthRequest`/`AuthResponse`, `SyncRequest`,
  `SyncResponse`, `BlobChunk`, `DeviceList`, `ErrorBody` and the plan's limits,
  with bytes encoded as standard base64.
- `notes-core`: `NoteDocument` on Loro 1.16.2 with create, edit, snapshot
  export/import, update export/import and time travel (S2).
- `notes-core`: Unicode-safe prefix/suffix text-box binding with 1,000-edit
  convergence and remote-cursor tests (S5).
- `notes-core`: note model and metadata (notebook, tags, pin, archive, color),
  trash and restore, title/preview extraction, and deterministic commit
  timestamps. Metadata lives in the Loro `meta` map; tags are field-level LWW.
- `notes-core`: history API with per-peer versions, text preview, diff against
  the current note and restore-as-a-new-edit, so history only grows.
- `notes-core`: workspace CRDT for creating, renaming, deleting and listing
  notebooks, with snapshot import/export.
- `notes-core`: sanitized Markdown rendering with raw HTML escaped and unsafe
  URL schemes neutralized.
- `notes-core`: zip export/import of Markdown files, a versioned JSON manifest,
  notebook metadata and content-addressed attachments (ADR-0006).
- `notes-store`: `NoteStore` trait and `SqliteStore` with schema v1, notes,
  metadata, notebooks, attachments metadata, FTS5 search, tag and notebook
  filters, sync cursor and per-note pushed frontier.
- `notes-store`: the same store compiles for native and `wasm32`; the S4 probe
  proves the underlying `rusqlite` + OPFS VFS stack in Chromium, Firefox and
  WebKit (ADR-0004).
- `notes-store`: 3-store property test with random updates, metadata changes
  and sync order; export/import updates existing notes instead of duplicating.
  Core also has a 3-replica metadata, trash and history convergence test.
- `notes-server`: `Storage` trait with an in-memory implementation and a D1
  implementation; handlers for setup, login, sync, attachment chunks, device
  list and revocation run above it and stay Worker-free.
- `notes-server`: one-time setup behind `SETUP_TOKEN`, constant-time login
  comparison against a SHA-256 of the client's Argon2id key, one token per
  device hashed in D1, exponential login backoff (1 s to 300 s) and a five
  minute `last_seen` throttle.
- `notes-server`: push-then-pull sync, 25 updates per push, 100-row pages with
  a `more` flag, own updates filtered while the cursor still advances, and a
  28-query worst case inside the free 50-query limit.
- `notes-server`: content-addressed attachment chunks, idempotent re-upload,
  `409` on metadata conflict, and BLOB storage in the second D1 database.
- `migrations/sync` and `migrations/blobs`: schema v1 for account, device,
  update log, blob metadata and blob chunks.
- `notes-store`: schema v2 adds `note.local_vv`, `last_pushed`, an integer sync
  cursor and the same frontiers on the notebook-list row. The offline queue is
  one query; a pulled update that arrives while the note was clean advances
  `last_pushed` so it is not pushed back. `save_note` commits pending edits so
  an edit cannot be stored in a shape the sync engine cannot export.
- `notes-store`: canonical version-vector encoding (ADR-0007), because Loro's
  own byte encoding of an equal vector is not stable enough for the queue's
  SQL byte comparison.
- `notes-sync`: Argon2id login key (19 MiB, t=2, p=1) with a pinned known
  answer; the passphrase and the key never reach the server.
- `notes-sync`: `SyncTransport` trait and a blocking `ureq` (rustls) HTTP
  transport for native builds; a browser transport follows in P7. Session
  stores: memory, file (mode 0600, atomic rename) and a null store for the
  future cookie-based web client.
- `notes-sync`: sync engine with one push batch of at most 25 updates, paged
  pulls until the server is drained, an append-only offline queue, pull cursor
  saved with the updates it covers, notebook list as the reserved
  `workspace_note_id` document, and no mark recorded before the server answers.
- `notes-sync`: retry with 1/2/4/8 s backoff, server `retry-after` honoured up
  to 300 s, attachment chunks of 1 MiB with SHA-256 verification after
  download, and `HashMismatch` when a server returns different bytes.
- Two-client tests on the real handler stack with injected faults: a request
  that never arrives, a response lost after the server applied it, `429` with
  `retry-after`, and corrupted attachment chunks. Every case converges without
  losing or duplicating text.
- `scripts/sync-e2e.sh` boots `wrangler dev --local`, applies both migrations
  and runs `crates/notes-sync/tests/worker_e2e.rs`: two headless clients
  converge after offline edits, trash and restore, rename a notebook, transfer
  an attachment and revoke a device against real D1. Added to the `server` CI
  job.
- Worker adapter over the same handlers: `worker::Router`, bearer token or
  `HttpOnly; Secure; SameSite=Strict` cookie, `no-store` JSON envelopes and
  stable error codes. `wrangler.toml` builds `crates/notes-server` with
  `worker-build`.
- `scripts/server-e2e.mjs`: 48 HTTP checks against a real Worker under
  `wrangler dev --local` with both local D1 databases — setup refusals, cookie
  and bearer auth, push/pull ordering and pagination, push limits, chunk
  round-trip, revocation and login backoff.
- `scripts/free-tier-sim.py`: models a heavy day and fails when a daily limit
  exceeds 10%, plus per-request and row-size checks. Limits re-read from the
  Cloudflare docs on 2026-10-20.
- CI: the `server` job runs wasm clippy, caches `worker-build`, exercises the
  HTTP API with `scripts/server-e2e.mjs` and converges two headless clients with
  `scripts/sync-e2e.sh`; the free-tier simulator runs in the gates job.
- Web release build via `dx` 0.7.10; brotli budget script and a headless
  Chromium boot check at 360, 900 and 1400 px (S1 web).
- CI jobs for wasm tests and size, web build/budget, Linux desktop boot under
  Xvfb, Windows build, Android NDK compile and the server job. Run
  [39385729456](https://github.com/Protegee1505/notebox/actions/runs/39385729456)
  is green across all eight jobs, including the client sync step in `server`.
- ADR-0001, ADR-0002, ADR-0004, ADR-0005, ADR-0006 and ADR-0007 record the
  spike, dependency and encoding decisions. `docs/API.md` documents the v1
  endpoints the P3 engine implements.
- Development disk: Cargo target, Rust/Cargo/npm/yarn/dx caches and Playwright
  browsers now live under `/tmp/opencode`; `scripts/relocate-caches.sh` repairs
  the layout and `scripts/budget-check` fails below 512 MiB free on `/home`.
  See `docs/DISK.md`.

## Measurements

| Check | Result |
| --- | --- |
| Native tests | 155 passed, 1 skipped (the ignored Worker client e2e) |
| Search, 10,000 notes | seeded in 14,534 ms; search 4 ms (budget 100 ms), 1 hit |
| 3-store convergence | random updates, metadata changes and sync order; identical text, metadata and history |
| 3-replica core convergence | random edits, drops and random sync order |
| Web bundle (release, brotli) | local P5 1,545,831 B (1.47 MiB), limit 3.00 MiB; raw 6,485,710 B. P4 was 1,479,074 B local / 1,478,616 B CI |
| P5 browser flow (Playwright) | 26/26 checks: autosave, timeline, diff, preview, restore, version retention, trash, confirm, cancel, restore and empty |
| P5 layout review | 360/900/1400 px, light and dark; no horizontal overflow, no console errors |
| Restore across stores | older version restored on store B reached store A after one update exchange; both reported the same three version ids |
| P4 browser flow (Playwright) | 15/15 checks: seed, create, autosave, preview, Markdown, palette, search and find |
| P4 layout review | 360/900/1400 px, light and dark; no horizontal overflow, no console errors |
| Loro wasm delta (brotli) | 712,105 bytes (0.68 MiB), budget 1.5 MiB |
| 10,000 edits | 46,000 chars; snapshot 998 B; updates 46,106 B |
| Browser SQLite (S4) | 1,000 notes across reload: Chromium 760/646 ms, Firefox 339/175 ms, WebKit 188/163 ms (seed/reload) |
| Browser DB size | 122,880 B for 1,000 notes; rollback journal, one worker connection |
| SQLite wasm (release) | 1,783,762 B raw; 510,738 B brotli; 679,936 B gzip |
| Worker wasm (release, wasm-opt) | 644,372 B raw; 177,142 B brotli; limit 64 MiB |
| Server end-to-end | 48 checks green against `wrangler dev --local` and local D1 |
| Client sync, in-process | 14 two-client tests on the real handler stack; lost response, dropped request, `429` and corrupt chunks all converge |
| Client sync, real Worker | 1 test / 24 assertions green through `wrangler dev --local` and D1 in about 2.4 s |
| Argon2id login key | 0.05 s per derivation (release build, this machine); debug build 0.61–0.66 s |
| Offline queue | 30 new notes: 25 pushed in one run, 5 in the next; queue empty after the server accepts |
| Attachment round trip | client-to-client 2.5 MiB / 3 chunks in-process; 150 KB through real D1 with bytes and metadata identical |
| Free-tier heavy day | 2,158 requests (2.16% of 100k), 5,443 rows read (0.11%), 1,314 rows written (1.31%) |
| Worst-case request | 28 of 50 queries on a 25-update push; 5 bound parameters; base64 update row 1.37 MB of the 2 MB row limit |
| wasm check | `notes-proto`, `notes-core`, `notes-store`, `notes-sync` compile for `wasm32-unknown-unknown`; `notes-server` clippy-clean for wasm |
| Loro wasm smoke (Node) | 1 passed |
| Text-box convergence | 1,000 randomized whole-value edits |
| Linux desktop boot (CI) | stayed up 20 s under Xvfb; P4 local build also stayed up 20 s (`exit 124`) |
| Windows desktop build (CI) | `notebox.exe` built and uploaded |
| Android headless compile (CI) | NDK r27c, four crates |

## Blocked

- **Cloudflare**: no `wrangler login` on this machine. S3 needs a deployed
  Worker for the 10 ms CPU check; the P2 remote migrations, `SETUP_TOKEN`
  secret and deploy are blocked for the same reason. The exact commands are in
  README, "Deploying the free Worker".
- **Android runtime**: no `/dev/kvm` and no Android SDK, so S2/S5 cannot be run
  on a device or emulator locally, and the P4 Android screenshot/boot check is
  still missing. The local `aarch64-linux-android` check now also lacks
  `aarch64-linux-android-clang`; CI still compiles the four crates with NDK
  r27c. The full Android app has not been built, and the Argon2id timing on a
  mid-range phone is not measured.
- **Cold starts**: Linux desktop boot is verified under Xvfb, but the 600 ms
  cold-start number is not measured. Windows is build-only; Android has no
  boot or cold-start number.
- **P5 cross-device acceptance**: restore itself is proven across two stores in
  `sync_convergence`, but the app has no account/sync screen wired to the
  engine and the web build uses a per-tab in-memory database. "A version
  restored on one device appears on another within a minute" cannot be
  demonstrated in the running app until P7.
- **Browser persistence**: P4 runs the real `SqliteStore` schema through
  `open_in_memory()` in the browser, so a reload starts from the seed notes.
  The OPFS-backed worker and the browser sync transport remain for P7; until
  then the UI labels the web store "Browser memory".

## Next

1. P6 attachments: paste, drop and file picker, upload and lazy download,
   gallery, then Android camera and WebP compression.
2. Wire the OPFS-backed `SqliteStore` worker and browser sync transport in
   P7, replacing `open_in_memory()` and extending the S4 probe to the real
   schema. Then rerun the P5 acceptance with two browser profiles: restore on
   one, see the text on the other within a minute.
3. S3 after `wrangler login`: deploy, create the account, measure request CPU
   with `wrangler tail` and write `docs/adr/0003-worker-free-tier.md`.
4. Run S2/S5 on an Android emulator with `/dev/kvm` (CI or a physical device) to
   close the runtime gap, including Gboard composition.
