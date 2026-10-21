# Notebox

Local-first personal notes app. One Rust codebase for web, Windows, Linux and Android;
Loro CRDT per note; SQLite locally; a Cloudflare Worker and D1 for sync.

The full specification and phase plan is [PLAN.md](PLAN.md). Agent rules are in
[AGENTS.md](AGENTS.md).

## Status

Phase 8: polish and performance. P1–P7 are complete except the owner-only
release execution. The note document, store, search, history, export, the
browser SQLite spike and the Worker sync server are implemented, and
`notes-sync` logs in with an Argon2id-derived key, pushes and pulls with a
durable cursor and retries with backoff. The Dioxus app has a responsive shell
(one, two and three panes), a Markdown editor with rendered preview, find, tags,
card and list views, command palette, shortcuts, login and settings, with light
and dark themes. Note history shows a version timeline, a diff and a read-only
preview, and restore writes the old text as a new edit; the trash view restores
notes and confirms before emptying; image attachments are compressed to WebP
and transferred by content hash. Export writes a zip of readable Markdown,
metadata and attachments; restore imports it from Settings, and the v2 archive
also carries each note's Loro history, so a fresh device gets the version
timeline back. The running app can sign in, run the sync loop on start, focus,
after an edit and every minute, show status, list and revoke devices, and keep
both bodies when an edit collides with a remote change. Native uses the SQLite
file; the web build keeps SQLite in memory and saves a versioned database image
to OPFS, so local notes, history and image bytes survive a reload without an
account. A second tab is held at an "already open" screen rather than writing
over the first.

The web build is also an installable PWA. A manifest, icons and a service worker
precache the hashed app shell, so it opens offline after the first visit. New
deployments wait until the user chooses Reload from the update banner; the old
worker never swaps under an open editor. Phase 8's accessibility, 100 KB note
and Linux cold-start budgets pass, and a local-Worker wipe/sign-in drill returns
every note and its history; the same drill against a deployed Worker is the
last P8 step. A remote deploy and the S3 CPU
measurement wait for `wrangler login`, and Android runtime testing still needs
an emulator. The tag-triggered release workflow builds Linux, Windows and web
artifacts; the Android job stays off until you enable it with
`RELEASE_ANDROID=true` and its signing secrets exist. See
[docs/STATUS.md](docs/STATUS.md), [docs/API.md](docs/API.md) and
[docs/adr/](docs/adr/).

## Personal server and offline use

A native build can carry one Worker URL so the owner only has to type a username
and passphrase. Set `NBX_SERVER_URL` when building:

```bash
NBX_SERVER_URL="https://notebox.example.workers.dev" \
  cargo run -p notes-app --features desktop
```

The release workflow passes the repository variable `NBX_SERVER_URL` to the
Linux and Windows bundles. Set it before tagging to make the installers open
with that server already filled in. The URL is visible in the binary and on the
login screen; it is an address, not a secret. Leave the variable unset for a
build that starts blank. The web build always uses the origin that serves it.

The login screen keeps the server field editable on desktop, with a "Use the
built-in server" action, and shows the fixed origin on web. "Try it offline — no
account" is always available: notes, history, images and export work locally
with no Worker. To sync with your own server, deploy your own Worker, create
its single account, and enter its URL on desktop. On web, deploy your own copy
of the app instead; sync is same-origin by design and there is no CORS surface.
There is no in-app sign-up: only the one account created with the setup token
can sign in.

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
./scripts/web-build.sh
node scripts/web-budget.mjs target/dx/notebox/release/web/public
```

`web-build.sh` clears `dx`'s old hashed files, builds the release web app and
runs `pwa-build.mjs`, which adds `manifest.webmanifest`, the icons, `_headers`,
`sw.js` and `version.json`. The service worker precaches the current hashed
assets. `pwa-build.mjs` reads the Dioxus asset manifest, so stale files left by
an earlier build are not cached.

The browser checks use Playwright:

```bash
PLAYWRIGHT_MODULES=/path/to/node_modules \
  node scripts/web-persist.mjs
