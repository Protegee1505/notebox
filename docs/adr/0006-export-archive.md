# ADR-0006: Export archive — a zip of Markdown and JSON

- **Status:** accepted
- **Date:** 2026-10-20
- **Phase:** P1
- **Owner:** Core

## Question

How does a note database leave the app without locking the owner in: one file
that a person can open with ordinary tools, and that the app can import again
without losing note ids, notebooks, tags or history?

## Context

- P1 requires export and import of a zip containing `.md` files, attachments
  and JSON.
- The core crates compile for native, `wasm32-unknown-unknown` and Android, so
  the archive code must be pure Rust and must not pull in OpenSSL or a C
  compression library with a cross-compile toolchain.
- `zip` 9.0.5 supports a `deflate-flate2-zlib-rs` feature, which uses the pure
  Rust `zlib-rs` backend. Its declared MSRV is Rust 1.88; the workspace claimed
  1.85.
- Import must never duplicate notes on a second run, and the archive is also
  the backup path when browser storage is unavailable (ADR-0004).

## Decision

Export a zip with:

```
manifest.json
notes/<note-id>.md
attachments/<sha256>.<ext>   (optional)
```

`manifest.json` is versioned and carries note metadata (title, notebook, tags,
pin, archive, color, timestamps, trash state), the notebook list and the
attachment index. Each Markdown file holds the note's current text; the note id
in the path is the stable identity. History is not exported in P1: the
`manifest.json` format version leaves room for a later history section.

Import validates the manifest version, rejects absolute paths and `..` path
traversal, caps the entry count and total uncompressed size, preserves note
ids, and updates an existing note instead of inserting a duplicate. A note
whose id is new is created through the same core path as an edit, so history
records the import.

Use `zip` 9.0.5 with only `deflate-flate2-zlib-rs`; disable default features so
zopfli and the other codecs do not grow the wasm build. The workspace
`rust-version` moves from 1.85 to 1.88 to match. `zip` is the only runtime
dependency added for P1 after the spike ADRs; `tempfile` is test-only.

## Consequences

- One archive opens with `unzip`; notes are readable without Notebox, and the
  JSON is enough to rebuild the app's metadata.
- The compression backend is pure Rust, so Android and wasm cross-compiles do
  not need a C zlib or OpenSSL.
- Rust 1.88 is the minimum supported toolchain. CI and the development
  toolchain are newer (1.99.1); an older toolchain now fails with a clear
  `rust-version` error rather than a confusing build failure.
- Attachments are referenced by content hash, so exporting the same bytes with
  the same extension stores one archive entry, and two files with the same
  display name cannot overwrite each other. P6 will reuse this index for upload
  and lazy download.
- History export is deliberately deferred; a backup restores current notes,
  not the version timeline. P5/P8 can add it under a new manifest version
  without breaking P1 archives.
- P1 stores attachment metadata only, so its archive entries are empty until
  P6 stores the bytes. The path scheme and manifest already carry the fields
  P6 needs.
