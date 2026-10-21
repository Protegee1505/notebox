//! Server handlers, independent of the Worker runtime.
//!
//! The handlers validate input, check the device token and call [`Storage`].
//! They never merge CRDT updates and never hash a password, so a request
//! stays within the Workers free tier's CPU budget.

use std::time::Duration;

use notes_proto::{
    is_sha256_hex, is_token_hex, AuthRequest, AuthResponse, BlobChunk, BlobStored,
    CredentialUpdateRequest, CredentialUpdateResponse, DeviceInfo, DeviceList, PulledUpdate,
    SyncRequest, SyncResponse, MAX_BLOB_CHUNKS, MAX_BLOB_CHUNK_BYTES, MAX_DEVICE_NAME_CHARS,
    MAX_PUSH_UPDATES, MAX_UPDATE_BYTES, MAX_USERNAME_CHARS,
};

use crate::auth::{
    backoff_seconds, constant_time_eq, new_token, sha256_hex, LOGIN_RATE_LIMIT_AFTER,
};
use crate::storage::{AccountRow, BlobMetaRow, DeviceRow, NewUpdate, Storage, UpdateRow};
use crate::ServerError;

/// Number of updates returned in one sync page.
///
/// The plan caps a pull at 100 rows; one extra row is read to learn whether
/// another page is waiting without a second query.
pub const SYNC_PAGE: usize = 100;

/// How often an active device's `last_seen` is written.
///
/// Writing on every request would triple the daily row writes for no benefit.
pub const DEVICE_TOUCH_INTERVAL_SECS: i64 = 300;

/// Lower bound on the time a request is allowed to take, used by the budget
/// simulator. The real value is measured on the deployed Worker in S3.
pub const BUDGET_REQUEST_MS: u64 = 10;

/// Deployment configuration read from Worker secrets and vars.
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    setup_token: Option<String>,
}

impl ServerConfig {
    /// A server with no setup token; `/v1/auth/setup` answers `503`.
    pub fn without_setup() -> Self {
        Self::default()
    }

    /// A server whose setup endpoint accepts `token`.
    pub fn with_setup_token(token: impl Into<String>) -> Self {
        Self {
            setup_token: Some(token.into()),
        }
    }

    /// Whether the setup endpoint is enabled.
    pub fn has_setup_token(&self) -> bool {
        self.setup_token.is_some()
    }
}

/// Handlers over one [`Storage`] backend.
#[derive(Debug)]
pub struct Server<S> {
    storage: S,
    config: ServerConfig,
}

impl<S: Storage> Server<S> {
    /// Build a server over `storage`.
    pub fn new(storage: S, config: ServerConfig) -> Self {
        Self { storage, config }
    }

    /// The storage backend, for tests that need to inspect it.
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// One-time account creation. Requires the deployment's setup token.
    pub async fn setup(
        &self,
        provided_token: Option<&str>,
        request: &AuthRequest,
        now: i64,
    ) -> Result<AuthResponse, ServerError> {
        if self.storage.account().await?.is_some() {
            return Err(ServerError::SetupClosed);
        }
        let expected = self
            .config
            .setup_token
            .as_ref()
            .ok_or(ServerError::NotConfigured)?;
        let provided = provided_token.ok_or(ServerError::BadSetupToken)?;
        if !constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
            return Err(ServerError::BadSetupToken);
        }

        let (username, login_key, device_name) = validate_auth(request)?;
        let account = AccountRow {
            username,
            auth_hash: sha256_hex(login_key.as_bytes()),
            created_at: now,
            failed_logins: 0,
            blocked_until: 0,
        };
        if !self.storage.create_account(&account).await? {
            return Err(ServerError::SetupClosed);
        }

