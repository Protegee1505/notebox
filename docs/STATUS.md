# Status

Updated: 2026-10-22
Phase: 8 (polish and performance, in progress); P1–P7 complete except the
owner-only release execution. The app can now restore a backup in Settings or
from the command palette, and export format v2 carries each note's Loro history,
so a fresh device gets the version timeline back. P1 archives still import. A
wiped browser profile also signs in to a local Worker and pulls its note, tag,
pin and history back. An accessibility audit of every main screen and theme
passes, including note-list arrow keys, Tab trapped inside dialogs and focus
return after closing a panel. Image notes now show their first image on cards,
Back opens a full-width desktop gallery, the Navigator collapses the sidebar,
and the attachment strip starts closed behind a footer count. A rendered image
or the footer count now opens a per-note attachment viewer with download,
remove-from-note and permanent delete; Settings reports local attachment bytes
and the cloud use of each 500 MB database, refreshed only on demand. The phone note
list now keeps a pasted image title
inside the viewport and keeps full title lines when the list scrolls, the
editor header fits and the toolbar scrolls at 320 px, and the colour menu no
longer puts a full-screen blocker over Back and Settings. A first launch now
asks for an account or an explicit offline choice instead of auto-seeding
samples; display choices persist on the device, search matches tags,
highlights matches on the result cards and opens a hit at the match, and the
optional welcome tour is created only when the owner asks for it. A phone can
start a note from a floating button on the list instead of opening the
sidebar. Relaunching now returns to the last note and screen, a visible device
sees another device's edit on its next ten-second poll without a reload,
deleting asks first by default, the avatar opens an account panel on desktop
and phones, and a first theme click while the choice is System always changes
the visible theme.
A 100 KB note opens in a median under 50 ms in the web UI and typing drops no
frames; Linux desktop cold start is 555 ms warm. The installer budget now
measures the `.deb` (9.9 MB), not the self-contained AppImage. The free Worker
is deployed and a fresh browser signed in and synced. S3 now has production
analytics and a 100-request tail probe: 156 production invocations, zero
errors, and no error 1102, with the CPU-budget nuance recorded in ADR-0003.
The full wipe/pull/history drill against the deployed Worker, the Windows and
Android runtime checks, and the v0.1.0 release remain; the search fixes are
deployed as 1e6bfdf, the image-card and desktop-chrome fixes as e11a0f5, the
session, confirm-delete and theme fixes as 71e0b72, and the avatar account
panel as 2582101, all after green CI, and all need owner acceptance before the
release. The running app can sign
in, restore a session, sync notes and attachments, show status, list and revoke
devices, and keep a conflict copy. The web build is served by the Worker and
syncs same-origin; local notes, history and images persist in OPFS, with no
account needed. The web build is now installable and works offline after one
visit, and a waiting deployment updates only when the user chooses Reload. The
release workflow builds Linux, Windows and web artifacts when a version tag is
pushed, with Android opt-in through `RELEASE_ANDROID=true`; it has not run yet.
A native build can bake in the owner's Worker URL with `NBX_SERVER_URL`, while
the login screen still offers offline use and a different server. The phone
editor now keeps its add-tag field, its tag chips fold away while the note body
scrolls, and no light row is left below the app when the keyboard opens.

## Shipped

- `notes-app`, `notes-core`, `notes-store`, `notes-sync`, `notes-server`:
  **browse, save and delete a note's attachments, and see cloud storage**.
  Clicking a rendered attachment image, pressing Enter or Space on it, or
  pressing the footer attachment count opens a per-note viewer at that image;
  the strip's **Browse all** opens the first image. The viewer keeps a
  thumbnail grid, position count, arrows/Home/End, a Tab trap and Escape, and
  returns focus to the image or footer button that opened it. It can download
  a file, remove an image reference from the note while another note still
  links the bytes, and permanently delete the file. Delete asks first with
  focus on Cancel, rechecks its guards at confirmation, refuses while a sync is
  running, while another note links the hash, while signed out for a previously
  uploaded file, or while offline/needs-login; a signed-in delete removes the
  cloud blob first, marks the local row pending to repair a crash or a lost
  response, edits the note that started the delete even if another note opens,
  and only then removes the references, local bytes and row. The last item
  closes the viewer and returns focus to the app root. There is no tombstone;
  ADR-0021 records the history and restore caveats.
  Settings adds local **Images & files** counts/size and, when signed in, two
  cloud meters for attachments and notes/history against the free 500 MB
  per-database limit, with remaining bytes and a refresh button. The notes
  figure is labelled approximate because `update_log` is base64. Usage is
  fetched on Settings open or Refresh, never polled. The new authenticated
  `GET /v1/usage` and idempotent `DELETE /v1/blob/{hash}` routes add no D1
  migration. Verified with 250 native tests passed and one skipped,
  `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo deny check`, `scripts/budget-check`, `server-e2e.mjs`
  71/71, `web-persist.mjs` 32/32 (preview and footer entry, download, keyboard
  navigation, focus return, two-file delete, last-file close),
  `web-a11y.mjs` 78/78 (320 px viewer fit and WCAG A/AA) and `web-perf.mjs`
  7/7. The web build is 1.75 MiB brotli. Not deployed yet.
