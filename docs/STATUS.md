# Status

Updated: 2026-10-20
Phase: 1 (core and store); P1 complete, P2 next

## Shipped

- Cargo workspace and six crate skeletons; CI gates run on every push.
- `notes-proto`: UUIDv4 note and notebook ids, `Notebook`, and a versioned
  `Envelope<T>` shared by clients and the Worker.
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
- Web release build via `dx` 0.7.10; brotli budget script and a headless
  Chromium boot check at 360, 900 and 1400 px (S1 web).
- CI jobs for wasm tests and size, web build/budget, Linux desktop boot under
  Xvfb, Windows build and Android NDK compile. Run
  [39371140289](https://github.com/Protegee1505/notebox/actions/runs/39371140289)
  is green across all six jobs.
- ADR-0001, ADR-0002, ADR-0004, ADR-0005 and ADR-0006 record the spike and
  dependency decisions.
- Development disk: Cargo target, Rust/Cargo/npm/yarn/dx caches and Playwright
  browsers now live under `/tmp/opencode`; `scripts/relocate-caches.sh` repairs
  the layout and `scripts/budget-check` fails below 512 MiB free on `/home`.
  See `docs/DISK.md`.

## Measurements

| Check | Result |
| --- | --- |
| Native tests | 73 passed |
| Search, 10,000 notes | seeded in 14,534 ms; search 4 ms (budget 100 ms), 1 hit |
| 3-store convergence | random updates, metadata changes and sync order; identical text, metadata and history |
| 3-replica core convergence | random edits, drops and random sync order |
| Web bundle (release, brotli) | 390,856 bytes (0.37 MiB); wasm about 379 KB |
| Loro wasm delta (brotli) | 712,105 bytes (0.68 MiB), budget 1.5 MiB |
| 10,000 edits | 46,000 chars; snapshot 998 B; updates 46,106 B |
| Browser SQLite (S4) | 1,000 notes across reload: Chromium 760/646 ms, Firefox 339/175 ms, WebKit 188/163 ms (seed/reload) |
| Browser DB size | 122,880 B for 1,000 notes; rollback journal, one worker connection |
| SQLite wasm (release) | 1,783,762 B raw; 510,738 B brotli; 679,936 B gzip |
| wasm check | `notes-proto`, `notes-core`, `notes-store` compile for `wasm32-unknown-unknown` |
| Loro wasm smoke (Node) | 1 passed |
| Text-box convergence | 1,000 randomized whole-value edits |
| Linux desktop boot (CI) | stayed up 20 s under Xvfb |
| Windows desktop build (CI) | `notebox.exe` built and uploaded |
| Android headless compile (CI) | NDK r27c, four crates |

## Blocked

- **Cloudflare**: no `wrangler login` on this machine. S3 needs a deployed
  Worker for the 10 ms CPU check and the Argon2 timing; P2 deploy is blocked
  for the same reason.
- **Android runtime**: no `/dev/kvm` and no Android SDK, so S2/S5 cannot be run
  on a device or emulator locally. The local `aarch64-linux-android` check now
  also lacks `aarch64-linux-android-clang`; CI still compiles the four crates
  with NDK r27c. The full Android app has not been built.
- **Cold starts**: Linux desktop boot is verified under Xvfb, but the 600 ms
  cold-start number is not measured. Windows is build-only; Android has no
  boot or cold-start number.
- **Browser store integration**: `SqliteStore` compiles for wasm, but only the
  raw S4 probe runs in a browser worker. Wiring the real store into the app
  worker and rerunning the OPFS check belongs to P4.

## Next

1. Commit P1 in small changes: ids/proto, note model and history, store and
   search, export/import with ADR-0006.
2. P2 server: D1 migrations, setup and login, sync endpoints, blobs and a
   local `wrangler dev --local` end-to-end test. Deploy to the free account
   once `wrangler login` is available.
3. P3 sync engine: token storage, push/pull with cursor, offline queue, retry
   with backoff and attachment transfer; converge two headless clients against
   the real Worker.
4. Wire `SqliteStore` into the browser worker for P4 and extend the OPFS probe
   to open the real schema.
5. S3: after `wrangler login`, deploy the Worker and measure CPU and Argon2.
6. Run S2/S5 on an Android emulator with `/dev/kvm` (CI or a physical device) to
   close the runtime gap, including Gboard composition.
