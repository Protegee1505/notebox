# Status

Updated: 2026-10-20
Phase: 3 (sync); P1 and P2 complete, P3 complete locally, deploy and S3 next

## Shipped

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
  [39379251864](https://github.com/Protegee1505/notebox/actions/runs/39379251864)
  is green across all eight jobs, including `server`.
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
| Native tests | 133 passed, 1 skipped (the ignored Worker client e2e) |
| Search, 10,000 notes | seeded in 14,534 ms; search 4 ms (budget 100 ms), 1 hit |
| 3-store convergence | random updates, metadata changes and sync order; identical text, metadata and history |
| 3-replica core convergence | random edits, drops and random sync order |
| Web bundle (release, brotli) | 390,856 bytes (0.37 MiB); wasm about 379 KB |
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
| Linux desktop boot (CI) | stayed up 20 s under Xvfb |
| Windows desktop build (CI) | `notebox.exe` built and uploaded |
| Android headless compile (CI) | NDK r27c, four crates |

## Blocked

- **Cloudflare**: no `wrangler login` on this machine. S3 needs a deployed
  Worker for the 10 ms CPU check; the P2 remote migrations, `SETUP_TOKEN`
  secret and deploy are blocked for the same reason. The exact commands are in
  README, "Deploying the free Worker".
- **Android runtime**: no `/dev/kvm` and no Android SDK, so S2/S5 cannot be run
  on a device or emulator locally. The local `aarch64-linux-android` check now
  also lacks `aarch64-linux-android-clang`; CI still compiles the four crates
  with NDK r27c. The full Android app has not been built, and the Argon2id
  timing on a mid-range phone is not measured.
- **Cold starts**: Linux desktop boot is verified under Xvfb, but the 600 ms
  cold-start number is not measured. Windows is build-only; Android has no
  boot or cold-start number.
- **Browser store integration**: `SqliteStore` compiles for wasm, but only the
  raw S4 probe runs in a browser worker. Wiring the real store into the app
  worker, the browser sync transport and rerunning the OPFS check belong to P4
  and P7.

## Next

1. P4 UI: app shell, editor, login and settings against the core traits; wire
   `SqliteStore` into the browser worker and extend the OPFS probe to the real
   schema.
2. Push the P3 commits and confirm the `server` job, including the new client
   sync step, is green on GitHub.
3. S3 after `wrangler login`: deploy, create the account, measure request CPU
   with `wrangler tail` and write `docs/adr/0003-worker-free-tier.md`.
4. Run S2/S5 on an Android emulator with `/dev/kvm` (CI or a physical device) to
   close the runtime gap, including Gboard composition.