- `notes-app`: **the avatar is the account control**. The topbar avatar is now
  a real button. It opens an account panel: an anchored popover on desktop and a
  fixed bottom sheet on phones. The panel shows the username or "Local only", a
  one-line sync state, a contextual "Sync now" when signed in or "Sign in to
  sync" when local, and "Account settings". Sign-out, devices and credentials
  stay in Settings. The status pill is a read-only indicator now, and the avatar
  carries the sync dot when the pill collapses on a narrow screen. Escape closes
  the panel and returns focus to the avatar; a click outside closes it; opening
  the palette, shortcuts, Find, history or another screen closes it. The header
  rises above a toast while the panel is open so the sheet rows stay tappable.
  Verified with 240 native tests passed and one skipped,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`,
  `scripts/budget-check`, `web-a11y.mjs` 76/76 (button semantics, panel
  open/focus/close, phone sheet inside the viewport), `web-persist.mjs` 22/22,
  `web-restore.mjs` 18/18 and `web-budget.mjs` 1.73 MiB brotli. No account, sync
  protocol or schema change. Deployed as 2582101 after green CI run 39733053626;
  `/version.json` matched, and a fresh live Chromium run opened the panel at
  1400 px, returned focus on Escape, and measured the phone sheet inside
  390×844 with no console errors.
- `notes-app`, `notes-sync`: **resume the last workspace, see other devices
  without a reload, and confirm a delete**. Local preferences now save the open
  note, Notes/Settings screen, Active/Archived/Trash view, phone list-or-editor
  pane and wide-gallery flag. Relaunch restores them, falls back to the first
  note when the saved one is gone, and never restores Login; search text and
  notebook/tag filters stay out. A new default-on "Confirm before deleting"
  setting puts an inline confirmation under the editor header, focuses Cancel,
  closes on Escape or Cancel, and can be turned off; Empty Trash keeps its own
  confirmation. The avatar is now a static, `aria-hidden` badge (Settings stays
  in the sidebar), the note list reserves 8 px above the first row so the
  selected ring and hover lift are not clipped, and a topbar theme click while
  the choice is System resolves `prefers-color-scheme` before choosing, so the
  first click always changes the visible theme. Visible clients now poll every
  ten seconds and wake on returning to the foreground; hidden pages still make
  no requests. ADR-0019 and ADR-0020 record the preferences and cadence.
  `scripts/free-tier-sim.py` reports 9,358 requests (9.36%) for three devices
  over eight visible hours, under the 10% gate. Verified with 240 native tests
  passed and one skipped, `web-persist.mjs` 22/22 (last note and screen restore,
  one-click theme, delete opt-out), `web-a11y.mjs` 73/73 (static avatar, 8 px
  ring clearance, default confirmation and focus, one-click theme, setting
  toggle), `web-restore.mjs` 18/18, `web-wipe-login.mjs` 20/20 (a second live
  profile edited the note and the first received it without a reload; 122 ms
  browser login-key median), and `web-budget.mjs` 1.73 MiB brotli. Chromium
  release build, no console errors. Deployed as 71e0b72 after green CI run
  39727904123; a fresh live Chromium run passed 7/7: Settings and the saved
  note came back after reload, the avatar was a static `SPAN`, one theme click
  left System for dark, and trash asked first with focus on Cancel.
- `notes-app`, `notes-store`: **image notes show their image, and the desktop
  chrome responds**. The store fills `NoteSummary.first_image` from one batched
  query for list, search and save summaries. Cards render the first linked
  image as a thumbnail, and a labelled image tile when the bytes are missing,
  over 800 KB, or not a previewable type; previews and search snippets drop
  raw image Markdown. At most 12 thumbnails stay in memory. Back now opens the
  `browsing` gallery: at 700 px and wider the editor is hidden and the list
  fills the window, while choosing a card returns to the editor; phones keep
  the existing list pane. A separate desktop Navigator button collapses the
  sidebar and shows the New note button on the list, while phones keep the
  drawer. The attachment strip starts closed behind a footer count button with
  a close control, and closes itself when the last attachment is removed.
  Verified with 239 native tests (one skipped), store tests for first-image
  metadata, removed links and missing bytes, `web-persist.mjs` 17/17 (closed
  panel and a decoded card thumbnail), `web-restore.mjs` 18/18 and
  `web-a11y.mjs` 67/67 (a 1160 px gallery at 1400 px, Navigator collapse and
  restore, plus the 320 px phone checks). Chromium release build. Deployed as
  e11a0f5 after green CI run 39718495732; a fresh live 1400 px browser opened
  offline, kept the attachment panel closed, decoded the card thumbnail,
  opened the 1160 px gallery with the editor hidden, and collapsed the
  sidebar, with no console errors.
- `notes-app`: **search highlights stay on the result cards, and a hit opens at
  the match**. Opening a result no longer opens or prefills Find, focuses the
  Find box or selects text in the editor; `<mark>` runs render only on a card's
  title, snippet and matching tags. A hit still lands on the first matching
  body text (the whole phrase when present, else the first query term) with a
  collapsed caret about a third from the top; a tag-only hit opens at the top.
  The editor is not focused, so a phone keyboard stays closed and focus stays
  out of the text. Opening any note now also resets the textarea's
  scroll, which removed a stale offset that could hide the next note's tag
  chips. Ctrl F remains the deliberate Find action.
  `scripts/web-a11y.mjs` replaces the prefill check with three checks: Find
  closed with no selection, a long note scrolled to its match, and a phone tap
  that leaves focus out of the editor; the flow passes 67/67 with no console
  errors. Chromium release build. Deployed as 1e6bfdf after CI run 39637535803.
- `notes-app`: **tag chips fold away while the note body scrolls**. The chip
  row sits above the scrolling body, so on a phone it kept taking a line from
  the text. It now collapses over 160 ms once the body passes 24 px (a
  `grid-template-rows` 1fr→0fr transition, no measured height), hands the
  height back to the text, and returns when the note is back at the top. Write
  and Preview each drive it from `onscroll`, keyed to note, mode and tag count,
  so switching note or mode, or adding or removing a tag, shows the chips
  again; `visibility: hidden` follows the collapse and keeps the row out of the
  tab order. The signal changes only when the row should move, not on every
  scroll tick. `scripts/web-a11y.mjs` grew two phone checks: scroll to the end
  (row at 0 px, chips hidden) and scroll back (row and chips return); the flow
  passes 61/61. Chromium at 320 px on the release build.
- `notes-app`: **a phone starts a note without opening the sidebar**. The note
  list now carries a floating plus button at widths up to 1099 px, where the
  sidebar is off-canvas; it calls the same `new_note` path as the sidebar, so
  it clears search, switches to Active and opens the editor with the caret in
  place. The list gains bottom padding so the last card scrolls clear of the
  button, and the button stays hidden on desktop, where the sidebar's New note
  button is always visible. `scripts/web-a11y.mjs` checks it is hidden at
  1400 px and opens the editor from the phone list (61/61).
- `notes-app`: **first launch is a real choice, and display settings survive
  a reload**. A saved account, the persisted offline flag, or an existing local
  database opens the note list; anything else stops on Login with no back
  button and no global note shortcuts. "Try it offline" persists
  `offline = true`; signing out also marks the device offline, while signing in
  clears it. An older database with notes but no flag is treated as offline and
  the flag is written. `theme`, `layout` and `offline` live in
  `preferences.json` on native and `localStorage`
  (`notebox.preferences.v1`) on web, are loaded before the first render and are
  never exported or synced. `scripts/web-persist.mjs` now checks that `dark`
  and the list layout survive a full browser restart (15/15).
- `notes-app`: **the welcome tour is opt-in, and the duplicate-body trigger is
  gone**. Startup no longer seeds notes into an empty database. The empty
  Active view offers "Add welcome tour", which reuses `Personal` and `Projects`
  by name and creates three notes with fresh random ids. The old fixed ids let
  two devices merge concurrent inserts into the same Loro text container,
  which is how a note body could repeat without a user edit. Existing duplicate
  bodies are left untouched; History → Restore is the manual repair path.
- `notes-store`, `notes-app`: **tags are searchable, matches are visible, and
  the shortcuts match the labels**. Local schema v5 adds a `tags` column to
  the FTS5 mirror, copies v4 rows into it and fills tags from `note_tag`; D1,
  the auth protocol and the sync protocol are unchanged. Snippets are plain
  body text (literal `[` and `]` stay literal), and the shared
  `notes_store::search_terms` split drives both the FTS expression and
  `<mark>` highlighting in titles, snippets and tags. Clicking a body hit opens
  Find with the phrase or first body term; a tag-only hit opens the note with
  Find closed. Ctrl K focuses note search, Ctrl P opens the command palette,
  and undo/redo use standard curved arrows. `scripts/web-a11y.mjs` grew the
  first-run and search checks (61/61).
- `notes-app`: **no light row under the phone keyboard, and a way to add tags
  on a phone**. Theme tokens now resolve at `:root` as well as on `.app`, and
  the app mirrors its theme onto `<html>` (a `use_memo` on the theme runs one
  `document::eval` per change), so `html`, `body` and the static crash panel
  paint with the app's palette; `html`/`body` lost `height: 100%` and the shell
  heights follow a `--app-height` variable kept in step with `visualViewport`
  by a small script in `index.html` (fallback `100dvh`, skipped while zoomed),
  with the viewport meta asking the keyboard to resize the layout
  (`interactive-widget=resizes-content`). The add-tag field was hidden below
  699 px while the chips row only renders when a note already has tags, so the
  footer keeps the field on phones and gained a tap "Add tag" button beside it.
  `scripts/web-a11y.mjs` grew five checks: the viewport meta, `html`/`body`
  matching the app's dark background (`rgb(21, 24, 21)`), a keyboard-sized
  viewport (320 px) keeping the app, its footer and the tag field on screen
  with nothing in-flow below it, and the phone editor adding then removing a
  tag. The flow is 48 checks; all pass on the fixed build, while the old
  CSS/HTML fails 4 of them (app height 780 px, body scroll height 780 px, a
  transparent `html` background and no tag field). Deployed as `c3eeddf`; the
  live `/version.json` reports that build and the served `index.html` carries
  the keyboard viewport hint and the `--app-height` script.
- `notes-app`: **note cards keep their full titles once the list scrolls**.
  The note list is both the scroll container and the grid. When its content was
  taller than the viewport, the implicit grid rows resolved to each card's
  `min-height` (`7.5rem` on phones, `10rem` on desktop), the cards' column flex
  children shrank to fit, and `.note-title`'s line clamp plus `overflow: hidden`
  sliced the glyphs. The six-note account showed it; the earlier four-note phone
  probe fit without scrolling and stayed green. `grid-auto-rows: max-content`
  sizes each row to its cards, and `flex: none` keeps compact list rows from
  shrinking. In Chromium 156 with the release CSS at 360×780 and six notes,
  cards were all 120 px with 10.7 px title boxes before and 120–165 px with
  21.6/43.2 px title boxes after; at 1024×700 with 14 notes, 160 px cards grew
  to 160–182 px. Compact rows went from 40.8 px with 0 px titles to 84.1 px
  with 19 px titles. `scripts/web-a11y.mjs` now forces a 360×420 overflow and
  checks every card title keeps a full line, then checks the compact rows at
  1024×420. The 42-check flow passes on the fixed build and fails on the old
  CSS. Deployed as `5fae2ef`; a fresh live 360×420 boot with the three seeded
  notes scrolled the list while every title kept a full 21.6 px line, the grid
  reported `max-content` rows and the console stayed clean.
- `notes-app`: **fix the sync crash that froze the phone**. The first sync that
  downloaded an image attachment panicked in `load_attachments` with
  `AlreadyBorrowed`: `if let Some(id) = self.state.peek().selected` kept the
  read guard alive across `load_attachments`'s write guard. The selected id is
  now copied out before the call, and the same guard ordering is fixed in the
  `open_history` "no note" branch. Reproduced by replaying the live account's
  55-update log and 39 KB WebP into a local Worker and a debug wasm build:
  before the fix the panel showed `RuntimeError: unreachable` right after
  `GET /v1/blob/...`; after it the sync completed with the panel hidden and no
  console errors. Deployed as `bd6d1e6`; a fresh 360 px live boot loaded the new
  wasm, kept the panel hidden and logged no console errors.
- `notes-core` and `notes-app`: **phone title, layout and tap fixes**.
  `Note::title()` replaces Markdown images with their alt text, and the UI also
  sanitises cached summary titles on display and in conflict copies, so notes
  written by older builds stop showing a data URL. Cards use
  `minmax(0, 1fr)`, `min-width: 0` and `overflow-wrap: anywhere`; the phone
  editor keeps a dot-only save state, the notebook name fits and the formatting
  toolbar scrolls horizontally instead of clipping its last controls. The
  colour menu is a fixed bottom sheet with no backdrop: a root click closes it
  and the same tap reaches Back or Settings, and screen, sign-in and
  local-session transitions clear it and the find bar. `index.html` adds a
  static wasm-panic
  panel with a Reload button so a crashed app does not look like dead controls.
  A 360 px touch probe against `wrangler dev --local` signed in, reached
  `Synced`, showed the note without horizontal overflow, and kept Back,
  Settings and the navigation drawer responsive.
- `docs/adr/0003-worker-free-tier.md`, `scripts/s3-probe.mjs`,
  `scripts/s3-analytics.mjs` and `docs/measurements/`: **S3 evidence**. The
  seven-day production window before the probe held 156 invocations, all
  error-free; CPU per minute bucket was a 4.6 ms median, 26.3 ms p95 and
  41.8 ms maximum, with 31 of 156 requests in buckets above 10 ms. The peaks
  are consistent with isolate startup or heavier authenticated work; the
  controlled warm probe is 1-2 ms. None produced error 1102 or an
  `exceededCpu` outcome. A controlled 100-request probe at one request per
  second captured 100/100 tail events, zero failures, a 1 ms median and 2 ms
  maximum CPU, and only the expected 401 codes. The ADR records the decision to
  keep the free Worker and to monitor for 1102. Desktop Argon2id is 38.2 ms
  median over 20 release runs (`crates/notes-sync/examples/argon2_bench.rs`);
  the release web build measured 119 ms median over three rejected logins,
  including the local Worker round trip. The phone number remains open.
- `scripts/web-wipe-login.mjs`: the P8 wipe drill now times three rejected
  sign-in attempts before creating the account, checks the browser Argon2id
  path stays under three seconds and keeps the expected 401 console messages
  out of the error count. The flow passes 16/16 checks against a local Worker.
- `notes-app` and `notes-server`: **Settings → Change username or password**.
  The form asks for the current password, allows a username change, and treats
  a blank new password as "keep the current one". `POST
  /v1/auth/credentials` verifies the current derived key, updates the account
  row and `auth_hash` in one D1 batch, resets the login backoff, keeps the
  calling device signed in and deletes every other device row. A wrong current
  password returns `403 wrong_password`; there is no password reset. The app
  saves the new username, rebuilds its client with the same device token and
  refreshes the device list. `MIN_PASSWORD_CHARS = 12` is shared with account
  setup, and no migration or server-side Argon2id was added. ADR-0017 records
  the trade-offs.
- `scripts/server-e2e.mjs`: the throwaway Worker drill now also tries a wrong
  current key, changes the username and key, checks that only the caller's
  device survives, rejects the old pair and accepts the new one.
- `notes-app`: personal-server login. A native build reads `NBX_SERVER_URL` at
  compile time and pre-fills the Worker URL; the login screen keeps it editable,
  shows "Use the built-in server" after an edit, and explains when no URL is
  baked in. The web login shows the serving origin read-only because sync is
  same-origin. "Try it offline — no account" is the secondary action on every
  build, so local notes, history and images need no Worker.
- `notes-sync`: `scripts/create-account` and the
  `create_account` example create the single account on a local or deployed
  Worker. The password and setup token are read without echo, the password
  must be at least 12 characters, setup errors are returned as typed sync
  errors, the temporary setup device is revoked, and the output tells the owner
  to delete `SETUP_TOKEN`.
- Deployment: the free Worker is live at
  `https://notebox.nbx-7207a015.workers.dev`. Both APAC D1 databases
  (`notebox-sync` and `notebox-blobs`) have schema v1 applied, the one-time
  `SETUP_TOKEN` secret was removed after the single account was created, and the
  `NBX_SERVER_URL` repository variable points at the Worker. A fresh headless
  Chromium context signed in, reached "Synced" and wrote welcome-note updates;
  the verification devices were revoked. The owner still signs in on their own
  browser with the credentials saved outside the repository.
