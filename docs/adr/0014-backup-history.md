# ADR-0014: Backups carry note history

- **Status:** accepted
- **Date:** 2026-10-21
- **Phase:** P8
- **Owner:** Core and app

## Question

How does a backup restore the version timeline as well as the current text,
without breaking archives written by P1, without giving up the plain-Markdown
escape hatch, and without letting a restore overwrite history?

## Context

- ADR-0006 deferred history export to P5/P8. The manifest was versioned for
  exactly this change, and P1 archives are still in the wild.
- A note's history lives in its Loro document, not in the `.md` file. The
  Markdown body and the `meta` map are projections of that document.
- A restore may land on a device that already has edits to the same note. Those
  edits must stay in history; nothing may be discarded.
- The archive must stay readable with ordinary tools and must be produced by
  pure Rust on native, wasm and Android.
- Import runs on the UI thread. A full snapshot per note makes an archive larger
  and an import slower than v1, but backups are an explicit action, not a
  request on the sync hot path.

## Decision

Bump the archive manifest to version 2 and add one optional full Loro snapshot
per note:

```
manifest.json
notes/<note-id>.md
history/<note-id>.loro       (optional)
attachments/<sha256>.<ext>   (optional)
```

- The Markdown file still holds the current text and remains the human-readable
  copy. The snapshot is an additional entry, not a replacement.
- The manifest note gains an optional `snapshot` path. Version 2 writers validate
  that the path is exactly `history/<note-id>.loro`; readers reject traversal,
  missing entries and versions newer than this build.
- Readers accept version 1 archives. Their notes have no snapshot and import
  exactly as before, creating a new local history entry for the restored text.
- Export takes `Note::document().snapshot()`. It is a full snapshot, so it has
  no dependency on any other file or on the sync log.
- Import restores a new note with `Note::from_snapshot`, then sets the local
  peer id for future edits. Import merges a snapshot into an existing note
  with `LoroDoc::import`; it never replaces the document. If the merged current
  text differs from the archive's Markdown, the archive text is applied as a
  normal edit, so the pre-restore text and the concurrent local edits remain in
  the timeline.
- Import remains one SQLite transaction. A malformed snapshot rolls the whole
  restore back with a typed error.
- The Settings restore flow reads and validates the archive before showing
  counts, then imports only after confirmation. Importing the same archive
  twice is idempotent for notes, history and attachments.

## Consequences

- P8's wipe-and-restore drill can verify the history panel after a restore, not
  just the current note text.
- Archives are larger by roughly one snapshot per note and take longer to
  import. The budget check does not measure backups because they are explicit
  and unbounded by design; the web build size is unchanged.
- A v1 archive still restores current notes but cannot recreate their timeline.
  v2 archives that are read by a future version remain forward-compatible only
  through the version check; a newer version is refused rather than misread.
- The snapshot preserves original peer ids and timestamps, so the history view
  labels restored versions as another device. That is accurate and is what the
  browser test asserts.
- A future format can replace full snapshots with a base snapshot plus updates,
  or omit snapshots for notes without history, without changing the v2 reader.
