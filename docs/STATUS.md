# Status

Updated: 2026-10-21
Phase: 7 (installable web app and tagged release workflow; first tagged
release remains); P1–P6 complete. The running app can sign in, restore a
session, sync notes and attachments, show status, list and revoke devices, and
keep a conflict copy. The web build is served by the Worker and syncs
same-origin; local notes, history and images persist in OPFS, with no account
needed. The web build is now installable and works offline after one visit,
and a waiting deployment updates only when the user chooses Reload. The
release workflow builds Linux, Windows, Android and web artifacts when a
version tag is pushed; it has not run yet because the Android signing key is an
owner action.

## Shipped

- `notes-app`: installable web app. `index.html` registers a root-scoped
  service worker and publishes an update bridge; `manifest.webmanifest`,
  desktop and maskable icons, and `_headers` are part of the release build.
  The worker precaches only the current hashed assets from the Dioxus asset
  manifest, serves navigations network-first with an offline shell fallback,
  and never intercepts `/v1/*`, `/sw.js` or `/version.json`. New deployments
  wait; the banner's Reload saves and flushes the open note, activates the
  worker, and reloads. Later dismisses it for the session.
- `notes-app`: About shows the package version plus `NBX_BUILD_ID` and the
  short `NBX_GIT_SHA`; local builds say `dev`. CI stamps the run number and
  commit.
- `scripts/pwa-build.mjs`: post-build PWA pipeline. Copies the manifest and
  icons, precaches the current assets only, writes a content-hashed build id
  into `sw.js`, and writes `version.json`. `scripts/web-build.sh` wraps a clean
  `dx build` and the PWA step; the deploy and CI commands use it.
- `scripts/pwa-check.mjs`: 12-check Chromium flow for manifest and icon
  validity, empty `Page.getInstallabilityErrors`, service-worker control,
  offline reload, an offline edit across another reload, a simulated second
  build showing the update banner, cache replacement on Reload, the note still
  open after the update, no `/v1/*` requests and no console errors. It copies
  the build before simulating the update, so the checked artifact is not
  changed. `NBX_PWA_SCREENSHOTS=1` adds six layout captures.
- `.github/workflows/ci.yml`: the web job builds through `web-build.sh`, runs
  both browser flows, and uploads the release output including the PWA files.
- `.github/workflows/release.yml`: a `v*` tag whose name matches the
  `notes-app` version runs the gates and builds Linux `.deb` and `.AppImage`,
  Windows `.msi`, NSIS setup `.exe` and portable `.exe`, an arm64 Android APK
  signed from CI secrets, and the installable web `.zip`. It installs and boots
  the Linux packages and the Windows MSI on clean runners, verifies the APK
  with `apksigner` and `aapt`, runs the PWA checks, writes `SHA256SUMS` and
  publishes a GitHub Release. Missing Android signing secrets fail the job
  before it builds; it never publishes an unsigned or debug APK. The workflow
  has not run yet.
- `Dioxus.toml`: bundle identifier, publisher, icon, category, description and
  Debian runtime dependencies for `dx bundle`. The Android signing block is
  added only on the CI runner from secrets and restored afterward.
- `docs/adr/0011-installable-web-app.md` records the precache, update and
  cache-header decisions and the offline/update evidence.
- `notes-app`: P7 sync wiring. The running app signs in, restores a saved
  session, saves the open draft before each run, and syncs on start, focus,
  three seconds after an edit and every 60 seconds while visible, with a five
  second floor and the server's backoff. Native login/sync/device calls run on
  named threads with their own SQLite connection; browser calls run in
  `spawn_forever` tasks. The top bar and Settings show Local / Connecting /
  Syncing / Synced / Offline / Sign in / Problem and the last run; Settings
  lists devices, marks the current one and revokes another after a second
  click. A permanent error stops automatic retries until Sync now.
- `notes-app`: conflicts never overwrite. A save for the note open when a run
  starts is held until the report, then `plan_open_note` compares the base,
  the editor and the merged body. A clean editor takes the merged body; an
  edit made during the run becomes a new note titled `Conflict copy of ...`
  with the `conflict` tag, while the original takes the merged body. A note
  the user switches to during a run is refreshed when clean, and a failed run
  keeps a held edit in the same way. A successful copy asks for a follow-up
  run after the five second floor, so it does not wait for the minute timer.
- `notes-sync`: browser `WebTransport` over same-origin `fetch` with a 30 s
  abort, `BoxedTransport` for one `SyncClient<BoxedTransport>` alias on every
  target, and a no-op `logout` default. The token stays in memory; the
  `HttpOnly; SameSite=Strict` cookie carries the session across reloads and is
  `Secure` only on HTTPS.
