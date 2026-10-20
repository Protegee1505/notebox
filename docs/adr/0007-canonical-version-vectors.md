# ADR-0007: Canonical version-vector encoding

- **Status:** accepted
- **Date:** 2026-10-20
- **Phase:** P3
- **Owner:** Store and sync

## Question

How does a stored per-note sync frontier decide, reliably, that the server has
exactly the operations the note already holds?

## Context

- The store keeps `local_vv` and `last_pushed` as opaque bytes on each note and
  on the workspace row. `unsynced_notes` asks SQL whether the two columns are
  the same value (`local_vv IS NOT last_pushed`); `note_was_clean` uses the same
  comparison before importing a pulled update.
- The sync engine decodes `last_pushed` to build `updates_since`.
- Loro's `VersionVector::encode` serialises an `FxHashMap` directly. Two
  vectors with identical peers and counters can therefore encode to different
  bytes when they were built through different insert histories.
- Byte inequality would make a clean note look dirty on every run: the queue
  would never empty, and the same update would be pushed again and again. The
  P3 Worker end-to-end test showed this intermittently before the fix.

## Decision

`notes-core` owns `encode_version_vector` and `decode_version_vector`. The
format is a sequence of entries sorted by peer: 8-byte big-endian peer, then a
4-byte big-endian counter. Every store column and every engine comparison uses
these helpers, never `VersionVector::encode`/`decode`.

`local_vv` and `last_pushed` remain byte columns. With a canonical encoding,
byte equality is vector equality, so the SQL comparison stays a single indexed
column check instead of decoding in application code.

## Consequences

- A note is queued exactly when its content version differs from the last
  version the server accepted. Pulled updates that arrive clean are recorded
  with the post-import vector, so they do not bounce back to the server.
- The encoding is stable across Loro versions and across native, wasm and
  Android builds, which the sync protocol depends on.
- P3 is unreleased, so no released database holds vectors in the old Loro
  encoding. A development database from an earlier P3 build would fail to
  decode them; setting `local_vv` and `last_pushed` to `NULL` re-pushes the
  notes once. No schema migration is shipped for that case.
- Changing this format later is a sync-compatibility change and needs a new
  ADR, like an Argon2 parameter change.
