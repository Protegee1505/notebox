//! Sync engine: device login, push and pull, offline queue, retries and
//! attachment transfer.
//!
//! The engine is UI-agnostic and platform-agnostic. It talks to a server
//! through [`SyncTransport`], reads and writes notes through
//! [`notes_store::NoteStore`], and derives the login key with Argon2id on the
//! device. The Worker never sees the passphrase and never merges a note.
//!
//! ```no_run
//! use notes_sync::{HttpTransport, MemoryTokenStore, SyncClient, SyncConfig};
//!
//! let transport = HttpTransport::new("https://notebox.example.workers.dev")?;
//! let client = SyncClient::new(transport, SyncConfig::new("alice", "laptop"));
//! let sessions = MemoryTokenStore::new();
//! // First run on a fresh server:
//! // client.setup(setup_token, passphrase, &sessions).await?;
//! # Ok::<(), notes_sync::SyncError>(())
//! ```
//!
//! On a device that already has a token, load it and pass it in:
//!
//! ```no_run
//! # use notes_sync::{HttpTransport, MemoryTokenStore, SyncClient, SyncConfig, TokenStore};
//! # let transport = HttpTransport::new("http://127.0.0.1:8787").expect("url");
//! # let sessions = MemoryTokenStore::new();
//! let session = sessions.load()?;
//! let client = SyncClient::with_session(transport, SyncConfig::new("alice", "laptop"), session);
//! # Ok::<(), notes_sync::SyncError>(())
//! ```

#![forbid(unsafe_code)]

mod blob;
mod engine;
mod error;
mod key;
mod retry;
mod session;
mod transport;
mod util;

#[cfg(not(target_arch = "wasm32"))]
mod http;

pub use blob::{
    chunk_count, sha256_hex, AttachmentData, AttachmentRef, CHUNK_BYTES, MAX_ATTACHMENT_BYTES,
};
pub use engine::{
    workspace_note_id, SyncClient, SyncConfig, SyncOutcome, EDIT_DEBOUNCE_SECS, MAX_PAGES,
    SYNC_INTERVAL_SECS,
};
pub use error::SyncError;
pub use key::{
    derive_login_key, login_salt, normalize_username, ARGON2_ITERATIONS, ARGON2_MEMORY_KIB,
    ARGON2_PARALLELISM, SALT_BYTES, SALT_LABEL,
};
pub use retry::{
    backoff_seconds, NoSleep, Sleeper, MAX_ATTEMPTS, RETRY_BASE_SECS, RETRY_MAX_SECS,
    SERVER_WAIT_MAX_SECS,
};
pub use session::{MemoryTokenStore, NullTokenStore, SyncSession, TokenStore};
pub use transport::SyncTransport;

#[cfg(not(target_arch = "wasm32"))]
pub use http::{HttpTransport, MAX_RESPONSE_BYTES, REQUEST_TIMEOUT_SECS};

#[cfg(not(target_arch = "wasm32"))]
pub use retry::ThreadSleeper;

#[cfg(not(target_arch = "wasm32"))]
pub use session::FileTokenStore;
