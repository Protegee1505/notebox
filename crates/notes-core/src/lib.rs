//! Note model, CRDT documents, history, search and Markdown rendering.
//!
//! UI-agnostic by design: the Dioxus app calls this crate through traits and
//! plain data types only.
//!
//! Phase 1 adds the full note model on top of the Phase 0 document spike: the
//! `meta` map, titles and previews, the version timeline with diffs and
//! restore, Markdown rendering, the notebook workspace document, and the
//! portable zip export.

#![forbid(unsafe_code)]

mod document;
mod error;
mod export;
mod history;
mod markdown;
mod note;
mod text;
mod workspace;

pub use document::{NoteDocument, HISTORY_MERGE_INTERVAL_SECS, TEXT_CONTAINER};
pub use error::{ExportError, NoteError};
pub use export::{
    note_file_name, read_export_zip, write_export_zip, ExportAttachment, ExportBundle, ExportNote,
    ATTACHMENTS_DIR, EXPORT_FORMAT, EXPORT_VERSION, MANIFEST_PATH, NOTES_DIR,
};
pub use history::NoteVersion;
pub use markdown::render_markdown;
pub use note::{
    normalize_tags, Note, NoteMeta, META_CONTAINER, PREVIEW_MAX_CHARS, TITLE_MAX_CHARS,
};
pub use text::{apply_replacement, diff_replacement, TextReplacement};
pub use workspace::{WorkspaceDocument, NOTEBOOKS_CONTAINER};

// Re-export the Loro types that cross crate boundaries, so the store and sync
// engine do not need to pin a second copy of the CRDT crate.
pub use loro::{ExportMode, Frontiers, ImportStatus, VersionVector};
