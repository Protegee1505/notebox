//! Errors returned by the note document layer.

use thiserror::Error;

/// Failure from a note document operation.
#[derive(Debug, Error)]
pub enum NoteError {
    /// Loro rejected the operation or the payload was not a valid document.
    #[error("document operation failed: {0}")]
    Loro(#[from] loro::LoroError),

    /// Loro could not encode a snapshot or update.
    #[error("document encoding failed: {0}")]
    Encode(#[from] loro::LoroEncodeError),

    /// The requested character position is outside the document.
    #[error("{position} is outside a note of {len} characters")]
    OutOfBounds {
        /// Requested Unicode scalar position.
        position: usize,
        /// Current length in Unicode scalars.
        len: usize,
    },

    /// A metadata field did not hold the JSON shape this build expects.
    #[error("note metadata is not valid JSON: {0}")]
    Metadata(#[from] serde_json::Error),

    /// Loro could not walk the oplog to build the version list.
    #[error("history traversal failed: {0}")]
    History(String),
}

/// Failure while writing or reading a portable export archive.
#[derive(Debug, Error)]
pub enum ExportError {
    /// The archive is not a readable zip.
    #[error("archive is not a readable zip: {0}")]
    Zip(#[from] zip::result::ZipError),

    /// Reading or writing the archive stream failed.
    #[error("archive input/output failed: {0}")]
    Io(#[from] std::io::Error),

    /// The manifest or a note file held invalid JSON.
    #[error("export metadata is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// The archive is a zip but not a Notebox export.
    #[error("not a Notebox export: {0}")]
    Format(String),

    /// The export was written by a newer format this build cannot read.
    #[error("unsupported export version {0}")]
    Version(u16),

    /// An entry name tried to leave the archive's own directory.
    #[error("unsafe path in archive: {0}")]
    UnsafePath(String),

    /// A file listed in the manifest is missing from the archive.
    #[error("missing archive entry: {0}")]
    MissingEntry(String),

    /// The archive holds more or larger entries than an import will accept.
    #[error("archive too large: {0}")]
    TooLarge(String),
}
