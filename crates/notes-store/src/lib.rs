//! Backend-agnostic local storage.
//!
//! Native builds use SQLite through `rusqlite`; browser builds use the same
//! `rusqlite` API over `sqlite-wasm-rs`, with the database file in OPFS. Both
//! implement [`NoteStore`], so the app and the sync engine never see which
//! backend is in use.
//!
//! Nothing is deleted unless the user empties the trash: a delete sets
//! `trashed_at` in the note's own CRDT, and even then the local snapshot keeps
//! the note's full history until the trash is emptied.

#![forbid(unsafe_code)]

mod error;
mod sqlite;

pub use error::StoreError;
pub use sqlite::{SqliteStore, APP_DB_NAME, BROWSER_DB_DIR, IMAGE_MAGIC, SCHEMA_VERSION};

#[cfg(target_arch = "wasm32")]
pub use sqlite::{install_browser_vfs, open_browser};

use std::collections::HashSet;

use notes_core::{ExportBundle, ExportNote, Note};
use notes_proto::{NoteId, Notebook, NotebookId};

/// Fields needed to create a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNote {
    /// Markdown body. The first line becomes the title.
    pub text: String,
    /// Unix seconds when the note was created.
    pub created_at: i64,
    /// Notebook the note starts in, if any.
    pub notebook_id: Option<NotebookId>,
    /// Tags to start with.
    pub tags: Vec<String>,
}

impl NewNote {
    /// An untitled, untagged note created now.
    pub fn new(text: impl Into<String>, created_at: i64) -> Self {
        Self {
            text: text.into(),
            created_at,
            notebook_id: None,
            tags: Vec::new(),
        }
    }
}

/// Which notes a list or search covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteView {
    /// Not trashed and not archived. The default.
    #[default]
    Active,
    /// Not trashed and archived.
    Archived,
    /// Trashed notes, newest trash time last.
    Trash,
    /// Everything, trashed and archived included.
    All,
}

/// Filter for [`NoteStore::list_notes`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoteQuery {
    /// Which slice of the notes to list.
    pub view: NoteView,
    /// Only notes in this notebook.
    pub notebook_id: Option<NotebookId>,
    /// Only notes carrying this tag.
    pub tag: Option<String>,
    /// Maximum rows; `0` means no limit. Defaults to 200.
    pub limit: usize,
    /// Rows to skip.
    pub offset: usize,
}

impl NoteQuery {
    /// Default paging for the note list.
    pub fn page(view: NoteView) -> Self {
        Self {
            view,
            limit: 200,
            ..Self::default()
        }
    }
}

/// A note as the list and card grid needs it, without loading the CRDT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSummary {
    /// Stable id.
    pub id: NoteId,
    /// First line of the body.
    pub title: String,
    /// Plain-text preview of the body after the title.
    pub preview: String,
    /// Notebook, if any.
    pub notebook_id: Option<NotebookId>,
    /// Tags in the order the user added them.
    pub tags: Vec<String>,
    /// Pinned notes sort first.
    pub pinned: bool,
    /// Archived notes are out of the main list.
    pub archived: bool,
    /// Card color token.
    pub color: Option<String>,
    /// Unix seconds when the note was created.
    pub created_at: i64,
    /// Unix seconds of the last local save.
    pub updated_at: i64,
    /// Unix seconds when the note was trashed.
    pub trashed_at: Option<i64>,
}

/// Filter for [`NoteStore::search`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// What the user typed. Punctuation is kept, words are searched by prefix.
    pub text: String,
    /// Which slice of the notes to search.
    pub view: NoteView,
    /// Only notes in this notebook.
    pub notebook_id: Option<NotebookId>,
    /// Only notes carrying this tag.
    pub tag: Option<String>,
    /// Maximum hits; `0` means the default of 50.
    pub limit: usize,
}

impl SearchQuery {
    /// A plain search across active notes.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The matching note.
    pub note: NoteSummary,
    /// Body excerpt with the matched words wrapped in `[` `]`.
    pub snippet: String,
    /// FTS5 relevance score, lower is better.
    pub rank: f64,
}

/// A tag and how many notes carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    /// Tag text as stored.
    pub tag: String,
    /// Number of non-trashed notes with this tag.
    pub count: u64,
}

/// The one-row sync cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncState {
    /// Server sequence number of the last pulled update, or `None` before the
    /// first pull. The server orders updates with a single increasing integer,
    /// so the cursor is one integer, not a version vector.
    pub cursor: Option<i64>,
    /// Unix seconds of the last successful sync.
    pub updated_at: i64,
}