- `scripts/deploy-web.sh`: guarded web deploy for agents. `--check` reports the
  live build, the committed diff since it, CI and remote migration state without
  changing anything; `--if-needed` is a no-op when live already matches
  `origin/main` or only docs/CI/tooling changed. A deploy requires a clean tip
  of `origin/main`, a green CI run and no pending D1 migrations, builds with the
  commit stamp, deploys with a version message, verifies `/version.json` and
  prints the rollback command. `AGENTS.md` now points agents at the script
  instead of raw `wrangler deploy`.
- `notes-store`: stable welcome ids. `NewNote` accepts an optional fixed
  `NoteId`, and `create_notebook_with_id` creates a notebook with a supplied
  id. The three welcome notes and two notebooks use fixed UUIDs, so a fresh
  second device merges them with the account's copies instead of adding a new
  set. Normal note and notebook creation still uses a random id.
- `notes-server`: an unknown username now returns `invalid_credentials`
  without incrementing the account's failed-login counter or writing to D1.
  Only a wrong key for the real username counts toward the 1–300 s backoff, so
  a stranger cannot lock the owner out by trying random usernames against a
  public personal Worker. The server test covers the counter and a successful
  login immediately after.
- `.github/workflows/release.yml`: Linux, Windows (and the opt-in Android)
  bundles receive the `NBX_SERVER_URL` repository variable. The Android job is
  skipped unless `RELEASE_ANDROID=true`, and `publish` uses `always()` with a
  failure guard so a skipped Android job no longer blocks a Windows/Linux/web
  release. Release notes list Android only when an APK is present. The
  checkout-free publish job also sets `GH_REPO`, which `gh release create`
  needs to resolve the repository.
