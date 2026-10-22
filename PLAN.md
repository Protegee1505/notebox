# Notes app: build plan for coding agents

Oct 20, 2026 · @LoweSam

## Summary

Build a local-first Rust app: one Dioxus codebase for web, Windows, Linux and Android, syncing through a small Cloudflare Worker and D1 database that cost nothing to run.

You open the app, enter your username and password, and every note is there. Each note keeps its full edit history. Devices work offline and merge when they reconnect.

| Layer | Choice | Fallback |
| --- | --- | --- |
| UI | Dioxus 0.7: one Rust codebase for web (WASM), Windows, Linux, Android | Tauri 2 + Leptos |
| Editor | Markdown in a native text box with a rendered view; images and links as Markdown | CodeMirror live preview (needs a little JS) |
| Note format | One Loro CRDT document per note: merges offline edits, keeps history | Revision snapshots + 3-way merge |
| Local storage | SQLite (rusqlite natively, sqlite-wasm-rs in the browser) | IndexedDB in the browser |
| Server | Cloudflare Worker in Rust (workers-rs) + D1; the same Worker serves the web app | Same server crate as axum on another free host |
| Images | Compressed WebP in a second D1 database (R2 reportedly needs a card) | R2, if you accept a card |
| Login | One account; password stretched on your device; one token per device | Passkeys, later |
| Releases | Installable web app, .msi/.exe, AppImage/.deb, .apk on GitHub Releases | none |

Two trade-offs to accept. The editor is Markdown-based, not Word-style WYSIWYG: Rust has no mature rich-text editor, and a plain text box keeps typing fast and Android keyboards reliable. And Phase 0 proves the five riskiest assumptions first, each with a fallback, before any feature work starts.

## Do this first

Agents cannot create your accounts or hold your secrets. Do these six things once, then hand the rest to the agents.

1. **Cloudflare account (free).** Confirm you can create a Worker and a D1 database without adding a card. Do not enable R2: sources say it asks for a payment method even on its free tier.
2. **GitHub account and a public repo.** Actions builds the installers and Releases hosts them, so a fresh machine can download the app. The repo holds no secrets; the server is protected by your login.
3. **Tools on your dev machine.** Rust via rustup, the Dioxus CLI (`dx`), Node.js for `wrangler`, and Android Studio with SDK and NDK. Run `npx wrangler login` once. If you skip Android Studio, CI can build the APK, but you lose emulator testing.
4. **Username and a long password.** At least 12 characters; six or more random words is better. It is stretched into your login key on every device, and there is no email reset in v1.
5. **Secrets.** Let an agent generate a `SETUP_TOKEN` and store it with `npx wrangler secret put SETUP_TOKEN`. Generate the Android signing keystore with `keytool` and back it up outside the repo: lose it and you cannot update the app in place.
6. **Commit this plan.** Export this doc to Markdown, save it as `PLAN.md` in a new repo, and open Claude Code or opencode there. Agents read `PLAN.md` and `AGENTS.md` (starter below) first.

## Architecture

The app is local-first: every device holds the full notes database and works offline. The server is a dumb, cheap log that devices push to and pull from. All logic lives in UI-agnostic Rust crates, so the UI framework or the server host can be swapped without a rewrite.

[embedded content: architecture · 2 zones, 8 components]

Each device owns a full copy of your notes. The Worker only stores and serves updates and image chunks, so it stays within the 10 ms CPU limit.

**Why Dioxus.** Dioxus 0.7 targets web, desktop and Android from one Rust codebase, styled with plain CSS, and `dx serve --android` runs it on an emulator. Desktop and Android render in the system WebView, so the web app, Windows, Linux and Android all share one CSS design and look the same. Pin 0.7.x: the release notes I found list 0.8 only as alpha.