        self.issue_device(device_name, now).await
    }

    /// Exchange the account login key for a new device token.
    pub async fn login(
        &self,
        request: &AuthRequest,
        now: i64,
    ) -> Result<AuthResponse, ServerError> {
        let account = self
            .storage
            .account()
            .await?
            .ok_or(ServerError::InvalidCredentials)?;

        if now < account.blocked_until {
            return Err(ServerError::RateLimited {
                retry_after: account.blocked_until - now,
            });
        }

        let (username, login_key, device_name) = validate_auth(request)?;
        // Only a wrong key for the real username counts toward the backoff.
        // A random username must not let a stranger lock the owner out of a
        // public deployment, and rejecting it costs no D1 write.
        if !constant_time_eq(username.as_bytes(), account.username.as_bytes()) {
            return Err(ServerError::InvalidCredentials);
        }
        let key_matches = constant_time_eq(
            sha256_hex(login_key.as_bytes()).as_bytes(),
            account.auth_hash.as_bytes(),
        );
        if !key_matches {
            let failures = account.failed_logins.saturating_add(1);
            let delay = backoff_seconds(failures);
            self.storage
                .set_login_state(failures, now.saturating_add(delay))
                .await?;
            return Err(if failures >= LOGIN_RATE_LIMIT_AFTER {
                ServerError::RateLimited { retry_after: delay }
            } else {
                ServerError::InvalidCredentials
            });
        }

        self.storage.set_login_state(0, 0).await?;
        self.issue_device(device_name, now).await
    }

    /// Change the account's username and/or login key.
    ///
    /// The request is authenticated by the device token and authorized by the
    /// current password's derived key. The current device keeps working; every
    /// other device is revoked so old tokens cannot outlive a credential
    /// change. The server only hashes the new key with SHA-256; Argon2id stays
    /// on the client.
    pub async fn update_credentials(
        &self,
        device: &DeviceRow,
        request: &CredentialUpdateRequest,
    ) -> Result<CredentialUpdateResponse, ServerError> {
        validate_login_key(&request.current_login_key)?;
        validate_login_key(&request.new_login_key)?;
        let new_username = validate_username(&request.new_username)?;
        let account = self
            .storage
            .account()
            .await?
            .ok_or(ServerError::InvalidCredentials)?;

        let current_hash = sha256_hex(request.current_login_key.as_bytes());
        if !constant_time_eq(current_hash.as_bytes(), account.auth_hash.as_bytes()) {
            return Err(ServerError::WrongPassword);
        }

        let new_auth_hash = sha256_hex(request.new_login_key.as_bytes());
        self.storage
            .update_credentials(&new_username, &new_auth_hash, &device.id)
            .await?;
        Ok(CredentialUpdateResponse {
            username: new_username,
        })
    }

    /// Check a bearer token and return the device that owns it.
    pub async fn authenticate(&self, token: &str, now: i64) -> Result<DeviceRow, ServerError> {
        if !is_token_hex(token) {
            return Err(ServerError::Unauthorized);
        }
        let token_hash = sha256_hex(token.as_bytes());
        let mut device = self
            .storage
            .device_by_token_hash(&token_hash)
            .await?
            .ok_or(ServerError::Unauthorized)?;
        if now.saturating_sub(device.last_seen) >= DEVICE_TOUCH_INTERVAL_SECS
            || now < device.last_seen
        {
            self.storage.touch_device(&device.id, now).await?;
            device.last_seen = now;
        }
        Ok(device)
    }

    /// Push a batch, then pull the next page of other devices' updates.
    pub async fn sync(
        &self,
        device: &DeviceRow,
        request: &SyncRequest,
        now: i64,
    ) -> Result<SyncResponse, ServerError> {
        if request.cursor < 0 {
            return Err(ServerError::Invalid("cursor must not be negative".into()));
        }
        if request.updates.len() > MAX_PUSH_UPDATES {
            return Err(ServerError::TooLarge(format!(
                "a push carries at most {MAX_PUSH_UPDATES} updates"
            )));
        }
        let mut updates = Vec::with_capacity(request.updates.len());
        for update in &request.updates {
            if update.blob.is_empty() {
                return Err(ServerError::Invalid("an update must not be empty".into()));
            }
            if update.blob.len() > MAX_UPDATE_BYTES {
                return Err(ServerError::TooLarge(format!(
                    "an update is {} bytes; the limit is {MAX_UPDATE_BYTES}",
                    update.blob.len()
                )));
            }
            updates.push(NewUpdate {
                note_id: update.note_id,
                blob: update.blob.as_bytes().to_vec(),
            });
        }
        if !updates.is_empty() {
            self.storage
                .append_updates(&updates, &device.id, now)
                .await?;
        }

        let mut rows = self
            .storage
            .updates_after(request.cursor, SYNC_PAGE + 1)
            .await?;
        let more = rows.len() > SYNC_PAGE;
        rows.truncate(SYNC_PAGE);
        let cursor = rows.last().map_or(request.cursor, |row| row.seq);
        let updates = rows
            .into_iter()
            .filter(|row| row.device_id != device.id)
            .map(pulled_update)
            .collect();
        Ok(SyncResponse {
            cursor,
            updates,
            more,
        })
    }

    /// Store one attachment chunk. Chunk uploads are idempotent.
    pub async fn put_blob(&self, chunk: &BlobChunk, now: i64) -> Result<BlobStored, ServerError> {
        validate_blob(chunk)?;
        let meta = BlobMetaRow {
            hash: chunk.hash.clone(),
            size: chunk.size,
            mime: chunk.mime.clone(),
            chunks: chunk.total,
            created_at: now,
        };
        if !self.storage.insert_blob_meta(&meta).await? {
            let existing = self
                .storage
                .blob_meta(&chunk.hash)
                .await?
                .ok_or_else(|| ServerError::Storage("blob metadata vanished".into()))?;
            if existing.size != chunk.size
                || existing.chunks != chunk.total
                || existing.mime != chunk.mime
            {
                return Err(ServerError::Conflict(
                    "this hash is already stored with different metadata".into(),
                ));
            }
        }
        self.storage
            .put_blob_chunk(&chunk.hash, chunk.index, chunk.data.as_bytes())
            .await?;
        Ok(BlobStored {
            hash: chunk.hash.clone(),
            index: chunk.index,
        })
    }

    /// Fetch one attachment chunk.
    pub async fn get_blob(&self, hash: &str, index: u32) -> Result<BlobChunk, ServerError> {
        if !is_sha256_hex(hash) {
            return Err(ServerError::Invalid(
                "hash must be lowercase hex SHA-256".into(),
            ));
        }
        let meta = self
            .storage
            .blob_meta(hash)
            .await?
            .ok_or_else(|| ServerError::NotFound("attachment".into()))?;
        if index >= meta.chunks {
            return Err(ServerError::NotFound("attachment chunk".into()));
        }
        let data = self
            .storage
            .blob_chunk(hash, index)
            .await?
            .ok_or_else(|| ServerError::NotFound("attachment chunk".into()))?;
        Ok(BlobChunk {
            hash: meta.hash,
            index,
            total: meta.chunks,
            size: meta.size,
            mime: meta.mime,
            data: data.into(),
        })
    }

    /// List every device that can reach the account.
    pub async fn list_devices(&self, current_id: &str) -> Result<DeviceList, ServerError> {
        let devices = self
            .storage
            .devices()
            .await?
            .into_iter()
            .map(|device| DeviceInfo {
                current: device.id == current_id,
                id: device.id,
                name: device.name,
                created_at: device.created_at,
                last_seen: device.last_seen,
            })
            .collect();
        Ok(DeviceList { devices })
    }

    /// Revoke a device token. Returns whether the device existed.
    pub async fn revoke_device(&self, id: &str) -> Result<bool, ServerError> {
        if id.is_empty() || id.len() > 64 {
            return Err(ServerError::Invalid("device id is not valid".into()));
        }
        Ok(self.storage.delete_device(id).await?)
    }

    /// The Worker's per-request CPU budget as a duration, for scripts.
    pub fn request_budget() -> Duration {
        Duration::from_millis(BUDGET_REQUEST_MS)
    }

    async fn issue_device(
        &self,
        device_name: String,
        now: i64,
    ) -> Result<AuthResponse, ServerError> {
        let token = new_token();
        let device = DeviceRow {
            id: uuid::Uuid::new_v4().to_string(),
            name: device_name,
            token_hash: sha256_hex(token.as_bytes()),
            created_at: now,
            last_seen: now,
        };
        self.storage.create_device(&device).await?;
        Ok(AuthResponse {
            device_id: device.id,
            token,
        })
    }
}

