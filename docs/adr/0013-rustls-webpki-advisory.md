# ADR-0013: Patch rustls-webpki for RUSTSEC-2026-0382

- **Status:** accepted
- **Date:** 2026-10-21
- **Phase:** P7 dependency security
- **Owner:** sync

## Question

How should the workspace respond to the `RUSTSEC-2026-0382` advisory that
`cargo deny check` reports for `rustls-webpki` 0.103.15, without changing the
pinned Dioxus version or ignoring the advisory?

## Context

- The native sync path is `notes-sync` -> `ureq` 3.4.2 -> `rustls` 0.23.46 ->
  `rustls-webpki` 0.103.15.
- The advisory says 0.103.15 may accept certificate paths containing distrusted
  emSign intermediate public keys when those keys are already trusted as
  anchors. 0.103.16 rejects those paths.
- `cargo deny check` is a required gate. Ignoring the advisory would keep a
  known certificate-validation defect in a network client whose whole job is
  authenticated sync.
- Dioxus stays pinned at 0.7.10. This is a patch to a transitive TLS
  dependency, not a framework upgrade.
- There is no API or database change, and the Android/wasm targets do not use
  this native transport.

## Decision

Update the lockfile only:

```sh
cargo update -p rustls-webpki --precise 0.103.16
```

Keep `ureq` 3.4.2 and `rustls` 0.23.46. Do not add a `deny.toml` ignore for
this advisory and do not change `Cargo.toml`.

## Evidence

- `Cargo.lock`: `rustls-webpki` 0.103.15 -> 0.103.16, checksum updated.
- `cargo deny check`: advisories, bans, licences and sources all pass.
- `cargo nextest run --workspace`: 206 passed, 1 skipped after the update.
- `cargo clippy -p notes-app --no-default-features --features desktop -- -D
  warnings` and the wasm32 web check pass.
- `./scripts/budget-check` passes.

## Consequences

- The fix is a semver-compatible patch with no source changes. Android and
  browser builds are unaffected because they do not use this transport.
- A future dependency update should remove this note if `rustls-webpki` moves
  to 0.104 or a newer 0.103 patch.
- No paid service, Worker limit or free-tier budget changes.