/// Changes to the notebook-list document waiting for the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePush {
    /// Encoded Loro updates the server has not seen.
    pub updates: Vec<u8>,
    /// The version those updates bring the server to, encoded with
    /// [`notes_core::encode_version_vector`]. Hand this back to
    /// [`NoteStore::mark_workspace_pushed`] after the server accepts.
    pub version: Vec<u8>,
}

/// What an import changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Notes that did not exist before.
    pub notes_created: usize,
    /// Notes that existed and were updated from the archive.
    pub notes_updated: usize,
    /// Notebooks created or refreshed.
    pub notebooks: usize,
    /// Attachments recorded.
    pub attachments: usize,
}

/// One attachment as the gallery and the sync queue need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// Lowercase SHA-256 of the bytes. This is the identity.
    pub hash: String,
    /// Display name, with an extension that matches the bytes.
    pub name: String,
    /// MIME type, for example `image/webp`.
    pub mime: String,
    /// Size in bytes, or `0` while the bytes are still missing.
    pub size: u64,
    /// Whether the bytes are in this database. A note can link an attachment
    /// that only exists on another device or on the server.
    pub available: bool,
    /// Whether the server has the bytes.
    pub uploaded: bool,
    /// Unix seconds when this database first recorded the attachment.
    pub created_at: i64,
}

/// How much local space attachments take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachmentUsage {
    /// Attachments with bytes in this database.
    pub count: u64,
    /// Total bytes stored.
    pub bytes: u64,
}

/// Local note storage contract.
///
/// Implementations use interior mutability, so every method takes `&self` and
/// the trait object can live in a UI signal. Operations are synchronous: a
/// browser backend runs them inside its worker, not on the UI thread.
pub trait NoteStore {
    /// Create a note and store it.
    fn create_note(&self, new_note: NewNote) -> Result<Note, StoreError>;

    /// Load a note's CRDT document by id.
    fn open_note(&self, id: &NoteId) -> Result<Note, StoreError>;

    /// Store a note's snapshot and metadata.
    ///
    /// `updated_at` is the local save time; the caller supplies the clock so
    /// tests and imports stay reproducible.
    fn save_note(&self, note: &Note, updated_at: i64) -> Result<NoteSummary, StoreError>;

    /// The stored summary of one note.
    fn note_summary(&self, id: &NoteId) -> Result<NoteSummary, StoreError>;

    /// List notes matching a filter, pinned first, then most recently edited.
    fn list_notes(&self, query: &NoteQuery) -> Result<Vec<NoteSummary>, StoreError>;

    /// Full-text search, best match first.
    fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, StoreError>;

    /// Move a note to the trash. Returns whether the state changed.
    fn trash_note(&self, id: &NoteId, at: i64) -> Result<bool, StoreError>;

    /// Bring a note back from the trash. Returns whether the state changed.
    fn restore_note(&self, id: &NoteId, at: i64) -> Result<bool, StoreError>;

    /// Erase every trashed note, for real. Returns how many were erased.
    fn empty_trash(&self) -> Result<usize, StoreError>;

    /// Number of notes outside the trash.
    fn note_count(&self) -> Result<u64, StoreError>;

    /// Store attachment bytes and return the content-addressed record.
    ///
    /// Storing the same bytes twice returns the first record and keeps one
    /// copy. `name` is only a display name: the extension is chosen from
    /// `mime`, so the URL a note uses matches the bytes.
    fn put_attachment(
        &self,
        name: &str,
        mime: &str,
        bytes: &[u8],
        at: i64,
    ) -> Result<Attachment, StoreError>;

    /// Metadata for one attachment, if this database has a row for it.
    fn attachment(&self, hash: &str) -> Result<Option<Attachment>, StoreError>;

    /// Bytes for an attachment, or `None` when only a link exists.
    fn attachment_bytes(&self, hash: &str) -> Result<Option<Vec<u8>>, StoreError>;

    /// Attachments referenced by one note, in the order they were added.
    fn note_attachments(&self, note: &NoteId) -> Result<Vec<Attachment>, StoreError>;

    /// Attachments whose bytes the server has not seen, oldest first.
    fn unsynced_attachments(&self, limit: usize) -> Result<Vec<String>, StoreError>;

    /// Notes with at least one attachment to upload or download.
    ///
    /// The sync engine works one note at a time, so it needs note ids, not just
    /// hashes. A note stays listed while one of its attachments has no local
    /// bytes or has bytes the server has not accepted yet.
    fn notes_with_attachment_work(&self, limit: usize) -> Result<Vec<NoteId>, StoreError>;