PLAYWRIGHT_MODULES=/path/to/node_modules \
  node scripts/pwa-check.mjs
PLAYWRIGHT_MODULES=/path/to/node_modules \
  node scripts/web-restore.mjs
PLAYWRIGHT_MODULES=/path/to/node_modules \
  node scripts/web-a11y.mjs
PLAYWRIGHT_MODULES=/path/to/node_modules \
  node scripts/web-perf.mjs
PLAYWRIGHT_MODULES=/path/to/node_modules \
  node scripts/web-wipe-login.mjs
```

`web-persist.mjs` restarts Chromium and checks local notes, images and history.
`web-restore.mjs` exports a backup in one profile and restores it in a fresh
one, including its version history. `web-a11y.mjs` runs axe-core against every
main screen in light and dark themes, and checks the note list's arrow keys,
that Tab stays inside each dialog, and focus return after closing a panel;
install `axe-core` beside Playwright.
`web-perf.mjs` creates a 100 KB note, measures its open time and typing frame
health, and reopens it after a reload. `web-wipe-login.mjs` starts
`wrangler dev --local` (install the matching `worker-build` first), wipes a
profile's OPFS database and session, signs in, and checks that the note and its
history come back. `pwa-check.mjs` checks installability, an offline reload, an
offline edit, and a simulated second build that waits for the update banner.
Add `NBX_PWA_SCREENSHOTS=1` to refresh `docs/screenshots/p7-pwa-*`.

Measure the Linux desktop cold start from a release build with
`./scripts/desktop-cold-start.sh`; it boots a private Xvfb display and an
isolated data directory, so it does not touch your notes.

## Web app and offline use

The Worker serves the built web app from the same origin as sync. To install
it, open the deployed URL in a browser and use its "Install" action. The first
visit caches the app shell; later visits work without a network connection,
including notes stored in OPFS. Sync requests are never cached: when offline,
the app shows the local status and retries when the network returns. The web
login screen shows the serving origin and does not offer a different server, so
self-hosters deploy their own copy of the web app on their own Worker.

When a new deployment is published, the old service worker keeps serving the
current build. The next check happens on load or when the tab becomes visible;
if a new worker is waiting, a banner offers **Reload**. Choosing it saves the
open note, activates the new worker and reloads. There is no automatic reload
while you are typing.

`Settings → About` shows the package version and, in CI builds, the build and
commit identifiers. Set `NBX_BUILD_ID` and `NBX_GIT_SHA` when building to stamp
them; local builds say `dev`.

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
./scripts/web-build.sh
npx wrangler dev --local --var SETUP_TOKEN:local-dev-token
```

Open the Worker origin (`http://127.0.0.1:8787`) to use the web app. Sync is
same-origin by design, so a `dx serve` page on another port cannot reach the
Worker: open the build the Worker serves instead, or use the desktop app.
Restart `wrangler dev` after every `dx build`: Wrangler caches the asset
manifest, so a running dev server can 404 the newly hashed files.

Create the local account once, while the dev setup token is present:

```bash
NBX_SETUP_TOKEN=local-dev-token ./scripts/create-account \
  --server http://127.0.0.1:8787 --username alice
```

The script asks for the passphrase twice and never prints it. It removes its
temporary setup device, so the account starts clean. There is no in-app sign-up;
this is also the command for a deployed self-hosted Worker.

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
NBX_BUILD_ID="$(git rev-parse --short HEAD)" \
  NBX_GIT_SHA="$(git rev-parse HEAD)" \
  ./scripts/web-build.sh
npx wrangler deploy
```

The Worker serves the built web app through `[assets]` in `wrangler.toml`, so
open the deployed URL itself rather than a separate static host. Run
`web-build.sh` before every deploy: it writes the service worker, manifest,
icons and `_headers` that make the app installable and offline-ready.

After deploying, create the single account while `SETUP_TOKEN` still exists:

```bash
NBX_SETUP_TOKEN="the value you just set" ./scripts/create-account \
  --server https://notebox.<your-subdomain>.workers.dev --username alice