fn pulled_update(row: UpdateRow) -> PulledUpdate {
    PulledUpdate {
        seq: row.seq,
        note_id: row.note_id,
        blob: row.blob.into(),
        device_id: row.device_id,
        created_at: row.created_at,
    }
}

fn validate_auth(
    request: &AuthRequest,
) -> Result<(String, notes_proto::Base64Bytes, String), ServerError> {
    let username = validate_username(&request.username)?;
    validate_login_key(&request.login_key)?;
    let device_name = request.device_name.trim().to_string();
    if device_name.is_empty() || device_name.chars().count() > MAX_DEVICE_NAME_CHARS {
        return Err(ServerError::Invalid(format!(
            "device name must be 1 to {MAX_DEVICE_NAME_CHARS} characters"
        )));
    }
    if device_name.chars().any(char::is_control) {
        return Err(ServerError::Invalid(
            "device name must not contain control characters".into(),
        ));
    }
    Ok((username, request.login_key.clone(), device_name))
}

fn validate_username(value: &str) -> Result<String, ServerError> {
    let username = value.trim().to_string();
    if username.is_empty() || username.chars().count() > MAX_USERNAME_CHARS {
        return Err(ServerError::Invalid(format!(
            "username must be 1 to {MAX_USERNAME_CHARS} characters"
        )));
    }
    if username.chars().any(char::is_control) {
        return Err(ServerError::Invalid(
            "username must not contain control characters".into(),
        ));
    }
    Ok(username)
}