- `notes-server`: `POST /v1/auth/logout` expires the cookie with `204`; the
  Worker serves the Dioxus build through `[assets]`, so browser requests are
  same-origin and no CORS surface is added.
- `notes-app`: a saved account makes an empty database wait for the first
  pull instead of seeding demo notes, which would otherwise push a fresh set
  on every in-memory web reload.
- `notes-store`: `notes_with_attachment_work` returns notes that need an
  upload or a missing download, so one run moves up to eight notes' images.
- `notes-store`: browser database images. `to_image()` writes `NBX1` plus a
  normal SQLite file; `from_image()` rejects a foreign or empty image before
  running the normal migrations; `change_count()` exposes
  `Connection::total_changes`. A round-trip test covers notes, metadata,
  notebooks, three history versions, FTS search, attachment links and bytes,
  the sync cursor, `last_pushed` and the device peer. Bad headers are rejected.
- `notes-app`: browser persistence. The web build loads the saved image from
  `notebox.db` in OPFS, keeps SQLite in memory, and writes a new image from the
  700 ms tick when `change_count` moved. `opfs_store.js` asks for persistent
  storage and holds the exclusive `notebox-opfs-store-v1` Web Lock, so a second
  tab shows "Notebox is already open" instead of overwriting the first. A
  corrupt image stops on a fatal screen without replacing it; missing OPFS
  falls back to a clearly labelled "Browser memory". Settings shows "Browser
  storage" when the image is live. The cookie/restore and sync cursor paths are
  unchanged, and no new server request is made.
- `scripts/web-persist.mjs`: release-build Playwright check with a persistent
  Chromium profile. It creates a local-only note, attaches a PNG, waits for the
  WebP row, closes and reopens the browser, and finds the text, image reference,
  attachment row and decoded `data:image/webp` bytes. It also checks the second
  tab lock, the history panel, the Settings label after restart and a
  synthetic page-hide after a just-typed edit. 13/13 checks pass with no
  console errors.
- `docs/adr/0010-browser-persistence.md` records the image, lock and fallback
  decisions. It amends ADR-0004's runtime shape: the app keeps the synchronous
  `NoteStore` for v1, and revisits a worker-backed store if image writes grow
  past roughly 100 ms.
- P7 browser verification: a Playwright flow passes 22 checks (login, first
  sync, last-sync label, devices, current marker, two-click revoke, manual
  sync, sign-out, account clearing, reload stays local, login reopens) and a
  two-device conflict flow passes 13 (delayed sync, edit during the run,
  conflict toast, merged original, tagged copy with the typed text, follow-up
  sync before the minute timer, copy pulled by a third device).
  Screenshots are `docs/screenshots/p7-settings-synced-1400-light.png`,
  `p7-settings-local-1400-light.png`, `p7-login-1400-light.png` and
  `p7-conflict-1400-light.png`.
- `docs/adr/0009-app-sync-wiring.md` records the transport, threading,
  scheduling, conflict and cookie decisions, including the `spawn_forever`
  scope fix found by the browser run.
- `notes-app`: P6 image ingest. The file picker, drag and drop and a page-level
  paste listener all route through one path: read the file, apply EXIF
  orientation, cap the longest side at 1600 px, encode lossy WebP at quality
  80/68/56, and store bytes content-addressed. A 951,612 B 1200×700 PNG
  became a 29,450 B WebP in 1.3 s in a release web build. Unsupported files
  get a readable toast and leave no row.
- `notes-app`: the attachment strip under the editor lists the note's images
  with name, kind and size, offers Insert at the caret, and shows a Waiting
  placeholder for a link whose bytes have not arrived. The rendered preview
  resolves content hashes to cached inline `data:image/webp;base64` URLs;
  note cards show the image's alt text instead of a raw Markdown URL.
- `notes-store`: schema v4 stores attachment bytes in the local SQLite file,
  deduplicates by SHA-256, indexes note links, creates placeholders for links
  without bytes, tracks the upload queue, measures usage, and carries bytes
  through export/import. Removing a Markdown reference only drops the link,
  never the bytes. Attachment tests cover dedupe, placeholders, the queue,
  export/import and the v3 migration.
- `notes-core`: content-addressed attachment paths (`attachments/<hash>.<ext>`),
  MIME/extension helpers, safe names, reference scanning, and a Markdown image
  resolver that accepts only `data:image/` responses. The plain-text preview
  replaces image syntax with alt text.
- `notes-sync`: `SyncClient::sync_note_attachments` uploads local bytes and
  downloads missing placeholder bytes with the existing SHA-256 check. A
  two-store test creates a note with a photo on one device, syncs the note,
  sees a placeholder on the second, downloads the photo, and confirms the
  bytes and preview are available; a broken link remains Waiting and does not
  stop the pass.