npx wrangler secret delete SETUP_TOKEN
```

The second command closes setup permanently: `POST /v1/auth/setup` then answers
`503`, and only the existing account can sign in. There is no in-app sign-up
and no second account. For personal installers, set the URL as a repository
variable before tagging:

```bash
gh variable set NBX_SERVER_URL \
  --body "https://notebox.<your-subdomain>.workers.dev"
```

Then measure CPU time for the S3 spike with `npx wrangler tail` and write
`docs/adr/0003-*.md`. See [docs/API.md](docs/API.md) for the request shapes.

## Releases

Pushing a tag such as `v0.1.0` starts
[.github/workflows/release.yml](.github/workflows/release.yml). The tag must
match the `notes-app` version in `Cargo.toml`; the workflow stops before
building otherwise. It runs the gates on a clean runner, then builds on native
runners. The workflow has not run on GitHub yet; the first tag is its first
full exercise:

- Linux x86_64: `.deb` and `.AppImage`
- Windows x64: `.msi`, an NSIS setup `.exe` and a portable `.exe`
- Web: an installable PWA `.zip` with the service worker and manifest
- Android arm64: a signed release `.apk`, only when `RELEASE_ANDROID=true`

Set `NBX_SERVER_URL` as a repository variable before tagging to pre-fill the
server in the Linux and Windows builds. Without it, the login screen starts
with an empty URL and the app works offline. The Android job is skipped by
default, so a Windows/Linux/web release does not need the signing key.

The Linux `.deb` and AppImage are installed and booted for 20 seconds on a
clean runner. Windows installs the `.msi` and boots the installed app for 20
seconds. When Android is enabled, the APK is verified with `apksigner`, the
package name and a non-debuggable manifest check; Android runtime testing still
needs an emulator or phone. The web bundle runs the same PWA checks as CI. Every
artifact is published on a GitHub Release with `SHA256SUMS`.

```bash
# Linux
sudo apt install ./notebox_0.1.0_amd64.deb
./notebox_0.1.0_x86_64.AppImage
# Windows: run the .msi or *-setup.exe, or the portable .exe
# Android: sideload the signed .apk
# Web: deploy the unzipped public directory with wrangler
sha256sum -c SHA256SUMS
```

The 20 MB desktop-installer budget is measured on the `.deb` (9.9 MB), which
uses the system WebKitGTK. The self-contained AppImage bundles WebKitGTK, so it
is larger (89.6 MB) and is outside that budget by ADR-0015.

### Android signing

Android is not part of the initial Windows/Linux/web release. When you set the
repository variable `RELEASE_ANDROID=true`, the release job refuses to publish
an APK unless all four signing secrets are present. Create the keystore once and
keep it and its passwords backed up: losing the key prevents in-place updates.

```bash
keytool -genkeypair -v \
  -keystore notebox-release.jks \
  -alias notebox \
  -keyalg RSA -keysize 4096 -validity 10000
# Use a hex password (no quotes or backslashes) because Dioxus writes it into
# the generated Gradle file.
base64 -w0 notebox-release.jks > notebox-release.jks.b64

gh secret set ANDROID_KEYSTORE_BASE64 < notebox-release.jks.b64
gh secret set ANDROID_KEYSTORE_PASSWORD
gh secret set ANDROID_KEY_ALIAS
gh secret set ANDROID_KEY_PASSWORD
gh variable set RELEASE_ANDROID --body true

git tag -a v0.1.0 -m "Notebox 0.1.0"
git push origin v0.1.0
```

Windows installers are not code-signed, so SmartScreen may warn; there is no
paid certificate in this project. The Worker deploy remains manual: the release
web `.zip` is a tested artifact, not an automatic deploy.

## Free-tier rule

The app must stay inside Cloudflare's free plan. Do not enable R2 and do not add a
service that needs a payment method. Sign-up steps and account actions are listed in
PLAN.md under "Do this first".
