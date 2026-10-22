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

mod attachments;
mod document;
mod error;
mod export;
mod history;
mod markdown;
mod markup;
mod note;
mod text;
mod version;
mod workspace;

pub use attachments::{
    attachment_url, content_hash, extension_for_mime, mime_for_extension, normalize_file_name,
    parse_attachment_url, references, remove_attachment_references, AttachmentReference,
    AttachmentUrl, ATTACHMENTS_DIR,
};
pub use document::{NoteDocument, HISTORY_MERGE_INTERVAL_SECS, TEXT_CONTAINER};
pub use error::{ExportError, NoteError};
pub use export::{
    history_file_name, note_file_name, read_export_zip, write_export_zip, ExportAttachment,
    ExportBundle, ExportNote, EXPORT_FORMAT, EXPORT_VERSION, HISTORY_DIR, MANIFEST_PATH, NOTES_DIR,
};
pub use history::NoteVersion;
pub use markdown::{render_markdown, render_markdown_with_images};
pub use markup::{external_url, markup_spans, MarkupKind, MarkupSpan, MAX_MARKUP_SPANS};
pub use note::{
    normalize_tags, plain_line, Note, NoteMeta, META_CONTAINER, PREVIEW_MAX_CHARS, TITLE_MAX_CHARS,
};
pub use text::{apply_replacement, diff_replacement, TextReplacement};
pub use version::{decode_version_vector, encode_version_vector};
pub use workspace::{WorkspaceDocument, NOTEBOOKS_CONTAINER};

// Re-export the Loro types that cross crate boundaries, so the store and sync
// engine do not need to pin a second copy of the CRDT crate.
pub use loro::{ExportMode, Frontiers, ImportStatus, VersionVector};
