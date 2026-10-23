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
6. If the change affects the web app or Worker and is meant to be live, merge it,
   wait for CI, then run `./scripts/deploy-web.sh --if-needed` (see Deploy).
   Ask the owner first for auth, sync protocol or schema changes.
7. If a result contradicts PLAN.md, write an ADR in docs/adr/ and stop.

## Gates
- cargo fmt --all --check
- cargo clippy --workspace --all-targets -- -D warnings
- cargo nextest run --workspace
- cargo deny check
- scripts/budget-check

## Commands
- dx serve --web | dx serve --desktop | dx serve --android
- npx wrangler dev --local | ./scripts/deploy-web.sh
- npx wrangler d1 migrations apply <db> --local | --remote

## Cloud Shell restore
`/home` is a 4.8 GB partition and `/tmp/opencode` is wiped on every VM restart.
The boot hook runs `scripts/cloudshell-bootstrap.sh`; non-interactive agent
shells use `~/.config/notebox/bash-env.sh` through `BASH_ENV`.

- Check: `./scripts/cloudshell-bootstrap.sh --status`.
- Restore by hand: `./scripts/cloudshell-bootstrap.sh --force`. It refuses when
  `cargo`, `rustc`, `dx`, `nextest` or `worker-build` is running; pass
  `--allow-running` only when you intentionally stop a build first.
- Save seeds only after the gates and `./scripts/web-build.sh`, with no build
  running: `./scripts/cloudshell-seed.sh save --all`.
- `./scripts/cloudshell-seed.sh status` marks a seed `current` or
  `stale: ...`; save again after a `Cargo.lock`, toolchain or dx change.
- Keep 1200 MiB free on `/home`; `scripts/budget-check` keeps a 1200 MiB hard
  floor and checks every worktree target. If space is tight,
  `./scripts/cloudshell-seed.sh prune --target` removes the target seed first.
- Every worktree and clone must have its `target` under `/tmp/opencode`.
  `scripts/build-guard.sh` redirects new `cargo`/`dx` builds; check with
  `./scripts/disk-guard.sh --status`. If a real target is in `/home`, stop the
  build and run `./scripts/disk-guard.sh --fix`.
- The boot hook writes start/finish lines with the boot id to
  `~/.cache/notebox-build/boot.log`. A missing start line means the hook did
  not run; the `BASH_ENV` path still repairs on the first shell.
- The web build needs system `clang` for SQLite wasm. It is not in the seed;
  `scripts/web-build.sh` installs it on demand when non-interactive `sudo` is
  available.

See `docs/DISK.md`.

## Deploy
The live web app and the Worker ship together. Use the guarded script; never run
`npx wrangler deploy` directly, because it uploads whatever is in
`target/dx/...` and can publish a stale UI.

- `./scripts/deploy-web.sh --check` reports the live build, the committed diff
  since it, CI status and remote migration state. It changes nothing.
- `./scripts/deploy-web.sh --if-needed` deploys only when `origin/main` has
  runtime changes; it is a no-op for docs/CI/tooling-only commits and when live
  already matches.
- Deploy after a web or Worker change is merged and CI is green. Do not deploy
  docs, CI, test-only or unfinished phase work.
- Auth/session, sync protocol or D1 schema changes: ask the owner before
  deploying. If migrations are pending, apply them to both databases with
  `npx wrangler d1 migrations apply <db> --remote`, then rerun. Never edit an
  applied migration.
- The script refuses a dirty tree, a HEAD that is not `origin/main`, a missing
  or red CI run, and pending migrations. It builds with `NBX_BUILD_ID` and
  `NBX_GIT_SHA`, deploys, and verifies `/version.json`.
- If verification fails: `npx wrangler rollback`, then report the live build and
  what failed.

## Style
- Small commits, one concern each.
- Typed errors (thiserror) in libraries; anyhow only in binaries and scripts.
- No unwrap() outside tests.
- UI uses CSS theme tokens only: no hard-coded colors or sizes.
