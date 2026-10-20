# ADR-0009: App sync wiring and browser sessions

- **Status:** accepted
- **Date:** 2026-10-20
- **Update:** ADR-0010 adds browser persistence; the web build no longer ends
  a reload with only an in-memory store.
- **Phase:** P7 app sync wiring (packaging remains P7)
- **Owner:** app + sync

## Question

Can the existing sync engine be driven from the running Dioxus app on native
and web without blocking the UI, losing an edit during a merge, or keeping a
second copy of the session in browser script? The slice fails if a save can
overwrite a body the engine just pulled, if sign-out survives only until a
reload, or if a sync request stalls the editor.

## Context

- Dioxus is pinned to 0.7.10. Native storage is SQLite through `rusqlite`;
  the web build still uses the in-memory SQLite store because the OPFS worker
  has not been wired into the app yet (S4 proved the VFS separately).
- The Worker runs on the free plan: 10 ms CPU per request, no password
  hashing or CRDT merging on the server. `notes-sync` already implements the
  push/pull cursor, retries and attachment transfer against `NoteStore` and
  `SyncTransport`.
- The web build is served by the same Worker as `/v1/*` through `[assets]`.
  CORS is deliberately absent, so a page on another origin cannot use the
  session. Local `wrangler dev` is plain HTTP, where a `Secure` cookie is
  dropped.
- The editor's autosave writes the whole draft through the note's Loro
  document; a merge can change the stored body while a run is in flight.

## Decision

1. **One platform transport, chosen at runtime.** `notes-sync` gained a
   browser `WebTransport` and a `BoxedTransport`; the app names
   `SyncClient<BoxedTransport>` on every target. The web transport uses
   same-origin `fetch` with credentials included. The token lives in memory
   only; the `HttpOnly; SameSite=Strict` cookie is what survives a reload.
   `SESSION_COOKIE` is `Secure` only when the request arrived over HTTPS.
2. **Sign-out clears the cookie.** `POST /v1/auth/logout` answers `204` with
   an expired cookie. It does not revoke the device token; that stays the
   device list's job.
3. **No engine work on the UI thread.** Native login, sync and device calls
   run on named worker threads with their own SQLite connection and report
   through `std::sync::mpsc`; the existing 700 ms tick polls. Web calls run in
   `spawn_forever`, not `spawn`: a task spawned from the login or Settings
   scope is cancelled when that screen unmounts, which left the status stuck
   on "Syncing" in the first browser run. `pollster` drives the `?Send` engine
   future on native.
4. **Autosave before a run, and a barrier during it.** `start_sync` saves the
   open draft first. While a run is in flight, a save for the note that was
   open at the start is held in `Runtime::deferred_save` instead of written
   over a possible pull. When the report arrives, `plan_open_note` compares
   `(base, draft, merged)`:
   - merged == base: the held edit is saved if that note is no longer open;
   - clean editor: the merged body replaces it;
   - both sides changed: the local body becomes a new note whose first line
     is `Conflict copy of <title>` and which carries a `conflict` tag; the
     merged body stays in the original note.
   A note the user switched to during the run is re-read when its editor is
   clean. A failed run preserves a held edit the same way rather than
   assuming a partial pull did or did not land. The conflict message is raised
   after the run summary, so the copy is not hidden by a later toast. A
   successful copy also asks for a follow-up run after the five second floor,
   so it reaches the Worker before the minute timer.
5. **Schedule and status.** `is_due` implements PLAN's loop: on start, on
   focus, three seconds after the last save, and every 60 seconds while
   visible. A 5 second floor collapses boot/focus/save into one request;
   failures use the engine's exponential backoff and the server's
   `retry-after`; a hidden page and a dirty editor never run. Login starts the
   first sync immediately instead of showing "Synced" before a run. The top
   bar and Settings show Local, Connecting, Syncing, Synced, Offline, Sign in
   or Problem, plus the last run and the device list.
6. **No demo seed into an account.** An empty database with a saved account
   skips the welcome notes and waits for the first pull. Without this, the
   in-memory web store would push a fresh set of demo notes on every reload.

## Evidence

- `cargo check` / `clippy -D warnings`: workspace native plus `wasm32` for
  `notes-app`, `notes-sync` and `notes-server`.
- `cargo nextest run --workspace`: 201 passed, 1 skipped (the ignored Worker
  client e2e). New coverage: scheduling and open-note plans in `notes-app`,
  the attachment work queue in `notes-store`, and the browser URL rules in
  `notes-sync`.
- `scripts/server-e2e.mjs`: 51 checks against the local Worker and D1,
  including the `204` logout response and expired `HttpOnly` cookie.
- `scripts/sync-e2e.sh`: two headless clients converge against the local
  Worker and D1 in 3.23 s, including an attachment and a revoked device.
- A release `dx build` served by `wrangler dev --local` passes a Playwright
  flow of 22 checks (login, first sync, last-sync label, devices, current
  marker, two-click revoke, manual sync, sign-out, account clearing, reload
  stays local, login reopens) and a two-device conflict flow of 13 checks
  with a third puller (delayed sync, edit during the run, conflict toast,
  merged original, `conflict`-tagged copy holding the typed text, follow-up
  sync before the minute timer, copy visible on the third device), with no
  application console errors. The browser run is what found the cancelled
  `spawn` task.
- `node scripts/web-budget.mjs`: 1,786,340 B brotli (1.70 MiB) against the
  3.00 MiB first-load limit; raw 7,457,436 B.

## Consequences

- Browser persistence is still missing. The web app logs in, syncs and shows
  status, but a reload starts from an empty in-memory store and pulls the
  account again. The OPFS worker is the next slice and is not silently
  replaced by IndexedDB.
- The app-level run used the web build. Native login/sync threads share the
  same `AppContext` methods and engine tests, but a native UI smoke test
  against a live Worker is still pending; it can ride with the packaging CI
  item.
- A conflict creates a note instead of a merge dialog. Copies are tagged
  `conflict`; no body is discarded, and a CRDT merge still keeps both sides
  of the original note.
- The web app cannot be developed with `dx serve` against the Worker because
  that origin is cross-origin. Build with `dx build` and open the Worker
  origin. A future dev proxy could remove this friction without adding CORS
  to production.
- Each device list view costs one request; the app fetches it when Settings
  opens or the refresh button is pressed, not on every sync.
- Sign-out on the web sends the logout request without waiting so the UI
  clears immediately. An offline logout can leave the cookie in the browser;
  the account record is gone, so the app does not restore the session on its
  own. Revoking the device from another client is the hard stop.
- `PLAN.md`'s P7 packaging work (installers, tagged artifacts, offline cache)
  is unchanged and still follows this slice.

## Update

ADR-0010 closes the browser-persistence gap described above. This record's
context and consequences describe the app-sync slice before that change.