fn validate_login_key(key: &notes_proto::Base64Bytes) -> Result<(), ServerError> {
    if key.len() != notes_proto::LOGIN_KEY_BYTES {
        return Err(ServerError::Invalid(format!(
            "login key must be exactly {} bytes",
            notes_proto::LOGIN_KEY_BYTES
        )));
    }
    Ok(())
}

fn validate_blob(chunk: &BlobChunk) -> Result<(), ServerError> {
    if !is_sha256_hex(&chunk.hash) {
        return Err(ServerError::Invalid(
            "hash must be lowercase hex SHA-256".into(),
        ));
    }
    if chunk.total == 0 || chunk.total > MAX_BLOB_CHUNKS {
        return Err(ServerError::Invalid(format!(
            "total chunks must be 1 to {MAX_BLOB_CHUNKS}"
        )));
    }
    if chunk.index >= chunk.total {
        return Err(ServerError::Invalid(
            "chunk index must be below the total".into(),
        ));
    }
    if chunk.data.is_empty() || chunk.data.len() > MAX_BLOB_CHUNK_BYTES {
        return Err(ServerError::TooLarge(format!(
            "a chunk must be 1 to {MAX_BLOB_CHUNK_BYTES} bytes"
        )));
    }
    if chunk.data.len() as u64 > chunk.size {
        return Err(ServerError::Invalid(
            "chunk data is larger than the whole attachment".into(),
        ));
    }
    let maximum = u64::from(chunk.total) * MAX_BLOB_CHUNK_BYTES as u64;
    if chunk.size == 0 || chunk.size > maximum {
        return Err(ServerError::Invalid(
            "attachment size does not fit the declared chunk count".into(),
        ));
    }
    if chunk.mime.is_empty() || chunk.mime.len() > 128 || chunk.mime.chars().any(char::is_control) {
        return Err(ServerError::Invalid("mime type is not valid".into()));
    }
    Ok(())
}