**Why Markdown in a text box.** A native `<textarea>` is the most reliable input on Android keyboards and IMEs, and stays fast on 100 KB notes. Toolbar buttons insert Markdown, checklists are `- [ ]`, images and links are Markdown, and a rendered view (`pulldown-cmark`) shows the result. A WYSIWYG or live-preview editor is a Phase 9 option that needs a small JS editor such as CodeMirror.

**Why Loro per note.** Each note is one Loro document: concurrent offline edits from two devices merge without a conflict dialog, and the edit log doubles as the history you asked for. The server never parses it; it stores opaque update blobs, which keeps it inside the free 10 ms CPU limit.

**Why one Worker.** A Worker written in Rust (`workers-rs` supports D1) serves the API and the web app from the same origin, so there is no CORS setup and one deploy.

| Option | Verdict | Reason |
| --- | --- | --- |
| Tauri 2 + Leptos | Fallback | Mature packaging and Android support, but two build pipelines (Trunk for web, Tauri for desktop and Android) |
| Slint | Rejected | Free to use and has an Android backend, but a native toolkit makes rich text, inline images and link editing the hard part |
| egui, iced | Rejected | Same editor problem; a polished text editor with images is the hardest widget to build in a native toolkit |
| R2 for images | Avoided | Reportedly needs a payment method even on the free tier |
| Server on your own machine or a VM | Fallback | You asked to avoid it; keep the server behind a trait so it stays possible |

## Free-tier budget

One person on three devices uses a few percent of every daily limit; the real constraints are CPU time and row sizes, not volume. These limits were read from Cloudflare's docs on 2026-10-20. Agents must re-check them before launch, because free tiers change.

| Limit (Workers and D1 free) | Value | What it forces | One-user estimate |
| --- | --- | --- | --- |
| Worker requests | 100,000 per day, 1,000 per minute | Sync once a minute while the app is visible, plus on focus and after edits | About 3,000 per day for 3 devices |
| Worker CPU per request | 10 ms (waiting on D1 is not counted) | No password hashing, no CRDT merging, no image work on the server | Token check and row reads only |
| D1 rows written | 100,000 per day; each index adds a written row | Debounce autosave to one push per 3 seconds idle; keep indexes minimal | Under 1,000 on a heavy day |
| D1 rows read | 5 million per day | Pull in pages of at most 100 rows | Under 50,000 |
| D1 database size | 500 MB per database, 5 GB per account | Two databases: notes text and images, so images cannot fill the notes DB | Notes: a few MB per year |
| Row or BLOB size | 2 MB maximum | Split attachments into chunks of 1 MB or less; compress images to WebP, longest side 1600 px | 200 to 400 KB per photo |
| Queries per request | 50 on free; 100 bound parameters per query | A push request carries at most 25 updates | Far below |
| Time Travel | 7 days on free | Server-side restore for the last week, on top of every device holding a full copy | Backup |
| Static assets | Free and unlimited | Serve the web app from the same Worker | Free |
| R2 | Needs a payment method | Not used | Not applicable |

Images are the only thing that can outgrow the free tier: 500 MB is roughly 1,500 compressed photos per database. The Settings screen shows a usage meter, and a third database can be added later without a protocol change.

## Data model and sync

Every note is one Loro document; the server stores an append-only log of its updates. Merging happens on devices, never on the server.

**Note document.** A `text` container holds the Markdown body. A `meta` map holds `notebook_id`, `tags`, `pinned`, `archived`, `color`, `created_at` and `trashed_at`. The title is the first line of the text. Deleting sets `trashed_at`; nothing is erased until you empty the trash. One extra `workspace` document holds the notebook list.

**Local database (SQLite).** Per note: id, Loro snapshot, last pushed version, title, preview, notebook, flags, timestamps. Plus a full-text index (FTS5 if every target supports it, otherwise an in-memory index), an attachments table, and one `sync_state` row with the pull cursor.

**Server database `sync` (D1).**