- `notes-app`: restore from backup. Settings and the command palette open a zip
  picker, validate the archive before changing anything, and show note,
  notebook and attachment counts plus whether the archive carries history.
  Restore runs the store's single transaction, then refreshes the note list,
  notebooks, tags, the open note and browser persistence. A sync run in progress
  or a still-dirty editor blocks the restore; a failed read or import leaves the
  database untouched.
- `notes-core`: export format v2. Each note gains an optional full Loro snapshot
  at `history/<note-id>.loro`; the Markdown file still holds the current text.
  The reader accepts v1 and v2, rejects newer versions, and validates snapshot
  paths against traversal and missing entries. ADR-0014 records the decision.
- `notes-store`: history-preserving import. A new note is rebuilt from its
  snapshot with its original peer ids and timestamps; an existing note merges
  the snapshot and re-applies the archived text as a normal edit when it
  differs, so newer local edits stay in the timeline. Two store tests cover
  timeline retention and a restore over newer local edits.
- `scripts/web-restore.mjs`: 18-check release-build Playwright flow. Profile A
  makes a notebook, a pinned tagged note with a PNG and a trashed note, then
  exports. A fresh profile B imports through the Settings file picker and finds
  the text, tag, notebook assignment, pin, trash state, attachment row, decoded
  WebP bytes and a history version labelled as another device. Importing the
  same zip again does not duplicate the note or its history. The CI web job now
  runs it after the restart check.