    /// Record that the server has an attachment's bytes.
    fn mark_attachment_uploaded(&self, hash: &str, at: i64) -> Result<(), StoreError>;

    /// Count and total size of attachments stored locally.
    fn attachment_usage(&self) -> Result<AttachmentUsage, StoreError>;

    /// Import a remote update into a note, creating it if needed.
    fn apply_update(&self, id: &NoteId, update: &[u8], at: i64) -> Result<Note, StoreError>;

    /// Notes whose local version the server has not seen yet.
    ///
    /// This is the offline queue: every note whose last save is newer than the
    /// version last acknowledged by the server. `limit` caps the result;
    /// `0` means all. Most recently edited notes come first, so a long queue
    /// pushes what the user just typed before older edits.
    fn unsynced_notes(&self, limit: usize) -> Result<Vec<NoteId>, StoreError>;

    /// Encoded `VersionVector` of the last pushed update for a note.
    fn last_pushed(&self, id: &NoteId) -> Result<Option<Vec<u8>>, StoreError>;

    /// Record the encoded `VersionVector` the server has for a note.
    fn set_last_pushed(&self, id: &NoteId, version: &[u8]) -> Result<(), StoreError>;

    /// Notebook-list changes the server has not seen, if any.
    fn workspace_pending(&self) -> Result<Option<WorkspacePush>, StoreError>;

    /// Record that the server has the notebook-list document up to `version`.
    fn mark_workspace_pushed(&self, version: &[u8]) -> Result<(), StoreError>;

    /// Notebooks, oldest first.
    fn list_notebooks(&self) -> Result<Vec<Notebook>, StoreError>;

    /// Create a notebook with a fresh id.
    fn create_notebook(&self, name: &str, at: i64) -> Result<Notebook, StoreError>;

    /// Rename a notebook. Returns whether the notebook exists.
    fn rename_notebook(&self, id: &NotebookId, name: &str) -> Result<bool, StoreError>;

    /// Remove a notebook from the list. Notes keep their `notebook_id`.
    fn delete_notebook(&self, id: &NotebookId) -> Result<bool, StoreError>;

    /// The workspace document's snapshot, for sync and export.
    fn workspace_snapshot(&self) -> Result<Vec<u8>, StoreError>;

    /// Import remote workspace updates and refresh the notebook list.
    fn import_workspace(&self, bytes: &[u8]) -> Result<Vec<Notebook>, StoreError>;

    /// Tags with counts, most used first.
    fn list_tags(&self) -> Result<Vec<TagCount>, StoreError>;

    /// The pull cursor and last sync time.
    fn sync_state(&self) -> Result<SyncState, StoreError>;

    /// Save the pull cursor and last sync time.
    fn set_sync_state(&self, state: &SyncState) -> Result<(), StoreError>;

    /// Write every note, notebook and attachment as a zip.
    fn export_zip(&self, exported_at: i64) -> Result<Vec<u8>, StoreError>;

    /// Import a zip written by [`NoteStore::export_zip`].
    fn import_zip(&self, bytes: &[u8]) -> Result<ImportReport, StoreError>;
}

/// Build the export bundle for a store, without writing the zip.
///
/// Exposed for tests and for a future "inspect an export" screen.
pub fn export_bundle(store: &dyn NoteStore, exported_at: i64) -> Result<ExportBundle, StoreError> {
    let notes = store.list_notes(&NoteQuery {
        view: NoteView::All,
        limit: 0,
        ..NoteQuery::default()
    })?;
    let mut export_notes = Vec::with_capacity(notes.len());
    let mut attachments = Vec::new();
    let mut seen = HashSet::new();
    for summary in notes {
        let note = store.open_note(&summary.id)?;
        for attachment in store.note_attachments(&summary.id)? {
            if !attachment.available || !seen.insert(attachment.hash.clone()) {
                continue;
            }
            let Some(data) = store.attachment_bytes(&attachment.hash)? else {
                continue;
            };
            attachments.push(notes_core::ExportAttachment {
                note_id: Some(summary.id),
                name: attachment.name,
                mime: attachment.mime,
                data,
            });
        }
        export_notes.push(ExportNote {
            id: summary.id,
            meta: note.meta(),
            text: note.text(),
            snapshot: Some(note.document().snapshot()?),
        });
    }
    Ok(ExportBundle {
        exported_at,
        notebooks: store.list_notebooks()?,
        notes: export_notes,
        attachments,
    })
}