```sql
account(id INTEGER PRIMARY KEY CHECK (id = 1), username, auth_hash, created_at)
device(id, name, token_hash, created_at, last_seen)
update_log(seq INTEGER PRIMARY KEY AUTOINCREMENT, note_id, blob, device_id, created_at)
```

**Server database `blobs` (D1).** `blob_meta(hash, size, mime, chunks)` and `blob_chunk(hash, idx, data)`. Attachments are content-addressed by SHA-256, so uploads are idempotent and duplicates cost nothing.

| Endpoint | Purpose |
| --- | --- |
| `POST /v1/auth/setup` | One-time account creation; needs `SETUP_TOKEN` |
| `POST /v1/auth/login` | Username, login key, device name in; device token out |
| `POST /v1/sync` | Push up to 25 updates and a cursor in; new updates, next cursor and a `more` flag out |
| `PUT /v1/blob/{sha256}` and `GET /v1/blob/{sha256}` | Upload and download attachment chunks |
| `GET /v1/devices` and `DELETE /v1/devices/{id}` | List and revoke devices |

The wire format is JSON with base64 blobs: easy to debug with curl, and cheap enough at this volume. Every payload sits in an envelope with a version field, so end-to-end encryption can be added later without a migration.

**Client sync loop.** Run on start, on window focus or returning to the foreground, 3 seconds after the last edit, and every 10 seconds while visible; never while hidden (ADR-0020). Push each changed note's updates since its last pushed version, pull pages until `more` is false, import every update into the local document (importing twice is harmless), then save the cursor in the same transaction. There is no conflict screen: concurrent edits merge.

**History.** Turn on Loro's change timestamps. A version is a group of changes with no gap over 5 minutes from one device. The history view lists versions, shows a diff against the current text (`similar` crate), and previews any version read-only. Restore writes the old text as a new edit, so history only ever grows. Trash keeps notes until you empty it.

**Compaction (later).** A device uploads a full snapshot and the server deletes the updates it covers. Full history stays inside the snapshot.

**If Loro fails its spike.** Store a full-text revision per debounced save, merge diverging revisions with a 3-way diff, and keep both versions as a conflict copy when the diff cannot decide. The endpoints stay the same.

## Auth and device login

Login is username and password, but the password hashing happens on your device, not on the server. A free Worker gets 10 ms of CPU per request, and a proper password hash costs far more than that.

1. **First run.** `POST /v1/auth/setup` creates the single account and works only with the `SETUP_TOKEN` secret, so nobody else can claim a freshly deployed server.
2. **Login key.** The device runs Argon2id over the password, with a salt derived from the username, and sends the result. The server stores only a fast SHA-256 hash of that key. Argon2id runs in Rust on every target, including the browser. Set the memory and time cost in spike S3 so login stays under about one second on a mid-range phone.
3. **Device token.** The server returns a random 256-bit token, stores only its hash, and names the device. Native apps keep it in the OS keystore where one exists (Windows Credential Manager, Secret Service, Android Keystore). The web app uses an HttpOnly, SameSite=Strict cookie, since it shares the Worker's origin.
4. **Abuse limits.** Failed logins back off exponentially, counted in D1. Settings lists devices and revokes any of them.
5. **Recovery.** There is no email reset and no reset script. Losing the password while no device is signed in needs a deliberate disaster-recovery procedure with direct D1 access; `SETUP_TOKEN` creates the first account and does not reset an existing one.

The server URL is compiled into each build from a CI variable, so a fresh machine needs only the username and password. End-to-end encryption is a Phase 9 option: derive a second key from the same password with a different salt and encrypt every update blob before it leaves the device. The cost is that a lost password then means lost data.

## Repo layout and agent roles

One Cargo workspace. Each crate has one owner, so agents working in parallel do not collide.