- P6 browser verification: a Playwright flow passes 26 checks (picker, WebP,
  autosave, preview decode, dedupe, paste, drop, Insert, unsupported file,
  placeholder, themes and all three widths). Screenshots at 360, 900 and
  1400 px in light and dark are in `docs/screenshots/` (`p6-editor-*`), with
  no horizontal page overflow.
- `notes-app`: fixed two async bindings found in P6. Dioxus `eval().recv()`
  waits for `send`, so toolbar marks and image insertion now use `join()`;
  the Dioxus web file reader hangs or panics, so the browser reads a `File`
  through `web-sys` directly.
- `docs/adr/0008-attachments.md` records the content-addressed, local-BLOB,
  pure-Rust WebP design and its limits.
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
  the UI-level acceptance waits on a deployed Worker.
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
- `notes-app`: local login screen (form complete; sync is wired) and
  settings for appearance, layout, account, notebooks, storage, zip export,
  shortcuts and emptying the trash.
- `notes-app`: all UI actions go through `AppContext` and the `NoteStore`
  trait. Native uses the SQLite file in the XDG data directory; the web build
  keeps SQLite in memory and persists a versioned image to OPFS, labelled
  "Browser storage" unless the browser cannot provide it.
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
  transport for native builds; the browser transport is in P7. Session
  stores: memory, file (mode 0600, atomic rename) and a null store for the
  cookie-based web client.
- `Cargo.lock`: `rustls-webpki` 0.103.16 from 0.103.15, fixing
  RUSTSEC-2026-0382 in the native sync TLS path; ADR-0013 records the
  lockfile-only update. Dioxus remains pinned at 0.7.10.
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
  spike, dependency and encoding decisions; ADR-0008 covers attachments,
  ADR-0009 sync wiring, ADR-0010 browser persistence, ADR-0011 the installable
  web app, ADR-0012 tagged release artifacts and ADR-0013 the rustls-webpki
  security patch. `docs/API.md` documents the v1 endpoints the P3 engine
  implements.
- Development disk: Cargo target, Rust/Cargo/npm/yarn/dx caches and Playwright
  browsers now live under `/tmp/opencode`; `scripts/relocate-caches.sh` repairs
  the layout and `scripts/budget-check` fails below 512 MiB free on `/home`.
  See `docs/DISK.md`.

## Measurements

