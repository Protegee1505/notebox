//! Failures the server returns to a client.

use thiserror::Error;

/// A request the server refuses, with the status and code it maps to.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ServerError {
    /// No account exists and the deployment has no `SETUP_TOKEN` secret.
    #[error("setup is not configured on this server")]
    NotConfigured,

    /// The account already exists, or was created by a concurrent request.
    #[error("setup has already been completed")]
    SetupClosed,

    /// The setup token did not match.
    #[error("the setup token is not valid")]
    BadSetupToken,

    /// Username or login key did not match.
    #[error("invalid credentials")]
    InvalidCredentials,

    /// The current password did not match on an authenticated device.
    ///
    /// This is deliberately separate from [`Self::Unauthorized`]: a wrong
    /// password must not make the client discard a valid device session.
    #[error("current password is incorrect")]
    WrongPassword,

    /// Too many failed logins; the client must wait.
    #[error("too many failed logins; retry in {retry_after} seconds")]
    RateLimited {
        /// Seconds until another attempt is accepted.
        retry_after: i64,
    },

    /// The request carried no usable device token.
    #[error("authentication required")]
    Unauthorized,

    /// A field was missing, malformed or outside its limit.
    #[error("invalid request: {0}")]
    Invalid(String),

    /// The payload exceeds a size or count limit.
    #[error("request too large: {0}")]
    TooLarge(String),

    /// The requested row does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// The row already exists with different content.
    #[error("conflict: {0}")]
    Conflict(String),

    /// The storage backend failed.
    #[error("storage failed: {0}")]
    Storage(String),
}

impl ServerError {
    /// HTTP status that carries this error.
    pub fn status(&self) -> u16 {
        match self {
            Self::NotConfigured => 503,
            Self::SetupClosed => 409,
            Self::BadSetupToken => 403,
            Self::InvalidCredentials | Self::Unauthorized => 401,
            Self::WrongPassword => 403,
            Self::RateLimited { .. } => 429,
            Self::Invalid(_) => 400,
            Self::TooLarge(_) => 413,
            Self::NotFound(_) => 404,
            Self::Conflict(_) => 409,
            Self::Storage(_) => 500,
        }
    }

    /// Stable machine-readable code for the JSON error body.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::SetupClosed => "setup_closed",
            Self::BadSetupToken => "bad_setup_token",
            Self::InvalidCredentials => "invalid_credentials",
            Self::WrongPassword => "wrong_password",
            Self::RateLimited { .. } => "rate_limited",
            Self::Unauthorized => "unauthorized",
            Self::Invalid(_) => "invalid_request",
            Self::TooLarge(_) => "too_large",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Storage(_) => "storage_failed",
        }
    }

    /// Seconds to wait, when this error rate-limits the caller.
    pub fn retry_after(&self) -> Option<i64> {
        match self {
            Self::RateLimited { retry_after } => Some(*retry_after),
            _ => None,
        }
    }
}

/// Failure from a [`crate::Storage`] implementation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// The backend rejected an operation or is unreachable.
    #[error("{0}")]
    Failed(String),
}

impl From<StorageError> for ServerError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
impl From<worker::Error> for StorageError {
    fn from(error: worker::Error) -> Self {
        Self::Failed(error.to_string())
    }
}
