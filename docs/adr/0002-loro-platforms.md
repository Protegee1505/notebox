# ADR-0002: Loro on native, wasm and Android

- **Status:** proposed
- **Date:** 2026-10-20
- **Spike:** S2
- **Owner:** Core

## Question

Does one Loro document per note create, edit, export, import and time-travel on
native, `wasm32-unknown-unknown` and Android, at a size that fits the web
budget and the 2 MB D1 row limit?

## Context

- `loro` 1.16.2, `wasm-bindgen` 0.2.129, Rust 1.99.1.
- Node 24.21 runs the wasm tests; no browser is needed for the CRDT itself.
- Android uses NDK r27c and the `aarch64-linux-android` target.
- History timestamps are enabled with a 5-minute merge interval, matching
  PLAN.md's version grouping.

## Decision

Use Loro 1.16.2 as the note document. Each note has a `text` root container for
the Markdown body. `NoteDocument` wraps the document, re-exports the Loro types
that cross crate boundaries, and keeps all snapshot/update handling opaque.

## Evidence

Native (`cargo nextest run -p notes-core`): 17 tests pass.

- Create, edit, snapshot export, snapshot import and time travel
  (`tests/convergence.rs`).
- Three replicas, 120 random edits, random one-way syncs and dropped updates,
  followed by a full mesh in random order: all replicas end with identical text
  and version vector.
- 1,000 randomized whole-value text-box edits converge and survive a snapshot
  round trip.
- 10,000 edits: 46,000 characters, 100 commits. All commits fall inside one
  five-minute merge window, so Loro reports one change. Snapshot is **998
  bytes**; all updates are **46,106 bytes**, far below the 2 MB row cap.

WASM (`wasm-bindgen-test` in Node): create, edit, export, import and time travel
pass. The full native suite is not registered on wasm because only the smoke
test uses `#[wasm_bindgen_test]`.

Size probe (`scripts/wasm-size.sh`, release, brotli quality 11):

| Build | Raw | Brotli | Gzip |
| --- | ---: | ---: | ---: |
| baseline (no Loro) | 26,840 | 8,934 | 10,536 |
| note document + Loro | 3,848,457 | 720,987 | 1,051,805 |
| **delta** | **3,821,617** | **712,053** | **1,041,269** |

The CRDT adds about **0.68 MiB brotli**, under the 1.5 MB budget. The complete
web app measured separately is 0.37 MiB brotli because wasm-opt and dead-code
elimination remove most of what the probe links.

Android: `cargo check -p notes-proto -p notes-core -p notes-store -p notes-sync
--target aarch64-linux-android` passes with NDK r27c. The CI `android` job
repeats this. Runtime create/edit/export/import on an Android device or emulator
is **not tested**: this machine has no `/dev/kvm` and no Android SDK.

Dependency note: Loro 1.16.2 pulls four unmaintained transitive crates
(`im`, `sized-chunks`, `bitmaps`, `atomic-polyfill`). They have no safe upgrade;
`deny.toml` ignores their advisories with a comment, and this must be revisited
when Loro moves to `imbl`.

## Consequences

- S2 is not fully passed: native and wasm pass, Android compiles but has no
  runtime proof yet. The fallback (revision snapshots + 3-way merge) is not
  triggered.
- One note snapshot fits comfortably in a D1 row; the SQLite row stores the
  snapshot and the last pushed version vector.
- Import must be followed by a check of `ImportStatus::pending`; the sync
  engine cannot assume every update is self-contained.
- The `deny.toml` exceptions are a tracked risk, not a permanent policy.