- `scripts/web-wipe-login.mjs`: 19-check P8 account drill. It boots
  `wrangler dev --local` with both D1 databases, serves the release build and
  enters offline mode (no auto-seeded notes) before creating a note. It captures
  the app's derived login key from a first sign-in attempt, creates the one
  account with the setup token, signs in, and waits for the first sync. It then
  closes the browser, deletes the OPFS database, `localStorage`, cookies and
  service-worker caches, confirms the note is gone, signs in again with the
  same password and pulls the note body, tag, pin and a history version
  labelled as the other device. The pull does not duplicate it. Next it opens
  the credential form, changes both username and password, signs out, signs in
  with the new pair and confirms the note is still there. The CI web job
  installs `worker-build` and runs it after the backup round trip.
- `scripts/web-a11y.mjs`: 76-check P8 accessibility, onboarding, phone-layout
  and desktop-chrome flow. It runs axe-core's WCAG 2.0, 2.1 and 2.2 A/AA rules
  on notes, history, the command palette, shortcuts, trash, Settings, Login and
  the mobile layout, each in light and dark themes. The same run checks first
  launch opens the welcome Login with no back button, offline starts with no
  samples, the welcome tour creates three cards, Ctrl K focuses search, Ctrl P
  opens the palette, a search match gets a `<mark>`, opening a hit leaves Find
  closed and selects no text, a long hit scrolls to its match, a tag-only hit
  keeps Find closed, a phone tap on a hit keeps focus out of the editor,
  desktop Back opens a 1160 px gallery at 1400 px, choosing a gallery card
  returns to the editor, and Navigator collapses and restores the sidebar. The
  avatar is a button with dialog semantics; opening it focuses the account
  panel, Escape closes it and returns focus to the avatar, and at 390 px the
  panel is a fixed sheet inside the viewport. The selected card clears
  the list's top clip edge by 8 px, moving a note asks first and focuses Cancel
  before Escape/Cancel returns focus to the trash button, one topbar click from
  System lands on dark, and the confirmation setting toggles off and back on.
  It checks note-list Arrow/Home/Enter handling, keeps Tab and Shift+Tab inside
  the three modal dialogs, checks focus return after
  closing history, the palette or the shortcut sheet, Settings' back button and
  Login's first field. At 390, 360 and 320 px it adds an image data-URL note,
  asserts no horizontal list or document overflow, that the 320 px editor
  header fits with a reachable Quote button and a scrollable toolbar, that the
  colour sheet stays in the viewport, that the floating new-note button is
  hidden at 1400 px and opens the editor at 390 px, that the tag row collapses
  when the note body scrolls and returns at the top, that one tap on Back
  leaves the editor with the colour menu open, and that Settings and the
  navigation drawer still open. It then shortens the viewport to 360×420 so the
  list scrolls, asserts every card title keeps at least one full line, switches
  to the compact rows at 1024×420 and asserts the same there. The CI web job
  installs `axe-core` and runs it after the backup round trip.
- `notes-app`: accessibility pass. Muted text now meets 4.5:1 on every surface,
  the note list is a labelled listbox with arrow-key focus, the editor is a
  `main` landmark, icon-only file inputs have labels, tag remove and restore
  targets are at least 24 px, and closing an overlay returns focus to the app.
  History, the palette and the shortcut sheet take focus on open and keep Tab
  inside themselves. Trashed notes are restored from the editor header; the
  duplicate button inside the selectable card is gone. The theme audit caught a
  low-opacity palette hint, which no longer dims below AA.
- `scripts/web-perf.mjs`: 7-check P8 large-note flow. It creates a note of
  102,415 bytes, saves it, leaves and opens it five times, types 120 characters
  while recording input-to-next-frame time, saves again and reopens it after a
  reload. The run asserts a median open under 50 ms, no input-to-frame sample
  over 34 ms (two 60 Hz frames), every character present and no console errors.
