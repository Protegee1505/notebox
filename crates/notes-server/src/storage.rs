//! Durable server storage behind one trait.
//!
//! Two implementations exist: an in-memory one for tests and the D1 one the
//! Worker uses. The handlers only see this trait, so the same logic can run on
//! another host by adding an implementation.

use async_trait::async_trait;
use notes_proto::NoteId;

use crate::StorageError;

/// The single account, with its login backoff state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    /// Username chosen at setup.
    pub username: String,
    /// Lowercase hex SHA-256 of the client's Argon2id login key.
    pub auth_hash: String,
    /// Unix seconds when the account was created.
    pub created_at: i64,
    /// Consecutive failed login attempts.
    pub failed_logins: u32,
    /// Unix seconds before which login attempts are rejected.
    pub blocked_until: i64,
}

/// One logged-in device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    /// Random device id (UUID text).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Lowercase hex SHA-256 of the bearer token.
    pub token_hash: String,
    /// Unix seconds when the device first logged in.
    pub created_at: i64,
    /// Unix seconds when the server last recorded activity.
    pub last_seen: i64,
}

/// A note update about to be appended to the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewUpdate {
    /// Note the update belongs to.
    pub note_id: NoteId,
    /// Opaque encoded Loro update.
    pub blob: Vec<u8>,
}

/// A note update read back from the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRow {
    /// Commit order in the log.
    pub seq: i64,
    /// Note the update belongs to.
    pub note_id: NoteId,
    /// Opaque encoded Loro update.
    pub blob: Vec<u8>,
    /// Device that wrote it.
    pub device_id: String,
    /// Unix seconds when it was accepted.
    pub created_at: i64,
}

/// Metadata for one content-addressed attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobMetaRow {
    /// Lowercase hex SHA-256 of the whole attachment.
    pub hash: String,
    /// Decoded size of the whole attachment.
    pub size: u64,
    /// MIME type.
    pub mime: String,
    /// Number of chunks that make up the attachment.
    pub chunks: u32,
    /// Unix seconds when the first chunk arrived.
    pub created_at: i64,
}

/// Everything the handlers need from durable storage.
///
/// Methods are deliberately small: one handler call maps to a handful of
/// queries, which keeps each Worker request far below the free-tier limits.
#[async_trait(?Send)]
pub trait Storage {
    /// Read the single account, if setup has run.
    async fn account(&self) -> Result<Option<AccountRow>, StorageError>;

    /// Insert the account. Returns `false` if one already exists.
    async fn create_account(&self, account: &AccountRow) -> Result<bool, StorageError>;

    /// Replace the failed-login counter and the block deadline.
    async fn set_login_state(&self, failures: u32, blocked_until: i64) -> Result<(), StorageError>;

    /// Insert a device token hash.
    async fn create_device(&self, device: &DeviceRow) -> Result<(), StorageError>;

    /// Look a device up by its token hash.
    async fn device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<DeviceRow>, StorageError>;

    /// Record that a device was seen.
    async fn touch_device(&self, id: &str, at: i64) -> Result<(), StorageError>;

    /// Every device, oldest first.
    async fn devices(&self) -> Result<Vec<DeviceRow>, StorageError>;

    /// Remove a device. Returns whether it existed.
    async fn delete_device(&self, id: &str) -> Result<bool, StorageError>;

    /// Append updates in one atomic batch, preserving their order.
    async fn append_updates(
        &self,
        updates: &[NewUpdate],
        device_id: &str,
        at: i64,
    ) -> Result<(), StorageError>;

    /// Read at most `limit` updates with a sequence number above `cursor`,
    /// oldest first. The caller trims its own updates after reading.
    async fn updates_after(
        &self,
        cursor: i64,
        limit: usize,
    ) -> Result<Vec<UpdateRow>, StorageError>;

    /// Attachment metadata, if the first chunk was stored.
    async fn blob_meta(&self, hash: &str) -> Result<Option<BlobMetaRow>, StorageError>;

    /// Insert attachment metadata. Returns `false` if it already existed.
    async fn insert_blob_meta(&self, meta: &BlobMetaRow) -> Result<bool, StorageError>;

    /// Store one chunk. Re-uploading the same chunk is a no-op.
    async fn put_blob_chunk(&self, hash: &str, index: u32, data: &[u8])
        -> Result<(), StorageError>;

    /// Read one chunk, if present.
    async fn blob_chunk(&self, hash: &str, index: u32) -> Result<Option<Vec<u8>>, StorageError>;
}