| Check | Result |
| --- | --- |
| Native tests | 206 passed, 1 skipped (the ignored Worker client e2e) |
| Search, 10,000 notes | seeded in 14,534 ms; search 4 ms (budget 100 ms), 1 hit |
| 3-store convergence | random updates, metadata changes and sync order; identical text, metadata and history |
| 3-replica core convergence | random edits, drops and random sync order |
| P6 browser flow (Playwright) | 26/26 checks: picker, 29 KB WebP, autosave, preview decode, dedupe, paste, drop, Insert, unsupported file, placeholder, light/dark at 360/900/1400, no application console errors |
| P6 layout review | 360/900/1400 px, light and dark; no horizontal page overflow; the 900 px toolbar wraps instead of hiding Preview |
| P6 image ingest | 951,612 B 1200×700 PNG → 29,450 B WebP (3.1%), 1.3 s from picker to row in a release web build (headless Chromium) |
| P6 note + photo transfer | two stores, 180,000 B photo: note and link arrive first, placeholder shows, download fills bytes, second pass is a no-op; a missing blob is counted without failing |
| P7 browser flow (Playwright) | 22/22 checks: login, first sync, last sync, devices, current marker, two-click revoke, manual sync, sign-out, cookie/account clearing, reload, login reopen; no console errors |
| P7 installable web app (Playwright) | 12/12 functional checks: manifest and icons, empty installability errors, service-worker control, offline note and edit across reloads, second build waits for the banner, Reload replaces the cache, note survives, no `/v1/*` requests, no console errors; 6 layout captures with `NBX_PWA_SCREENSHOTS=1` |
| P7 PWA install/update layout | 360/900/1400 px, light and dark; banner wraps without horizontal overflow; screenshots `docs/screenshots/p7-pwa-*` |
| PWA bundle | 10 precached files; 32,610 bytes of manifest, icons, HTML and worker; pre-cache assets from the Dioxus manifest only |
| P7 browser persistence (Playwright) | 13/13 checks: Browser storage label, local-only note and PNG, WebP attachment, second-tab lock, browser restart, text/image/history, page-hide save, no console errors |
| P7 conflict flow (Playwright) | 13/13 checks with two device contexts plus a third puller: delayed sync, edit during the run, conflict toast, merged original, `conflict`-tagged copy holding the typed text, follow-up sync before the minute timer, copy pulled by another device |
| Web bundle (release, brotli) | P7 release build 1,818,284 B (1.73 MiB), raw 7,529,584 B, limit 3.00 MiB; P7 Dioxus build 1,792,565 B (1.71 MiB), raw 7,489,034 B; P6 manual build 1,955,393 B (1.86 MiB), raw 9,353,273 B; P5 CI was 1,545,831 B |
| Linux bundles | `notebox_0.1.0_amd64.deb` 9,867,218 B; `notebox_0.1.0_x86_64.AppImage` 89,597,120 B. The `.deb` installed and stayed up for 20 s under Xvfb; the extracted AppImage did the same. Both from `dx bundle` 0.7.10 with the patched lockfile |
| P5 browser flow (Playwright) | 26/26 checks: autosave, timeline, diff, preview, restore, version retention, trash, confirm, cancel, restore and empty |
| P5 layout review | 360/900/1400 px, light and dark; no horizontal overflow, no console errors |
| Restore across stores | older version restored on store B reached store A after one update exchange; both reported the same three version ids |
| P4 browser flow (Playwright) | 15/15 checks: seed, create, autosave, preview, Markdown, palette, search and find |
| P4 layout review | 360/900/1400 px, light and dark; no horizontal overflow, no console errors |
| Loro wasm delta (brotli) | 713,227 bytes (0.68 MiB), budget 1.5 MiB |
| 10,000 edits | 46,000 chars; snapshot 998 B; updates 46,106 B |
| Browser SQLite (S4) | 1,000 notes across reload: Chromium 760/646 ms, Firefox 339/175 ms, WebKit 188/163 ms (seed/reload) |
| Browser DB size | 122,880 B for 1,000 notes; rollback journal, one worker connection |
| SQLite wasm (release) | 1,783,762 B raw; 510,738 B brotli; 679,936 B gzip |
| Worker wasm (release, wasm-opt) | 644,372 B raw; 177,142 B brotli; limit 64 MiB |
| Server end-to-end | 51 checks green against `wrangler dev --local` and local D1, including logout and cookie expiry |
| Client sync, in-process | 14 two-client tests on the real handler stack; lost response, dropped request, `429` and corrupt chunks all converge |
| Client sync, real Worker | 1 test / 24 assertions green through `wrangler dev --local` and D1 in 3.23 s |
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
  on a device or emulator locally, and the P4/P6 Android screenshot, camera and
  boot checks are still missing. The local `aarch64-linux-android` check also
  lacks `aarch64-linux-android-clang`; CI still compiles the four crates with
  NDK r27c. The full Android app has not been built, and the Argon2id timing on
  a mid-range phone is not measured.
- **Release execution**: `.github/workflows/release.yml` has not run. The
  Android job needs the owner to create a keystore and set
  `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`,
  `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD`. The first `v*` tag will also
  exercise the clean-runner Linux and Windows install smoke jobs. The desktop
  update-available notice is not implemented; only the web build has one.
- **Cold starts**: Linux desktop boot is verified under Xvfb, but the 600 ms
  cold-start number is not measured. Windows is build-only; Android has no
  boot or cold-start number.
- **Human cross-device acceptance**: the app-side account, sync loop and
  browser transport are wired, and a two-device browser run passes the P7
  login, device, conflict and sign-out checks. The human P5/P6/P7 check across
  a desktop and a phone still waits on a deployed Worker and an Android
  runtime. The native desktop path is compile-checked and shares the engine
  tests, but this slice did not drive its login/sync threads through the UI
  against a live Worker.
## Next

1. P7 release execution: create and back up the Android keystore, set the four
   repository secrets, push a `v0.1.0` tag and fix any clean-runner smoke
   failure. Then add the desktop update-available notice (the web banner is
   done) and a fresh-machine README check. Do not publish an unsigned APK.
2. S3 after `wrangler login`: deploy the Worker with the P7 assets, create the
   account, measure request CPU with `wrangler tail` and write
   `docs/adr/0003-worker-free-tier.md`. Then run the human P5/P6/P7 acceptance
   on a desktop and a phone, including a photo, a version restore and an
   installed PWA update.
3. Run S2/S5/P6/P7 on an Android emulator with `/dev/kvm` (CI or a physical
   device) to close the runtime gap, including Gboard composition, the camera
   picker and the in-app login/sync loop.
4. Measure an OPFS image write on 1,000 notes and 50 images; if it exceeds
   roughly 100 ms, schedule the worker-backed `SqliteStore` from ADR-0004.