- `scripts/desktop-cold-start.sh`: P8 Linux cold-start measurement. It starts a
  private Xvfb display, isolates `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and
  `XDG_CACHE_HOME`, launches the release binary with `NBX_READY_FILE`, and
  waits for the editor's mount probe. The first run seeds sample notes; warm
  runs use that database. Five warm runs had a 555 ms median (547–587 ms) under
  the 600 ms budget. The probe is inert unless the environment variable is set.
- `docs/adr/0015-installer-budget.md`: the 20 MB desktop-installer budget is
  the platform package that uses the system webview — the `.deb` (9.9 MB) on
  Linux and the `.msi` on Windows. The 89.6 MB AppImage stays as an optional
  self-contained fallback because it bundles WebKitGTK; removing the engine
  would make it depend on the host and weaken the portable smoke test.
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
  Windows `.msi`, NSIS setup `.exe` and portable `.exe`, and the installable web
  `.zip`; the arm64 Android APK is built only with `RELEASE_ANDROID=true`. It
  installs and boots the Linux packages and the Windows MSI on clean runners,
  verifies an enabled APK with `apksigner` and `aapt`, runs the PWA checks,
  writes `SHA256SUMS` and publishes a GitHub Release. When Android is enabled,
  missing signing secrets fail the job before it builds; it never publishes an
  unsigned or debug APK. The workflow has not run yet.
- `Dioxus.toml`: bundle identifier, publisher, icon, category, description and
  Debian runtime dependencies for `dx bundle`. The Android signing block is
  added only on the CI runner from secrets and restored afterward.
- `docs/adr/0011-installable-web-app.md` records the precache, update and
  cache-header decisions and the offline/update evidence.
- `notes-app`: P7 sync wiring. The running app signs in, restores a saved
  session, saves the open draft before each run, and syncs on start, focus,
  three seconds after an edit and every ten seconds while visible (ADR-0020),
  with a five second floor and the server's backoff. Native login/sync/device calls run on
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
  run after the five second floor, so it does not wait for the next idle poll.
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
  Chromium profile. It enters offline mode, creates a local-only note, attaches
  a PNG, waits for the WebP row, confirms the attachment panel starts closed
  and the card shows a decoded thumbnail, closes and reopens the browser, and
  finds the text, image reference, attachment row and decoded `data:image/webp`
  bytes. It also checks the second tab lock, the history panel, the Settings
  label after restart, a synthetic page-hide after a just-typed edit, and that
  the dark theme and list layout survive the restart. 17/17 checks pass with no
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
  sync before the next idle poll, copy pulled by a third device).
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
- `notes-store`: schema v4 (v5 adds tags to the search index) stores attachment
  bytes in the local SQLite file, deduplicates by SHA-256, indexes note links,
  creates placeholders for links without bytes, tracks the upload queue,
  measures usage, and carries bytes through export/import. Removing a Markdown reference only drops the link,
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
- `notes-app`: the P5 trash view. A trashed note opens in the editor and is
  restored from its header, which returns to the active notes view. "Empty
  trash" expands an inline confirmation with the note count before erasing, and
  the empty state explains that deletes wait in the trash. Trash screenshots are
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
- `notes-app`: command palette (Ctrl P), note search (Ctrl K), keyboard
  shortcut sheet, global search, workspace shortcuts (Ctrl N/S/F/E/Z/1/2/3/,)
  and a visible Commands entry so touch devices can reach the palette.
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
  answer; the password and the key never reach the server.
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
  web app, ADR-0012 tagged release artifacts, ADR-0013 the rustls-webpki
  security patch, ADR-0014 backup history, ADR-0015 the installer budget and
  ADR-0016 the personal-server/offline-first release. `docs/API.md` documents
  the v1 endpoints the P3 engine implements.
- Development disk: Cargo target, Rust/Cargo/npm/yarn/dx caches and Playwright
  browsers now live under `/tmp/opencode`; `scripts/relocate-caches.sh` repairs
  the layout and `scripts/budget-check` fails below 512 MiB free on `/home`.
  See `docs/DISK.md`.

## Measurements

| Check | Result |
| --- | --- |
| S3 Worker CPU (production, Cloudflare analytics) | 156 invocations before the probe, 0 errors, all `success`; CPU bucket p50 median 4.618 ms, p95 26.291 ms, max 41.825 ms; 31 of 156 requests in 29 buckets above 10 ms; wall p99 median 127.2 ms, max 720.8 ms |
| S3 tail probe | 100 requests at 1/s: 100/100 tail events, 0 failures, CPU min 0 / p50 1 / p95 2 / max 2 ms; login 401 `invalid_credentials`, devices and sync 401 `unauthorized`; no 1102 or 5xx |
| S3 Argon2id | desktop release 38.2 ms median (20 runs, max 92.1 ms); Chromium release build 119 ms median (178/119/116 ms, includes local Worker round trip); phone not measured |
| S3 free limits | re-read 2026-10-21: 100,000 requests/day, 10 ms CPU, 50 subrequests, 50 D1 queries, 2 MB rows, 10 D1 databases; 2 databases in use |
| Native tests | 250 passed, 1 skipped (the ignored Worker client e2e) |
| Dependency security | `RUSTSEC-2026-0384` (race in `tokio::time::Interval` on 1.54.0) fixed by the lockfile-only bump to `tokio` 1.54.1; local `cargo deny check` is clean |
| CI | Run 39769056107 on 409740c: all eight jobs green (gates, wasm, server, Windows, Android, desktop, OPFS, web), including the attachment gallery checks and the tokio security bump. Run 39733053626 on 2582101 was also green; run 39727904123 on 71e0b72 was green too. The earlier run 39637535803 on 1e6bfdf was green, though GitHub Actions briefly left it `in_progress` during its 2026-10-21 partial outage after every job had passed |
| P8 backup restore (Playwright) | 18/18 checks: fresh-profile export/import of a notebook, pinned tagged note and trashed note; text, tag, pin, trash, attachment row, decoded WebP, history peer label and idempotent second import; no console errors |
| P8 wipe and sign-in (Playwright + local Worker) | 20/20 checks: three timed rejected logins (browser Argon2id median 122 ms), derived login key, account setup, first sync, image attachment, a second live profile appends a line that the first window receives without a reload, OPFS/localStorage/cookie/cache wipe, note gone, sign in, attachment download without a crash, note body/tag/pin/image, history labelled as the other devices, no duplicate, no console errors |
| P8 accessibility, onboarding and search (Playwright + axe-core) | 78/78 checks: WCAG 2.0/2.1/2.2 A/AA on notes, history, palette, shortcuts, trash, Settings, Login and mobile, each in light and dark; first launch shows welcome Login with no back button, offline starts empty, optional tour adds three cards, Ctrl K focuses search, Ctrl P opens the palette, a search mark renders, opening a hit leaves Find closed with no selection and a long hit scrolls to its match, a phone tap on a hit keeps focus out of the editor, tag-only matches keep Find closed; the avatar is a button that opens an account panel, focuses it, closes on Escape with focus back on the avatar, and becomes a fixed in-viewport sheet at 390 px, the selected card ring clears the list top by 8 px, trash asks first and focuses Cancel before Escape/Cancel returns focus to the trash button, one topbar click leaves System for dark, and the delete-confirmation setting toggles off and on; note-list Arrow/Home/Enter, Tab and Shift+Tab stay inside the three dialogs, focus return after overlays, Settings/Login/dialog autofocus; image title and no list overflow at 390/360/320; card titles keep a full line at 360×420 and compact rows at 1024×420; 320 px header, scrollable toolbar and reachable Quote; colour sheet inside the viewport; the floating new-note button hidden at 1400 px and opening the editor at 390 px; tag chips fold away while the note body scrolls and return at the top; one-tap Back and phone Settings/navigation; desktop Back opens a 1160 px gallery, choosing a note returns, and Navigator collapses and restores the sidebar; the attachment viewer fits a 320 px phone and passes axe WCAG A/AA on mobile dark; no console errors |
| P8 local preferences, attachments and restart (Playwright) | 32/32 checks: browser storage, the attachment panel starts closed, the card shows a decoded thumbnail, note with image and history survive restart, the last note and Settings screen return without choosing them, one theme click leaves System, dark theme and list layout survive restart, a rendered image and the footer count open the viewer, arrows step it, Tab stays inside, close returns focus, download saves the selected file, delete confirmation starts on Cancel, one of two files can be removed and the last copy can be permanently deleted, the viewer then closes and focuses the app root, Settings drops the last attachment, turning delete confirmation off skips the prompt, second tab blocked by the store lock, no console errors |
| P8 phone sign-in (one-off Playwright, 360 px touch + local Worker) | New note saved, derived login key, account setup, status `Synced`, synced note in the list, no list overflow, Settings round trip, navigation drawer, one-tap Back with the colour menu open; no console errors |
| Web login preview | Rebuilt release at 2026-10-21: the Worker origin is read-only with no URL input, the offline action is visible, and the screen has no horizontal overflow or console errors at 1400 px and 390 px |
| Live Worker deployment | `https://notebox.nbx-7207a015.workers.dev` serves release 0.1.0 / 2582101; `/version.json` reports the same build. A fresh live Chromium run opened offline, opened the account panel at 1400 px, saw "Sign in to sync" and "Account settings", returned focus to the avatar on Escape, and measured the fixed phone sheet inside 390×844; no console errors. The previous 71e0b72 deploy opened offline, saved a note, reopened Settings and the note after a reload, found the avatar a static `SPAN` with no role or tab stop, moved System to dark, and saw the trash confirmation with focus on Cancel; 7/7 checks. The e11a0f5 deploy opened the 1160 px Back gallery and decoded a card thumbnail; 1e6bfdf carried the search-open change verified by 63/63 local release checks. The ce65296 probe had shown Welcome Login with no back button, then Try it offline opened an empty library with the optional Add welcome tour and no console errors. The earlier 5fae2ef deploy checked the seeded list's title line; the e9c8e82 deploy passed a fresh sign-in and first sync; the 759971a phone probe created a local image note and exercised the colour sheet and one-tap Back |
| Deploy guard | `./scripts/deploy-web.sh --if-needed` deployed 2582101 after green CI run 39733053626 and verified `/version.json`; the check also reported no pending migrations on either D1 database, `dx` 0.7.10 and `worker-build` 0.8.7. The previous guard deployed 71e0b72 after CI run 39727904123 |
| Remote D1 migrations | `notebox-sync` and `notebox-blobs` each applied `0001_init.sql`; `wrangler d1 migrations list --remote` reports no pending migrations |
| P8 large note (web UI) | 102,415 B note: final gallery build open median 40.0 ms (45.9, 40.0, 43.7, 36.2, 36.2 ms); 120 typed characters, input-to-frame p95 10.7 ms, max 24.8 ms, none over 34 ms; text survives a reload; web start 1,003 ms including the offline choice; an earlier run opened in a median 9.9 ms with a 1,195 ms start |
| P8 desktop cold start (Linux release) | warm median 555 ms over 5 runs (547–587 ms), budget 600 ms; earlier first run 628 ms with sample seeding, 2,170 ms when WebKit's cache is cold; not remeasured since automatic seeding was removed; Xvfb with isolated XDG directories |
| P8 backup archive | v2 manifest plus one full Loro snapshot per note; v1 archives still read; restore over newer local edits keeps both timelines |
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
| PWA bundle | 10 precached files; 32,625 bytes of manifest, icons, HTML and worker; pre-cache assets from the Dioxus manifest only |
| P7 browser persistence (Playwright) | 13/13 checks: Browser storage label, local-only note and PNG, WebP attachment, second-tab lock, browser restart, text/image/history, page-hide save, no console errors |
| P7 conflict flow (Playwright) | 13/13 checks with two device contexts plus a third puller: delayed sync, edit during the run, conflict toast, merged original, `conflict`-tagged copy holding the typed text, follow-up sync before the next idle poll, copy pulled by another device |
| Web bundle (release, brotli) | gallery/usage build 1,836,762 B (1.75 MiB), raw 7,759,467 B, limit 3.00 MiB; session/theme build 1,815,379 B (1.73 MiB), raw 7,650,474 B; e11a0f5 image-card build 1,813,112 B (1.73 MiB), raw 7,638,386 B; P8 backup restore 1,787,615 B (1.70 MiB), raw 7,501,597 B; P7 release build 1,818,284 B (1.73 MiB), raw 7,529,584 B; P7 Dioxus build 1,792,565 B (1.71 MiB), raw 7,489,034 B; P6 manual build 1,955,393 B (1.86 MiB), raw 9,353,273 B; P5 CI was 1,545,831 B |
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
| Server end-to-end | 71 checks green against `wrangler dev --local` and local D1, including logout, cookie expiry, authenticated usage and idempotent blob deletion |
| Client sync, in-process | 14 two-client tests on the real handler stack; lost response, dropped request, `429` and corrupt chunks all converge |
| Client sync, real Worker | 1 test / 24 assertions green through `wrangler dev --local` and D1 in 3.23 s |
| Argon2id login key | 38.2 ms median over 20 release runs (max 92.1 ms); see the S3 Argon2id row. Debug build 0.61–0.66 s |
| Offline queue | 30 new notes: 25 pushed in one run, 5 in the next; queue empty after the server accepts |
| Attachment round trip | client-to-client 2.5 MiB / 3 chunks in-process; 150 KB through real D1 with bytes and metadata identical |
| Free-tier heavy day | Ten-second visible poll: 9,358 requests (9.36% of 100k), 27,043 rows read (0.54%), 1,314 rows written (1.31%), all under the 10% gate |
| Worst-case request | 28 of 50 queries on a 25-update push; 5 bound parameters; base64 update row 1.37 MB of the 2 MB row limit |
| wasm check | `notes-proto`, `notes-core`, `notes-store`, `notes-sync` compile for `wasm32-unknown-unknown`; `notes-server` clippy-clean for wasm |
| Loro wasm smoke (Node) | 1 passed |
| Text-box convergence | 1,000 randomized whole-value edits |
| Linux desktop boot (CI) | stayed up 20 s under Xvfb; P4 local build also stayed up 20 s (`exit 124`) |
| Windows desktop build (CI) | `notebox.exe` built and uploaded |
| Android headless compile (CI) | NDK r27c, four crates |

