# ADR-0021: Delete attachments from a note, the device and the cloud

- **Status:** accepted
- **Date:** 2026-10-22
- **Phase:** P8 attachments
- **Owner:** app + server

## Question

A note can show an image in its preview, but there was no way to browse all of
its images, save one, or remove bytes. Notes must not be destroyed by accident,
and the project must stay inside the Workers and D1 free plan. How should
attachment deletion work without a schema migration or a server-side merge?

## Context

Attachments are content-addressed by SHA-256. The same bytes can appear in
several notes, so deleting a Markdown reference is not the same as deleting the
blob. `DELETE /v1/blob/{hash}` is idempotent and removes the attachment's chunks
and metadata; there is no tombstone and no background collector.

The app already blocks a delete while a note links the hash. A signed-in client
must remove the cloud copy first: if the server request fails, the local note
and bytes stay intact. The reverse order could leave an orphaned blob that no
note can reach.

The gallery lives inside the editor, not in an app-wide library. "Remove from
note" strips Markdown references only; "Delete" removes references, local bytes
and the cloud copy. Older note versions may still mention the hash, so a history
restore can show a missing image.

## Decision

1. Keep the existing schema. Deletion uses the authenticated
   `DELETE /v1/blob/{hash}` route and the existing attachment row.
2. Delete is blocked when another note links the hash, a sync is running, or a
   signed-in device is offline/needs login. A signed-out attachment that was
   previously uploaded is also blocked, because this device can no longer
   remove its cloud copy and deleting the local row would orphan it.
   "Remove from note" stays available only while another note still links the
   file. Disabling it for the last reference avoids bytes that no note can
   reach and no screen can delete, because there is no app-wide file library.
3. A signed-in delete clears the local upload confirmation before asking the
   server. If the request succeeds but the local removal fails, or the app stops
   in between, the next sync uploads the bytes again from this device. If the
   server request fails, the local note, bytes and references remain.
4. Track the note id when deletion starts. The user may open another note while
   the request is in flight; the reference is removed from the original stored
   note, not whichever note is open when the answer arrives.
5. There is no tombstone. Restoring an old backup that still links the hash can
   upload the bytes again, and old history versions can show a missing image.
   The confirmation text says this before the irreversible step.
6. `GET /v1/usage` returns separate attachment and note/history byte counts
   against the 500 MB per-database free limit. Settings refreshes it when
   opened or when Refresh is pressed; no timer polls it. The note figure is
   approximate because `update_log` stores base64-encoded Loro updates.

## Evidence

- `crates/notes-store/tests/attachments.rs` covers the note-count guard, local
  deletion and the pending-upload state after a cloud delete starts.
- `crates/notes-core/src/attachments.rs` covers removing image references and
  preserving link labels.
- `crates/notes-server/tests/server.rs` covers authenticated usage, idempotent
  blob deletion and the note-count boundary.
- `crates/notes-sync/tests/engine.rs` covers usage and deletion transport calls.

## Consequences

Deleting an attachment is a deliberate, irreversible cloud operation, not a
trash operation. A lost cloud copy can be repaired by a sync from a device that
still has the bytes because the row is marked pending first. A device that
never had the bytes, or a restore from a backup after every local copy is gone,
cannot recover them. Free-plan storage is reported per database, so the two
meters are not added together.
