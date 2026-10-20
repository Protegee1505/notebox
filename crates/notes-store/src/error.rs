//! Errors returned by a note store.

use notes_core::{ExportError, NoteError};
use notes_proto::{NoteId, NotebookId};
use thiserror::Error;

/// Failure from a local storage operation.
#[derive(Debug, Error)]
pub enum StoreError {
    /// SQLite rejected a statement or could not open the database.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// The note document layer rejected an operation.
    #[error(transparent)]
    Note(#[from] NoteError),

    /// The export archive could not be read or written.
    #[error(transparent)]
    Export(#[from] ExportError),

    /// The note does not exist.
    #[error("note {0} was not found")]
    NoteNotFound(NoteId),

    /// The notebook does not exist.
    #[error("notebook {0} was not found")]
    NotebookNotFound(NotebookId),

    /// A stored row does not hold the values this build writes.
    #[error("stored row is not valid: {0}")]
    Corrupt(String),

    /// The caller asked for something that cannot be done.
    #[error("{0}")]
    Invalid(String),

    /// Setting up or opening the browser database failed.
    #[cfg(target_arch = "wasm32")]
    #[error("browser database failed: {0}")]
    Browser(String),
}