## Blocked

- **Owner sign-in**: the Worker, both D1 databases and the single account exist,
  the setup secret is gone and `NBX_SERVER_URL` is set. The owner still has to
  save the generated username and password outside the repository and sign in
  at the live URL; native Windows/Linux login and sync still need a live run.
  Once signed in, Settings can change the username or password; there is no
  reset if both the password and every signed-in device are lost.
- **Android runtime**: no `/dev/kvm` and no Android SDK, so S2/S5 cannot be run
  on a device or emulator locally, and the P4/P6 Android screenshot, camera and
  boot checks are still missing. The local `aarch64-linux-android` check also
  lacks `aarch64-linux-android-clang`; CI still compiles the four crates with
  NDK r27c. The full Android app has not been built, and the Argon2id timing on
  a mid-range phone is not measured.
- **Release execution**: `.github/workflows/release.yml` has not run. Android
  is now opt-in through `RELEASE_ANDROID=true`; enabling it needs the owner to
  create a keystore and set `ANDROID_KEYSTORE_BASE64`,
  `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and
  `ANDROID_KEY_PASSWORD`. The first `v*` tag will also exercise the clean-runner
  Linux and Windows install smoke jobs. The desktop update-available notice is
  not implemented; only the web build has one.
- **P8 account drill**: the wipe, sign-in and history pull passes against
  `wrangler dev --local` with 19/19 checks, including an OPFS and session wipe.
  The deployed Worker now accepts a fresh browser sign-in and first sync; the
  full wipe/pull/history drill against the deployed Worker still needs a run.
  A wiped device now enters offline mode with an empty library instead of
  seeding samples before sign-in, and the optional tour uses fresh note ids.
- **Existing duplicate bodies**: the fixed-id auto seed is gone, but bodies
  already duplicated by it are not rewritten automatically. History → Restore
  an earlier version of the affected note, or keep the current text; no startup
  migration guesses which copy the owner wanted.
- **Owner phone freeze retest**: the total dead-tap freeze was a wasm panic,
  not the image-title overflow or the colour backdrop. The first signed-in sync
  that downloaded an image attachment hit `AlreadyBorrowed` in
  `load_attachments` (see Shipped) and trapped the wasm instance; the new panic
  panel showed `RuntimeError: unreachable`. The guard ordering is fixed and the
  failure was reproduced and re-tested locally with the live account data. The
  deployed build now contains the fix; it still needs a retest on the owner's
  Brave Android device. The OPFS image and the server both keep the notes; if
  the interrupted
  run did not persist its pull or attachment, the next boot repeats that pull
  and download under the fixed code.
- **Cold starts and large notes**: Linux desktop cold start is 555 ms warm
  (median of five), under the 600 ms budget; that run predates the first-run
  gate and automatic seeding was removed, so the first run needs a fresh
  measurement. A 102,415-byte note opens with a median of 11.1 ms and typing
  drops no frames (p95 9.7 ms, max 20.2 ms); earlier opens reached 54.7 ms.
  Windows is build-only and Android has no boot or cold-start number. The 20 MB
  installer budget now measures the `.deb` (9.9 MB); the 89.6 MB AppImage is an
  optional self-contained fallback under ADR-0015.
- **Human cross-device acceptance**: the app-side account, sync loop and
  browser transport are wired, a two-device browser run passes the P7 login,
  device, conflict and sign-out checks, and the deployed Worker now accepts a
  live browser sign-in. The human P5/P6/P7 check across a desktop and a phone
  still waits on an Android runtime and a live native login. The native desktop
  path is compile-checked and shares the engine tests, but this slice did not
  drive its login/sync threads through the UI against a live Worker.
- **Attachment gallery deployment**: the viewer, download, permanent delete,
  `GET /v1/usage` and the Settings meters pass the local release checks and
  are merged to `origin/main` as `e7f41cf`, followed by test fixes `d2930bb`
  and `fbedddd`; CI run 39769056107 on `409740c` is green on all eight jobs.
  Deployment still needs the owner's separate approval; then verify the
  viewer, a download, a delete and both cloud meters against the live Worker.

## Next

1. P8 finish. Accessibility, the 100 KB note, the Linux warm cold-start budget,
   the installer decision (ADR-0015) and S3 (ADR-0003) pass, and so does the
   local-Worker wipe/sign-in/history drill. The deployed Worker has passed a
   fresh sign-in and first sync; close P8 with the full wipe/pull/history drill
   against it. Repeat the first-run, open-note and cold-start measurements on
   Windows and Android when those runtimes are available.
2. Ship the attachment gallery: merged and green in CI; ask the owner before
   deploying. Verify a rendered preview and the footer count open the viewer
   live, download a file, delete a throwaway attachment and see both Settings
   meters; then ask the owner to accept it.
3. Personal release execution: the Worker is deployed, the account is created,
   `SETUP_TOKEN` is deleted and `NBX_SERVER_URL` is set. **The v0.1.0 tag is
   deliberately not pushed yet**: the first-run, preferences, search,
   shortcut, image-card, desktop-chrome, session-restore, confirm-delete,
   account-panel and theme fixes are merged, deployed as 2582101 and green in
   CI run 39733053626; the owner should accept them. Then bump/confirm the
   version, push the tag, watch the first release workflow, and verify login
   and sync on a Windows install, a Linux install and the deployed web app.
4. P7 release execution for Android when wanted: create and back up the
   keystore, set the four repository secrets, set `RELEASE_ANDROID=true`, and
   publish a tagged release. Do not publish an unsigned APK. Add the desktop
   update-available notice in a later v0.1.1, after the first release; the web
   banner is already done.
5. Run the human P5/P6/P7 acceptance on a desktop and a phone, including a
   photo, a version restore and an installed PWA update. S3's desktop and
   browser CPU and Argon2 evidence is closed; the phone Argon2 number comes with
   the Android runtime work below.
6. Run S2/S5/P6/P7 on an Android emulator with `/dev/kvm` (CI or a physical
   device) to close the runtime gap, including Gboard composition, the camera
   picker and the in-app login/sync loop.
7. Measure an OPFS image write on 1,000 notes and 50 images; if it exceeds
   roughly 100 ms, schedule the worker-backed `SqliteStore` from ADR-0004.
