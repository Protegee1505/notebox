# Notebox

Local-first personal notes app. One Rust codebase for web, Windows, Linux and Android;
Loro CRDT per note; SQLite locally; a Cloudflare Worker and D1 for sync.

The full specification and phase plan is [PLAN.md](PLAN.md). Agent rules are in
[AGENTS.md](AGENTS.md).

## Status

Phase 0 spikes. The note document (Loro 1.16.2), the text-box binding, the
web/desktop boot checks and the browser SQLite store (OPFS in a worker) are
implemented. No full app features yet. See [docs/STATUS.md](docs/STATUS.md) and
[docs/adr/](docs/adr/) for progress and decision records.

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
./scripts/wasm-size.sh
dx build -p notes-app --platform web --release --features web
node scripts/web-budget.mjs target/dx/notebox/release/web/public
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

If `/home` is a small partition, keep build artifacts and caches on the larger
disk before building; the script is idempotent and also creates the local Cargo
target setting:

```bash
./scripts/relocate-caches.sh --force
```

See [docs/DISK.md](docs/DISK.md) for the layout, the 512 MiB free-space guard,
and recovery steps.

## Free-tier rule

The app must stay inside Cloudflare's free plan. Do not enable R2 and do not add a
service that needs a payment method. Sign-up steps and account actions are listed in
PLAN.md under "Do this first".
