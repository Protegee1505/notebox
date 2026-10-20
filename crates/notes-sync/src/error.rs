//! Failures the sync engine reports to its caller.
//!
//! Every variant carries enough information for the UI to decide what to do:
//! [`SyncError::is_retryable`] says whether waiting helps,
//! [`SyncError::is_unauthorized`] says whether the device must log in again,
//! and [`SyncError::retry_after`] carries the server's own wait time.

use notes_proto::ErrorBody;
use notes_store::StoreError;
use thiserror::Error;
/// Failure from a sync, login or transfer.
#[derive(Debug, Error)]
pub enum SyncError {
    /// The request never reached the server, or the answer never arrived.
    #[error("network failure: {0}")]
    Transport(String),

    /// The server answered with something this build cannot read.
    #[error("unexpected server response: {0}")]
    Protocol(String),

    /// The server speaks a protocol version this build does not know.
    #[error("server speaks protocol {found}, this build speaks {expected}")]
    ProtocolVersion {
        /// Version in the response envelope.
        found: u16,
        /// Version this build sends and expects.
        expected: u16,
    },

    /// The server refused the request with an error body.
    #[error("server refused the request: {status} {code}: {message}")]
    Server {
        /// HTTP status code.
        status: u16,
        /// Stable machine-readable code from the error body.
        code: String,
        /// Human-readable explanation.
        message: String,
        /// Seconds the server asked the client to wait.
        retry_after: Option<i64>,
    },

    /// The device token is missing, revoked or expired.
    #[error("the device token is not valid any more; log in again")]
    Unauthorized,

    /// This device has no stored session.
    #[error("this device is not logged in")]
    NoSession,

    /// Reading or writing the stored device session failed.
    #[error("session storage failed: {0}")]
    Session(String),

    /// Argon2id could not stretch the passphrase.
    #[error("login key derivation failed: {0}")]
    KeyDerivation(String),

    /// Local note storage failed.
    #[error(transparent)]
    Store(#[from] StoreError),

    /// A downloaded attachment did not match its content address.
    #[error("attachment {hash} failed its SHA-256 check")]
    HashMismatch {
        /// Content address the bytes were fetched under.
        hash: String,
    },

    /// The caller asked for something that cannot be done.
    #[error("{0}")]
    Invalid(String),
}

/// A CRDT failure while reading or writing a note becomes a store failure, so
/// callers only handle one error type.
impl From<notes_core::NoteError> for SyncError {
    fn from(error: notes_core::NoteError) -> Self {
        Self::Store(StoreError::Note(error))
    }
}

impl SyncError {
    /// Whether waiting and trying again can succeed.
    ///
    /// Transport failures and server-side trouble are worth another attempt;
    /// a rejected request or a revoked token is not.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Server { status, .. } => *status == 408 || *status == 429 || *status >= 500,
            _ => false,
        }
    }

    /// Seconds the server asked the client to wait before another attempt.
    pub fn retry_after(&self) -> Option<i64> {
        match self {
            Self::Server { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Whether the stored token must be dropped and the user logged in again.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Unauthorized)
    }
}

/// Build the error for a non-success status, using the server's error body.
///
/// Only the native HTTP transport reads responses here until the browser
/// transport lands in P7, so a wasm build without tests has no caller yet.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn server_error(status: u16, retry_after: Option<i64>, body: &[u8]) -> SyncError {
    let parsed = serde_json::from_slice::<notes_proto::Envelope<ErrorBody>>(body)
        .ok()
        .map(|envelope| envelope.payload);
    if status == 401 {
        return SyncError::Unauthorized;
    }
    match parsed {
        Some(body) => SyncError::Server {
            status,
            code: body.code,
            message: body.message,
            retry_after: body.retry_after.or(retry_after),
        },
        None => SyncError::Server {
            status,
            code: "unknown".to_string(),
            message: format!("status {status} with no readable error body"),
            retry_after,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{server_error, SyncError};

    fn error_envelope(code: &str, message: &str, retry_after: Option<i64>) -> Vec<u8> {
        let body = notes_proto::ErrorBody {
            code: code.to_string(),
            message: message.to_string(),
            retry_after,
        };
        serde_json::to_vec(&notes_proto::Envelope::new(body)).expect("encode")
    }

    #[test]
    fn only_retryable_failures_says_so() {
        assert!(SyncError::Transport("closed".into()).is_retryable());
        assert!(server_error(500, None, b"").is_retryable());
        assert!(server_error(429, Some(2), b"").is_retryable());
        assert!(!server_error(400, None, b"").is_retryable());
        assert!(!server_error(401, None, b"").is_retryable());
        assert!(!SyncError::Invalid("bad".into()).is_retryable());
    }

    #[test]
    fn a_401_is_reported_as_an_unauthorized_device() {
        let error = server_error(
            401,
            None,
            &error_envelope("unauthorized", "authentication required", None),
        );
        assert!(matches!(error, SyncError::Unauthorized));
        assert!(error.is_unauthorized());
    }

    #[test]
    fn an_error_body_keeps_its_code_and_wait_time() {
        let error = server_error(
            429,
            Some(7),
            &error_envelope("rate_limited", "too many failed logins", Some(4)),
        );
        match error {
            SyncError::Server {
                status,
                code,
                message,
                retry_after,
            } => {
                assert_eq!(status, 429);
                assert_eq!(code, "rate_limited");
                assert_eq!(message, "too many failed logins");
                assert_eq!(retry_after, Some(4), "the body wins over the header");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn an_unreadable_error_body_falls_back_to_the_status() {
        let error = server_error(502, Some(3), b"<html>bad gateway</html>");
        match error {
            SyncError::Server {
                status,
                code,
                retry_after,
                ..
            } => {
                assert_eq!(status, 502);
                assert_eq!(code, "unknown");
                assert_eq!(retry_after, Some(3));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
