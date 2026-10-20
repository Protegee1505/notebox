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
}