```text
notes/
  AGENTS.md  CLAUDE.md  PLAN.md  README.md
  Cargo.toml                 workspace
  wrangler.toml              Worker, D1 bindings, static assets
  migrations/                D1 SQL, one folder per database
  crates/
    notes-proto/             wire types, ids, version envelope (wasm-safe)
    notes-core/              Loro docs, note model, history, search, Markdown
    notes-store/             NoteStore trait; SQLite and browser backends
    notes-sync/              sync engine, login, Argon2 key derivation, HTTP client
    notes-server/            Worker handlers behind a Storage trait; D1 impl
    notes-app/               Dioxus UI for web, desktop, Android; CSS tokens
  docs/
    STATUS.md                done, blocked, next, requests between agents
    adr/                     one decision record per spike
  scripts/                   reset-password, budget simulator, release helpers
  .github/workflows/         CI, web deploy, release builds
```

Run each agent in its own git worktree and branch. The orchestrator merges.

| Agent | Owns | Must not touch |
| --- | --- | --- |
| Orchestrator (main session) | `PLAN.md`, `docs/`, merges, gate runs | Feature code |
| Core | `notes-proto`, `notes-core`, `notes-store` | UI, server |
| Server | `notes-server`, `migrations/`, `wrangler.toml` | Client crates |
| Sync | `notes-sync` | UI, server handlers |
| UI | `notes-app`, CSS and design tokens | Core logic: calls traits only |
| Release | `.github/`, packaging, PWA, README | Feature code |
| QA reviewer | Nothing; read-only | Runs gates and reviews each diff against its acceptance checks, in a fresh context so it does not grade its own work |

An agent that needs an interface change writes a request in `docs/STATUS.md` and stops; it does not edit another agent's crate.

## Build phases

Phase 0 is a gate: five spikes that test the riskiest assumptions. After it, the Core, Server and UI agents run in parallel, and the Sync agent joins once Core and Server land. Every phase ends with the gates under Quality gates and budgets.

[embedded content: roadmap · 10 phases, 3 gates]

Core, Server and UI start together once the spikes pass. Sync joins when Core and Server land; history, images and release work follow.

### Phase 0: spikes

Each spike writes one decision record in `docs/adr/` with its measurements. If a result contradicts this plan, the agent stops and tells the orchestrator instead of improvising.

| Spike | Question | Pass when | Fallback |
| --- | --- | --- | --- |
| S1 | Does one Dioxus app boot on web, Linux, Windows and Android? | A hello app runs in the browser, on Linux, on Windows (built in CI) and on an Android emulator; sizes and cold start recorded | Tauri 2 + Leptos |
| S2 | Does Loro work natively, in WASM and on Android at a fair size? | Create, edit, export, import and time-travel work on all three; WASM adds about 1.5 MB or less (brotli); a 10,000-edit note stays small | Revision snapshots + 3-way merge |
| S3 | Does the Worker stay under 10 ms CPU, and is client-side Argon2 fast enough? | Login and sync deployed to the free account; 100 requests with no error 1102 in `wrangler tail`; Argon2id about 1 s or less on desktop, browser and a phone | Same server as axum on another free host |
| S4 | Can the browser keep a real database? | sqlite-wasm-rs with the OPFS sahpool VFS in a worker keeps 1,000 notes across reloads in Chrome, Firefox and Safari | IndexedDB-backed `NoteStore` |
| S5 | Can a text box drive a Loro text reliably? | A prefix and suffix diff of each input event keeps 1,000 randomized edits convergent; Gboard and Windows IME composition work; a remote update does not move the cursor | Edits sent on blur; revisit the editor |

### Phases 1 to 9

