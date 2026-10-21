# ADR-0018: Local preferences, a first-run gate and an optional welcome tour

- **Status:** accepted
- **Date:** 2026-10-21
- **Phase:** P8
- **Owner:** app

## Question

How should Notebox remember display choices across reloads, decide whether a
first launch is signed in, offline or brand new, and offer sample material
without recreating the duplicate-body merge seen on two devices?

## Context

- Theme and layout describe one screen on one device. They are not note data,
  are not useful to another device and must never enter the workspace document,
  the sync protocol or D1.
- Preferences were held only in `AppState`, so every reload went back to the
  system theme and the card grid. `localStorage` is available before the first
  render on web; native builds already keep `session.json` and `account.json`
  in the XDG data directory.
- A local database can predate any preference file. Those notes belong to the
  person who wrote them, so a missing flag must not turn an existing offline
  library into a login wall.
- The old bootstrap wrote three fixed-id welcome notes into every empty local
  database. Two devices syncing those same ids into one Loro document could
  both insert their bodies into the same text container, producing duplicated
  text that no user edit had made.
- The search index is a local SQLite mirror. Adding tags to it changes
  `PRAGMA user_version`, but it does not change note snapshots, the Worker or
  the sync messages.

## Decision

1. **Preferences are device-local.** `theme`, `layout` and `offline` live in
   `preferences.json` under the native data directory and in
   `notebox.preferences.v1` in browser `localStorage`. They are loaded before
   the first render, written on each change, and are absent from exports,
   backups, note documents and the sync protocol. A missing or unreadable file
   falls back to the defaults.
2. **The first screen is a gate, not a seeded app.** After storage opens:
   a saved account, `offline = true`, or any local note opens the note list;
   otherwise the app shows Login with `login_first_run = true`. That Login
   hides its back button, disables global note shortcuts and Escape cannot
   dismiss it. "Try it offline" sets and persists `offline = true`. An existing
   database with no flag is treated as offline and the flag is persisted.
   Signing in clears `offline`; signing out sets it. The Settings route to
   Login hides the first-run restriction.
3. **Seeding is opt-in.** The empty Active notes view offers "Add welcome
   tour". It creates `Personal` and `Projects` only when those names do not
   exist, then adds three notes with fresh random ids through the ordinary
   `create_note` path. No notes are added at startup. Existing duplicate bodies
   are left untouched; History → Restore is the manual repair path.
4. **Search is local and tag-aware.** Schema v5 adds a `tags` column to the
   derived FTS5 table, copies the v4 rows into it, and fills each row from
   `note_tag`. Search snippets are plain body text. The shared
   `notes_store::search_terms` split feeds both the FTS expression and the UI
   highlighter, which renders `<mark>` runs without rewriting brackets or other
   literal text. Ctrl K focuses note search and Ctrl P opens the palette.

## Evidence

- `cargo nextest run --workspace`: 236 passed, 1 skipped. New tests cover tag
  matching without body text, literal brackets in snippets, the v3 → v5
  migration, preference round-trip and defaults, the first-launch decision,
  word-boundary highlighting and tag ordering.
- `scripts/web-a11y.mjs`: 61/61 checks, including first launch opens the
  welcome Login, no back button, offline starts with no sample notes, the tour
  creates three cards, Ctrl K focuses search, a search mark renders, a hit
  prefills Find, a tag matches a body without the word, and Ctrl P opens the
  palette. WCAG A/AA and the existing phone-layout checks still pass, including
  the floating new-note button and the tag chips that fold while scrolling.
- `scripts/web-persist.mjs`: 15/15 checks. Theme `dark` and the list layout
  survive a full browser restart, alongside the note, attachment and history.
- `scripts/web-restore.mjs` (18 checks), `scripts/pwa-check.mjs` (12 checks)
  and `scripts/web-wipe-login.mjs` (19 checks) pass on the release build.
- `scripts/web-perf.mjs`: 7/7 checks; 100 KB note open median 9.9 ms (max
  54.4 ms), input-to-frame p95 9.1 ms and max 13.7 ms, no dropped sample. The
  recorded web start of 1,195 ms includes the explicit offline choice and
  tour.
- `cargo check -p notes-app --no-default-features --features web --target
  wasm32-unknown-unknown`, `cargo fmt --all --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo deny check`,
  `scripts/budget-check` and `scripts/web-budget.mjs` (1.72 MiB brotli) pass.

## Consequences

- `PLAN.md` does not need a change: the app remains local-first and the Worker
  and sync protocol are untouched. Existing fixed-id welcome notes stay in old
  accounts; no migration rewrites their text.
- Anyone adding a display preference must extend the local `Preferences` shape,
  bump its browser key when the shape changes incompatibly, and keep it out of
  note and workspace documents.
- The local search schema is now v5. An older build refuses a newer database
  rather than opening it with the wrong index shape.
- Future onboarding material must use fresh ids and stay opt-in. Reintroducing
  startup seeding would revive the duplicate-body merge that this ADR closes.
