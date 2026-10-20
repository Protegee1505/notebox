# ADR-0008: Attachments — content hash, WebP ingest and local bytes

- **Status:** accepted
- **Date:** 2026-10-20
- **Phase:** P6
- **Owner:** Core, Store, App

## Question

How does a photo get from a camera, clipboard or file picker into a note, reach
the other devices, and survive an export, while every crate still builds for
native, `wasm32-unknown-unknown` and Android without a C toolchain?

## Context

- P6 requires paste, drag and drop and a file picker; images downscaled to a
  sensible size; Markdown that still shows the image; and a gallery of the
  note's attachments.
- ADR-0006 already writes attachments to `attachments/<sha256>.<ext>` in an
  export. A note reference to the same relative path makes export a copy and
  import a no-op rewrite.
- The D1 blob store is split into 1 MiB chunks and a D1 row may not exceed
  2 MB. The local SQLite file has no such limit.
- `image-webp` (the `image` crate's WebP encoder) is lossless only. A phone
  photo would stay several megabytes. `libwebp`/`webp` bindings need a C
  toolchain for wasm and Android, which P0 deliberately avoided.
- The browser store is still in memory (ADR-0004/P7), so a web reload currently
  loses notes and attachments alike.

## Decision

### Identity and reference

Every attachment is identified by the lowercase SHA-256 of its bytes. A note
refers to it with an ordinary relative Markdown URL:

```markdown
![holiday](attachments/9f2c…e1.webp)
```

The path is identical inside an export archive. The display name and MIME type
live in the attachment table; the extension is derived from the MIME type, so
the URL always matches the stored bytes even when a camera named a WebP
`IMG_0001.PNG`.

The Markdown body is the source of truth for which attachments a note uses.
`save_note` parses image and link destinations, rebuilds the `note_attachment`
index, and creates a placeholder row for a referenced hash that has no bytes
yet. Removing the Markdown reference removes the note's use of the attachment;
the bytes remain for another note or a later download. Attachment bytes are
never deleted by an edit.

### Local schema v4

```sql
attachment(hash TEXT PRIMARY KEY, name, mime, size, data BLOB NULL,
           created_at, uploaded_at INTEGER NULL)
note_attachment(note_id, hash, added_at, PRIMARY KEY(note_id, hash))
```

The whole blob is one local row: SQLite streams large values through overflow
pages and every caller already needs all bytes for preview or upload. The P1
metadata-only table is renamed `attachment_legacy` rather than destroyed.
`data IS NULL` is a placeholder for a link learned from another device;
`uploaded_at IS NULL` marks bytes the Worker has not seen.

### Ingest

`image` 0.25.10 decodes JPEG, PNG, GIF and WebP. EXIF orientation is applied
before resizing. The longest side is capped at 1600 px. `webp-rust` 0.3.2
(MIT, pure Rust) encodes lossy WebP at quality 80, 68 or 56 until the result is
at most 400 KB; even the last attempt may not exceed 2 MB. Input is capped at
25 MB before decoding. Animated GIFs keep their first frame. SVG is refused
because it can carry script and sanitising it is a separate problem.

The same pure-Rust code runs in the browser, the desktop WebView and Android,
so image ingest needs no NDK or `libwebp` build and no server CPU.

### Display

The rendered note receives inline `data:image/…;base64` URLs, built once per
content hash and cached in app state. This works identically in the browser,
the desktop WebView and Android. A `blob:` URL would use less memory on the
web; P7 can switch the resolver without touching the Markdown or the renderer.

## Consequences

- A photo is uploaded once no matter how many notes use it, and an export
  cannot contain two attachments with the same name.
- Ingest costs CPU on the device: a large image can take a second or two in a
  release build and noticeably longer in an unoptimised development build.
- Lossy WebP is not suitable for lossless screenshots or transparency-heavy
  art, and animated images lose their motion. Those are acceptable for P6.
- The local database holds every attachment the note links, including ones
  that are never opened. Background eviction and a usage cap are P7 work.
- Placeholder rows make lazy download possible. `notes-sync` now has
  `sync_note_attachments`, which uploads local bytes and downloads verified
  bytes for a note's placeholders; a two-client test moves a Markdown note and
  its photo between stores. The app still has no account screen or sync loop,
  so the human cross-device check waits on P7.
- `uploaded_at` plus `attachment_usage` give Settings and the sync engine the
  queue and size data they need without scanning blobs.
