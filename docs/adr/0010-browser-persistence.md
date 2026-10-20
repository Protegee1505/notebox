# ADR-0010: Browser persistence by OPFS database image

- **Status:** accepted
- **Date:** 2026-10-20
- **Phase:** P7 browser persistence
- **Owner:** app + store
- **Amends:** ADR-0004 (runtime shape)

## Question

How can the web build keep local-only notes, their history and attachment bytes
across a reload without making every `NoteStore` call asynchronous?

## Context

- ADR-0004 proved SQLite over the OPFS SAH-pool VFS in a worker with 1,000
  notes. Its runtime design put the whole `NoteStore` behind a worker, which
  would make `NoteStore`, `notes-sync` and the UI actions asynchronous.
- The current app and sync engine are synchronous on the UI thread. The
  browser build already opens a real v4 schema through
  `SqliteStore::open_in_memory()`; only persistence is missing.
- A signed-in reload pulls notes from the Worker, but local-only notes, an
  unsynced conflict copy and attachment bytes have no other copy. Losing them
  on reload is not acceptable.
- The store already has `SqliteStore::to_image` / `from_image` on top of
  `rusqlite`'s `serialize` feature. The in-memory connection uses the rollback
  journal, so one file image contains the whole database.
- `Connection::total_changes()` gives a cheap write counter without adding a
  dirty flag to every store method.
- OPFS file writes need a worker, but a blob image does not need the SAH-pool
  VFS. The worker is a small message bridge, not a second SQLite connection.

## Decision

1. **Persist one versioned image, not a VFS file.** `SqliteStore::to_image`
   returns `NBX1` followed by a normal SQLite file. `from_image` rejects a
   missing header and loads the body into a fresh in-memory connection before
   the normal migrations. The header is there so a corrupt or unrelated file
   fails visibly instead of being opened as SQLite.
2. **SQLite stays in memory on the UI thread.** The existing synchronous
   `NoteStore` and sync engine are unchanged. This deliberately supersedes
   ADR-0004's "all browser database work runs in a worker" clause for v1.
3. **A small worker owns `notebox.db` in OPFS.** `opfs_store.js` loads the
   image, writes it with `createWritable`, asks `navigator.storage.persist()`
   once and reports errors. Rust sends an `ArrayBuffer`; at most one image is
   in flight, and a write that happens during a save is sent on the next tick.
4. **Writes are detected, not annotated.** The 700 ms app tick compares
   `SqliteStore::change_count()` with the count of the last image. Save, pull,
   attachment and history writes all move the counter. A `pagehide` listener
   makes a best-effort final send.
5. **One writer per origin.** The worker holds the exclusive Web Lock
   `notebox-opfs-store-v1` for its lifetime. A second tab that cannot acquire
   it shows "Notebox is already open" and never writes. The lock is released
   when the tab (and its worker) goes away.
6. **Failure is visible and safe.**
   - OPFS, `getDirectory` or the worker unavailable: open memory, label it
     "Browser memory" and say notes reset on reload.
   - A saved image that fails `from_image`: show a fatal screen and keep the
     image untouched. Do not open a fresh database over it.
   - A save failing after boot: keep editing in memory, switch Settings to
     "Browser memory" and toast that an export is needed.
   - `persist()` false or denied: still save; the Settings copy warns that the
     browser may evict the data.
7. **The server is untouched.** No new requests, no D1 changes, no paid
   storage. The cookie/session restore path and the sync cursor stay in the
   image, so a signed-in reload resumes sync.

## Evidence

- `crates/notes-store/tests/image.rs` round-trips notes, metadata, notebooks,
  three history versions, FTS search, attachment links and bytes, the sync
  cursor, `last_pushed` and the device peer. Bad headers are rejected.
- `scripts/web-persist.mjs` drives a persistent Chromium profile in a release
  build: create a local-only note, attach a PNG, wait for the WebP row, close
  the browser, reopen the same profile, and find the text, image reference,
  attachment row and decoded `data:image/webp` bytes. It also opens a second
  tab and checks the lock screen, then checks the history panel and the
  Settings label. A synthetic page-hide after a just-typed edit proves the
  save-and-flush path. All 13 checks pass with no console errors.
- The unit and browser tests prove the image path, not a timing budget yet.
  The image is a full copy of the SQLite file, so the cost grows with note
  count and attachment bytes.

## Consequences

- The UI and sync engine keep their synchronous API. The image copy is the
  price paid for that simplicity.
- Every write eventually saves the whole database. The 700 ms tick and one
  in-flight image keep this bounded, but a large database makes each save
  larger. Revisit a true worker-backed `SqliteStore` if a flush on 1,000
  notes plus 50 images exceeds roughly 100 ms.
- The worker is a blob bridge, not the S4 SAH-pool VFS. The S4 result still
  stands as proof that OPFS and SQLite work in Chromium, Firefox and WebKit;
  the app-level browser test currently runs in Chromium.
- A second tab cannot edit. That is a deliberate trade for a single image and
  a simple conflict story; cross-tab editing can be designed later.
- The exported Markdown zip remains the backup path. `navigator.storage.persist`
  reduces eviction risk but cannot promise it.