| Phase | Deliverables | Done when | Starts after |
| --- | --- | --- | --- |
| P1 Core and store | Note model, offline CRUD, notebooks, tags, pin, archive, trash, search, history API, Markdown render, export and import (zip of .md files, attachments and JSON) | Tests pass, including a property test: 3 simulated replicas with random edits and random merge order converge; 10,000 notes search in under 100 ms | P0 |
| P2 Server | Migrations, setup and login, sync, blobs, login backoff, deploy to the free account | End-to-end tests against `wrangler dev --local` pass; a simulated heavy day stays under 10% of every free limit | P0, parallel with P1 |
| P3 Sync engine | Login flow, token storage, push and pull loop, offline queue, retry with backoff, attachment transfer | Two headless clients and the real Worker converge after offline edits, deletes and restores; killing the network mid-sync loses nothing | P1, P2 |
| P4 App shell and editor | Design tokens, light and dark; responsive layout (one pane under 700 px, two from 700, three from 1100); Keep-style card grid and list view; Markdown toolbar, checklists, links, find in note, word count, undo and redo, shortcuts, command palette; login and settings screens | Screenshots at 360, 900 and 1400 px reviewed; usable end to end on Linux, web and an Android emulator against a mock core | S1, S5; parallel with P1 and P2 |
| P5 History and trash | Version timeline, diff, read-only preview, restore, trash view | A version restored on one device appears on another within a minute | P3, P4 |
| P6 Attachments | Paste, drop, file picker, Android camera; WebP compression; upload, lazy download, gallery | A photo added on the phone shows on the desktop; offline notes keep their cached images | P3, P4 |
| P7 Packaging and CI | Web deploy through Worker static assets; installable web app with offline cache; tagged builds of .msi or .exe, AppImage, .deb and a signed .apk; update-available notice; README with fresh-machine steps | Pushing a tag produces every artifact; a clean VM install passes the smoke test | CI skeleton after S1; finish after P6 |
| P8 Polish and performance | Budgets met, accessibility pass, large notes, import and export round trip, backup and restore drill | Wipe a device, log in, and get every note and its history back; every budget under Quality gates and budgets met | P5, P6, P7 |
| P9 Optional | End-to-end encryption, live sync over WebSocket with Durable Objects, server compaction, live-preview editor, note links and backlinks, Android share-to-app | Each ships behind a setting with its own decision record | P8 |

## Quality gates and budgets

The test that matters most is sync convergence: if devices ever disagree or lose text, the app is worthless. Agents write it before the sync code.

**Gates every agent runs before saying done**

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo nextest run --workspace` natively, and `wasm-bindgen-test` for the crates that compile to WASM
- Property test: 3 simulated devices, random edits, deletes and restores, random sync order and drops; all must end with identical text and history
- Server end-to-end tests with a real Worker under `wrangler dev --local`
- `cargo deny check` for licences and advisories, and a secret scan so nothing sensitive reaches git
- UI screenshots at 360, 900 and 1400 px, light and dark, reviewed by the QA agent
- `scripts/budget-check` against the table below
- Free-tier simulator: one heavy day of use stays under 10% of every daily limit

**Starting budgets.** These are targets for Phase 0 to confirm or adjust, then lock.

| Metric | Target |
| --- | --- |
| Desktop cold start to an editable note | Under 600 ms |
| Opening a note | Under 50 ms |
| Typing in a 100 KB note | No dropped frames |
| Search across 10,000 notes | Under 100 ms |
| Web first load (WASM, brotli) | 3 MB or less; works offline after the first load |
| Android APK | 25 MB or less |
| Desktop installer | 20 MB or less |
| Idle | No polling while hidden; near-zero CPU |

**Restore drill.** Before calling v1 done: wipe a device, log in, and confirm every note and its history returns. Then export D1 with `wrangler`, and confirm the app's own export opens as plain Markdown files.

## AGENTS.md starter

Save this as `AGENTS.md` in the repo root, which opencode reads. Add a `CLAUDE.md` containing the single line `@AGENTS.md` so Claude Code reads the same rules.

```markdown
# AGENTS.md

Personal local-first notes app in Rust: Dioxus UI, Loro CRDT, SQLite, Cloudflare Worker + D1.
Read PLAN.md first, then docs/STATUS.md.

