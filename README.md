# Notebox

Local-first personal notes app. One Rust codebase for web, Windows, Linux and Android;
Loro CRDT per note; SQLite locally; a Cloudflare Worker and D1 for sync.

The full specification and phase plan is [PLAN.md](PLAN.md). Agent rules are in
[AGENTS.md](AGENTS.md).

## Status

Phase 7: app sync wiring. The note document, store, search, history, export,
the browser SQLite spike and the Worker sync server are implemented, and
`notes-sync` logs in with an Argon2id-derived key, pushes and pulls with a
durable cursor and retries with backoff. The Dioxus app has a responsive shell
(one, two and three panes), a Markdown editor with rendered preview, find,
tags, card and list views, command palette, shortcuts, login and settings,
with light and dark themes. Note history shows a version timeline, a diff and
a read-only preview, and restore writes the old text as a new edit; the trash
view restores notes and confirms before emptying; image attachments are
compressed to WebP and transferred by content hash. The running app can sign
in, run the sync loop on start, focus, after an edit and every minute, show
status, list and revoke devices, and keep both bodies when an edit collides
with a remote change. Native uses the SQLite file; the web build keeps SQLite
in memory and saves a versioned database image to OPFS, so local notes, history
and image bytes survive a reload without an account. A second tab is held at an
"already open" screen rather than writing over the first. A remote deploy and
the S3 CPU measurement wait for `wrangler login`, and Android runtime testing
still needs an emulator. See
[docs/STATUS.md](docs/STATUS.md), [docs/API.md](docs/API.md) and
[docs/adr/](docs/adr/).

## Layout

```text
crates/
  notes-proto/    wire types, ids, version envelope (wasm-safe)
  notes-core/     Loro documents, note model, history, search, Markdown
  notes-store/    NoteStore trait; SQLite and browser backends
  notes-sync/     sync engine, login, Argon2 key derivation, HTTP client
  notes-server/   Worker handlers behind a Storage trait; D1 implementation
  notes-app/      Dioxus UI for web, desktop and Android
migrations/       D1 migrations, one folder per database
docs/             status and architecture decision records
```

## Local commands

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo deny check
./scripts/budget-check
python3 scripts/free-tier-sim.py
./scripts/wasm-size.sh
# dx leaves old hashed files in the output; clear it for a clean bundle.
rm -rf target/dx/notebox/release/web/public
dx build -p notes-app --platform web --release --features web
node scripts/web-budget.mjs target/dx/notebox/release/web/public
PLAYWRIGHT_MODULES=/path/to/node_modules \
  node scripts/web-persist.mjs target/dx/notebox/release/web/public
```

The UI has no default renderer. Select one explicitly:

```bash
cargo run -p notes-app --features desktop
cargo check -p notes-app --no-default-features --features web --target wasm32-unknown-unknown
```

Run the Loro wasm test in Node after installing the matching
`wasm-bindgen-test-runner`:

```bash
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test -p notes-core --target wasm32-unknown-unknown --test wasm_smoke
```

The S4 browser database probe builds SQLite for wasm (needs clang), then drives
Chromium, Firefox and WebKit through a reload:

```bash
# wasm-bindgen must match Cargo.lock (0.2.129 currently)
WASM_BINDGEN=/path/to/wasm-bindgen \
PLAYWRIGHT_MODULES=/path/to/node_modules \
PLAYWRIGHT_BROWSERS_PATH=/path/to/playwright-browsers \
  node scripts/opfs-spike.mjs
```

The script uses a persistent browser profile per run because WebKit refuses
OPFS handles in an ephemeral context. Pass `--browser chromium` to shorten the
loop and `--skip-build` to reuse the last build.

The app-level persistence check uses the same persistent-profile idea. It
creates a local-only note with a PNG, restarts Chromium, and verifies the text,
image reference, attachment row, decoded WebP bytes and history panel. It also
opens a second tab and expects the store lock screen.

```bash
PLAYWRIGHT_MODULES=/path/to/node_modules \
  node scripts/web-persist.mjs
```

The release build must already exist; the script accepts an explicit public
directory when `dx` wrote outside `target/dx`.

If `/home` is a small partition, keep build artifacts and caches on the larger
disk before building; the script is idempotent and also creates the local Cargo
target setting:

```bash
./scripts/relocate-caches.sh --force
```

See [docs/DISK.md](docs/DISK.md) for the layout, the 512 MiB free-space guard,
and recovery steps.

## Local sync server

Install `worker-build` once (the Worker build runs it), then apply both D1
migrations and start Wrangler:

```bash
cargo install worker-build --version 0.8.7 --locked
npx wrangler d1 migrations apply notebox-sync --local
npx wrangler d1 migrations apply notebox-blobs --local
# Serve the real web app from the Worker (optional for the API scripts):
dx build -p notes-app --platform web --release --features web
npx wrangler dev --local --var SETUP_TOKEN:local-dev-token
```

Open the Worker origin (`http://127.0.0.1:8787`) to use the web app. Sync is
same-origin by design, so a `dx serve` page on another port cannot reach the
Worker: open the build the Worker serves instead, or use the desktop app.
Restart `wrangler dev` after every `dx build`: Wrangler caches the asset
manifest, so a running dev server can 404 the newly hashed files.

Run the full HTTP test, including setup, login backoff, sync pagination,
attachment chunks and device revocation, against a throwaway local Worker:

```bash
node scripts/server-e2e.mjs
```

Then run the client gate: the same kind of throwaway Worker, driven by two
headless `notes-sync` devices that converge after offline edits, trash and
restore a note, rename a notebook, transfer an attachment and revoke a device.

```bash
./scripts/sync-e2e.sh
```

Both scripts need Node 20+, `npx`, and `worker-build` on `PATH` (`~/.cargo/bin`
if you installed Rust with rustup). They download no account resources.

## Deploying the free Worker

These steps need the owner's Cloudflare login; agents cannot run them.

```bash
npx wrangler login
npx wrangler d1 create notebox-sync     # copy the database_id
npx wrangler d1 create notebox-blobs    # copy the database_id
# put both ids in wrangler.toml, replacing the placeholder UUIDs
npx wrangler d1 migrations apply notebox-sync --remote
npx wrangler d1 migrations apply notebox-blobs --remote
npx wrangler secret put SETUP_TOKEN     # generate a long random value
rm -rf target/dx/notebox/release/web/public
dx build -p notes-app --platform web --release --features web
npx wrangler deploy
```

The Worker serves the built web app through `[assets]` in `wrangler.toml`, so
open the deployed URL itself rather than a separate static host.

After deploying, call `POST /v1/auth/setup` once with the setup token to create
the account. Then measure CPU time for the S3 spike with `npx wrangler tail` and
write `docs/adr/0003-*.md`. See [docs/API.md](docs/API.md) for the request
shapes.

## Free-tier rule

The app must stay inside Cloudflare's free plan. Do not enable R2 and do not add a
service that needs a payment method. Sign-up steps and account actions are listed in
PLAN.md under "Do this first".
