//! One note as a Loro document.
//!
//! The body lives in a `text` container and keeps its full oplog. The document
//! itself is UI-agnostic: the app, the store and the sync engine exchange
//! snapshots, updates and version vectors, never UI types.

use loro::{ExportMode, Frontiers, ImportStatus, LoroDoc, LoroText, VersionVector};

use crate::{apply_replacement, diff_replacement, NoteError, TextReplacement};

/// Name of the root container that holds the Markdown body.
pub const TEXT_CONTAINER: &str = "text";

/// Concurrent commits from one device within this window collapse into one
/// history version, matching the five-minute grouping in `PLAN.md`.
pub const HISTORY_MERGE_INTERVAL_SECS: i64 = 5 * 60;

/// A single note document.
///
/// Cloning this type is not supported on purpose: use [`NoteDocument::fork`]
/// when a test or a conflict path needs an independent replica, because a plain
/// clone may reuse the same peer id.
#[derive(Debug)]
pub struct NoteDocument {
    doc: LoroDoc,
}

impl Default for NoteDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl NoteDocument {
    /// Create an empty document with change timestamps enabled.
    pub fn new() -> Self {
        let doc = LoroDoc::new();
        configure_history(&doc);
        Self { doc }
    }

    /// Restore a document from a full snapshot.
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, NoteError> {
        let doc = LoroDoc::from_snapshot(bytes)?;
        // Timestamp recording is runtime configuration, not part of the
        // snapshot, so it has to be enabled again on every load.
        configure_history(&doc);
        Ok(Self { doc })
    }

    /// The underlying Loro document, for the store and sync engine.
    pub fn as_loro(&self) -> &LoroDoc {
        &self.doc
    }

    /// Handle to the body container.
    pub fn text_container(&self) -> LoroText {
        self.doc.get_text(TEXT_CONTAINER)
    }

    /// Current Markdown body.
    pub fn text(&self) -> String {
        self.text_container().to_string()
    }

    /// Length of the body in Unicode scalars.
    pub fn char_len(&self) -> usize {
        self.text_container().len_unicode()
    }

    /// Whether the body is empty.
    pub fn is_empty(&self) -> bool {
        self.char_len() == 0
    }

    /// Apply a text box value, changing only the middle that differs.
    pub fn apply_edit<'a>(&self, new_text: &'a str) -> Result<TextReplacement<'a>, NoteError> {
        let edit = diff_replacement(&self.text(), new_text);
        apply_replacement(&self.text_container(), &edit)?;
        Ok(edit)
    }

    /// Insert at a Unicode scalar position.
    pub fn insert(&self, char_pos: usize, text: &str) -> Result<(), NoteError> {
        self.check_position(char_pos)?;
        self.text_container().insert(char_pos, text)?;
        Ok(())
    }

    /// Delete a Unicode scalar range.
    pub fn delete(&self, char_pos: usize, char_len: usize) -> Result<(), NoteError> {
        self.check_position(char_pos)?;
        let current = self.char_len();
        if char_pos + char_len > current {
            return Err(NoteError::OutOfBounds {
                position: char_pos + char_len,
                len: current,
            });
        }
        self.text_container().delete(char_pos, char_len)?;
        Ok(())
    }

    /// Group the pending operations into one change.
    pub fn commit(&self) {
        self.doc.commit();
    }

    /// Timestamp the next commit explicitly, in Unix seconds.
    ///
    /// The app lets Loro use the wall clock; tests and imports set this so the
    /// five-minute version grouping is reproducible.
    pub fn set_next_commit_timestamp(&self, timestamp: i64) {
        self.doc.set_next_commit_timestamp(timestamp);
    }

    /// Commit the pending operations with an explicit Unix-seconds timestamp.
    pub fn commit_at(&self, timestamp: i64) {
        self.doc.set_next_commit_timestamp(timestamp);
        self.doc.commit();
    }

    /// Full document snapshot, including history.
    pub fn snapshot(&self) -> Result<Vec<u8>, NoteError> {
        Ok(self.doc.export(ExportMode::Snapshot)?)
    }

    /// Updates this document has that the peer at `since` does not.
    pub fn updates_since(&self, since: &VersionVector) -> Result<Vec<u8>, NoteError> {
        Ok(self.doc.export(ExportMode::updates(since))?)
    }

    /// All updates in the oplog.
    pub fn all_updates(&self) -> Result<Vec<u8>, NoteError> {
        Ok(self.doc.export(ExportMode::all_updates())?)
    }

    /// Import a snapshot or updates. Importing the same data twice is harmless.
    pub fn import(&self, bytes: &[u8]) -> Result<ImportStatus, NoteError> {
        Ok(self.doc.import(bytes)?)
    }

    /// Oplog version, used as the sync cursor and for update export.
    pub fn version(&self) -> VersionVector {
        self.doc.oplog_vv()
    }

    /// Current oplog frontiers, used for time travel.
    pub fn frontiers(&self) -> Frontiers {
        self.doc.oplog_frontiers()
    }

    /// Show the document as it was at `frontiers`.
    ///
    /// While checked out the document is detached: imports still reach the
    /// oplog but are not applied until [`NoteDocument::checkout_latest`].
    pub fn checkout(&self, frontiers: &Frontiers) -> Result<(), NoteError> {
        self.doc.checkout(frontiers)?;
        Ok(())
    }

    /// Return to the latest state after a [`NoteDocument::checkout`].
    pub fn checkout_latest(&self) {
        self.doc.checkout_to_latest();
    }

    /// Whether the document is showing a historical state.
    pub fn is_detached(&self) -> bool {
        self.doc.is_detached()
    }

    /// Independent replica with a fresh peer id.
    pub fn fork(&self) -> Self {
        let doc = self.doc.fork();
        configure_history(&doc);
        Self { doc }
    }

    /// Replica containing only history up to `frontiers`.
    pub fn fork_at(&self, frontiers: &Frontiers) -> Result<Self, NoteError> {
        let doc = self.doc.fork_at(frontiers)?;
        configure_history(&doc);
        Ok(Self { doc })
    }

    /// Override the peer id. Tests use this for deterministic replicas; apps
    /// must let Loro pick a fresh random peer per session.
    pub fn set_peer_id(&self, peer_id: u64) -> Result<(), NoteError> {
        self.doc.set_peer_id(peer_id)?;
        Ok(())
    }

    /// This replica's peer id.
    pub fn peer_id(&self) -> u64 {
        self.doc.peer_id()
    }

    fn check_position(&self, position: usize) -> Result<(), NoteError> {
        let len = self.char_len();
        if position > len {
            return Err(NoteError::OutOfBounds { position, len });
        }
        Ok(())
    }
}

/// Runtime history configuration. Loro stores neither setting in snapshots,
/// so every loaded or forked document runs this again.
fn configure_history(doc: &LoroDoc) {
    doc.set_record_timestamp(true);
    doc.set_change_merge_interval(HISTORY_MERGE_INTERVAL_SECS);
}