## Hard rules
- Stay 100% free. Never add a paid service, a card-gated service (R2), or a server the owner must host.
- Server code runs on Workers Free: 10 ms CPU per request. No password hashing, CRDT merging or image processing there.
- notes-proto, notes-core, notes-store, notes-sync: no UI imports; must compile for native, wasm32-unknown-unknown and Android.
- Put logic behind traits (NoteStore, Storage, SyncTransport) so the UI framework or server host can be swapped.
- Never destroy notes. Edits append to history; deletes set trashed_at.
- No secrets in git. Use wrangler secrets and CI secrets.
- Use rustls, never OpenSSL (it breaks Android cross-compiles).
- Pin Dioxus to 0.7.x. Upgrade dependencies only with an ADR.
- Re-check Cloudflare free limits before any change that adds requests or writes.

## Workflow
1. Read your phase in PLAN.md and docs/STATUS.md.
2. Write tests first for merge, sync and history code.
3. Edit only the crates you own. For anything else, add a request to docs/STATUS.md and stop.
4. Run every gate below. Do not say done until all pass.
5. Update docs/STATUS.md: shipped, measurements, blocked, next.
6. If a result contradicts PLAN.md, write an ADR in docs/adr/ and stop.

## Gates
- cargo fmt --all --check
- cargo clippy --workspace --all-targets -- -D warnings
- cargo nextest run --workspace
- cargo deny check
- scripts/budget-check

## Commands
- dx serve --web | dx serve --desktop | dx serve --android
- npx wrangler dev --local | npx wrangler deploy
- npx wrangler d1 migrations apply <db> --local | --remote

