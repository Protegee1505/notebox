//! Note model, CRDT documents, history, search and Markdown rendering.
//!
//! UI-agnostic by design: the Dioxus app calls this crate through traits and
//! plain data types only.
//!
//! Phase 0 currently proves the riskiest part: one Loro document per note, a
//! text box value applied as a prefix/suffix replacement, and time travel over
//! the oplog.

#![forbid(unsafe_code)]

mod document;
mod error;
mod text;

pub use document::{NoteDocument, HISTORY_MERGE_INTERVAL_SECS, TEXT_CONTAINER};
pub use error::NoteError;
pub use text::{apply_replacement, diff_replacement, TextReplacement};

// Re-export the Loro types that cross crate boundaries, so the store and sync
// engine do not need to pin a second copy of the CRDT crate.
pub use loro::{ExportMode, Frontiers, ImportStatus, VersionVector};
