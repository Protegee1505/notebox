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