## Style
- Small commits, one concern each.
- Typed errors (thiserror) in libraries; anyhow only in binaries and scripts.
- No unwrap() outside tests.
- UI uses CSS theme tokens only: no hard-coded colors or sizes.
```

## Kickoff prompts

Run the bootstrap prompt once, then use the phase template. Give each parallel agent its own git worktree. After every phase, run the review prompt in a fresh session.

**Bootstrap**

```text
Read PLAN.md. Create the Cargo workspace and crate skeletons from "Repo layout and agent roles", AGENTS.md from "AGENTS.md starter", CLAUDE.md containing @AGENTS.md, docs/STATUS.md, docs/adr/, wrangler.toml with two D1 bindings (sync and blobs), and a CI workflow that runs the gates. Implement no features. Run the gates and commit.
```

**Phase template**

```text
You are the <ROLE> agent on the notes app. Read AGENTS.md, PLAN.md (sections: <SECTIONS>) and docs/STATUS.md.
Task: <TASK>
Own only: <CRATES>. If you need a change elsewhere, add a request to docs/STATUS.md and stop.
Done when: <ACCEPTANCE FROM THE PHASE TABLE>. Run every gate in AGENTS.md first.
Finish by updating docs/STATUS.md with what shipped, measurements, and anything blocked.
```

**Fill-ins**

| Phase | ROLE | TASK | SECTIONS | CRATES |
| --- | --- | --- | --- | --- |
| S1 | UI | Prove one Dioxus app boots on web, Linux, Windows (in CI) and an Android emulator; record sizes and cold start; write ADR-001 | Build phases, Free-tier budget | `notes-app`, `.github/` |
| S2, S5 | Core | Prove Loro on native, WASM and Android, then the text box to Loro binding with IME tests; write ADR-002 and ADR-005 | Data model and sync, Build phases | `notes-core` on a spike branch |
| S3 | Server | Deploy login and sync to the free Cloudflare account, measure CPU per request and client-side Argon2 time; write ADR-003 | Auth and device login, Free-tier budget | `notes-server`, `notes-sync` |
| S4 | Core | Compare sqlite-wasm-rs in a worker with an IndexedDB store; write ADR-004 | Build phases | `notes-store` |
| P1 | Core | Build the P1 deliverables test-first, starting with the 3-replica property test | Data model and sync, Quality gates | `notes-proto`, `notes-core`, `notes-store` |
| P2 | Server | Build the P2 deliverables and the free-tier simulator | Data model and sync, Auth, Free-tier budget | `notes-server`, `migrations/`, `wrangler.toml` |
| P3 | Sync | Build the sync engine, then run the convergence test against the real Worker | Data model and sync, Auth | `notes-sync` |
| P4 | UI | Build the app shell and editor against a mock of the core traits | Architecture, Build phases | `notes-app` |
| P5, P6 | UI | Build history and trash, then attachments, on the real sync engine | Data model and sync, Build phases | `notes-app` |
| P7 | Release | Build CI, web deploy, installers, signed APK and the README | Build phases, Quality gates | `.github/`, `scripts/` |

**Review prompt (fresh session, read-only)**

```text
You are the QA reviewer. Do not edit code. Read PLAN.md, AGENTS.md and the diff on branch <BRANCH>. Run every gate. Check each "Done when" item for this phase against evidence, not claims. List failures, broken AGENTS.md rules and free-tier risks. Reply PASS or FAIL with reasons.
```

## Risks, fallbacks and sources

The biggest risks are the Android build and the browser database. Both are tested in Phase 0 before anything depends on them.

| Risk | Signal | Fallback |
| --- | --- | --- |
| Dioxus Android or WebView quirks, or 0.8 churn | S1 fails, or a 0.7.x bug blocks a core flow | Tauri 2 + Leptos; the core crates are unchanged |
| Loro too large or slow in WASM | S2 misses its size or speed target | Revision snapshots with 3-way merge and conflict copies |
| Worker exceeds 10 ms CPU | Error 1102 in S3 or later | Move the work to the client; otherwise run the same server crate as axum on another free host |
| Cloudflare changes its free tier | Limits differ from the Free-tier budget table | Every device holds a full copy; export D1 and the app's Markdown zip; the server sits behind a trait |
| Browser OPFS database unreliable | S4 fails in any major browser | IndexedDB-backed store |
| Android keyboard breaks the text box binding | S5 or P4 device testing shows lost or doubled text | Guard composition events; send edits on composition end |
| Images fill the 500 MB database | Usage meter above 80% | Stronger compression; a third D1 database |
| Lost password | You cannot log in | A deliberate recovery procedure with direct D1 access; data is kept unless end-to-end encryption is on |
| Markdown is not WYSIWYG | You miss Word-style formatting | Phase 9 live-preview editor, which needs a small JS dependency |

**Sources.** Figures come from search results for these pages, not full reads, so agents must re-check them before launch. The R2 card requirement is reported inconsistently across sources, which is why the plan avoids R2.

- [Workers limits](https://developers.cloudflare.com/workers/platform/limits/) and [Workers pricing](https://developers.cloudflare.com/workers/platform/pricing/)
- [D1 pricing](https://developers.cloudflare.com/d1/platform/pricing/) and [D1 limits](https://developers.cloudflare.com/d1/platform/limits/)
- [workers-rs](https://github.com/cloudflare/workers-rs) (the `worker` crate has a `d1` feature)
- [Dioxus 0.7 bundling and Android](https://dioxuslabs.com/learn/0.7/tutorial/bundle) and [Dioxus releases](https://github.com/DioxusLabs/dioxus/releases)
- [Tauri with Leptos](https://v2.tauri.app/start/frontend/leptos/)
- [Slint on Android](https://docs.slint.dev/latest/docs/slint/guide/platforms/mobile/android/)
- [Loro on docs.rs](https://docs.rs/crate/loro/latest/source/README.md) and [Loro 1.0](https://loro.dev/blog/v1.0)
- [sqlite-wasm-rs](https://github.com/nyurik/sqlite-wasm-rs)
- [R2 free tier listing](https://freetier.co/directory/products/cloudflare-r2)
