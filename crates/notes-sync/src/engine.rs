//! The sync engine: login, push, pull and retry.
//!
//! One run does exactly what PLAN.md's client loop describes: push one batch
//! of at most [`MAX_PUSH_UPDATES`] updates (a larger backlog waits for the next
//! run, so a burst of new notes cannot spend an unbounded number of requests),
//! then pull pages until the server says there is nothing more, importing each
//! update and saving the cursor as it goes. Importing an update twice is
//! harmless, so a response lost in transit costs a repeated push, never text.
//!
//! Ordering guarantees the caller can rely on:
//!
//! - A note's updates are pushed only after the server accepted them, and the
//!   local "server has this" marker is written only after the response for
//!   that request arrived. A crash or a closed socket therefore re-sends
//!   rather than forgets.
//! - The pull cursor is written in the same step as the updates it covers, so
//!   a device never skips an update it did not import.
//! - The notebook list travels as one extra document under
//!   [`workspace_note_id`], which the server stores like any other note.

use std::cell::RefCell;

use notes_core::{decode_version_vector, encode_version_vector, VersionVector};
use notes_proto::{
    AuthRequest, Base64Bytes, CredentialUpdateRequest, CredentialUpdateResponse, DeviceList,
    NoteId, PushUpdate, StorageUsage, SyncRequest, MAX_DEVICE_NAME_CHARS, MAX_PUSH_UPDATES,
    MAX_UPDATE_BYTES, MAX_USERNAME_CHARS,
};
use notes_store::{NoteStore, SyncState};

use crate::blob::{self, AttachmentData, AttachmentRef};
use crate::key::{derive_login_key, normalize_username, MIN_PASSWORD_CHARS};
use crate::retry::{self, Sleeper};
use crate::{SyncError, SyncSession, SyncTransport, TokenStore};
use uuid::Uuid;

/// How often a visible app syncs on its own.
///
/// Ten seconds keeps a second device fresh without exceeding the project's
/// 10% share of the Workers free request budget. Hidden pages never poll, and
/// returning to the foreground asks for an immediate run.
pub const SYNC_INTERVAL_SECS: i64 = 10;

/// Quiet time after the last edit before a push, so typing does not sync on
/// every keystroke.
pub const EDIT_DEBOUNCE_SECS: i64 = 3;

/// Pages one run pulls before it leaves the rest for the next run.
///
/// A page holds 100 rows, so one run covers 3,200 updates before it yields.
pub const MAX_PAGES: u32 = 32;

/// The note id the notebook list travels under.
///
/// It is a fixed, non-random UUID so every device of the account agrees on it
/// without a setting. The bytes spell `notebox-workdocs`, which makes it
/// recognizable in a server log.
pub fn workspace_note_id() -> NoteId {
    NoteId::from_uuid(Uuid::from_u128(u128::from_be_bytes(*b"notebox-workdocs")))
}

/// What one sync run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// Updates the server accepted from this device.
    pub pushed: usize,
    /// Updates this device imported from other devices.
    pub pulled: usize,
    /// Pages pulled.
    pub pages: u32,
    /// Cursor to send next time.
    pub cursor: i64,
    /// Whether the run stopped at [`MAX_PAGES`] with more rows waiting.
    pub more: bool,
}

/// What one note's attachment pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachmentSyncOutcome {
    /// Local attachments the server accepted.
    pub uploaded: usize,
    /// Attachments whose bytes arrived from the server.
    pub downloaded: usize,
    /// Placeholders the server does not have either.
    pub missing: usize,
}

/// What a device needs to know before it can sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConfig {
    /// Account name. Both the client and the server use the trimmed form.
    pub username: String,
    /// Name this device shows in the settings list.
    pub device_name: String,
}

impl SyncConfig {
    /// A configuration with a username and a device name.
    pub fn new(username: impl Into<String>, device_name: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            device_name: device_name.into(),
        }
    }

    /// Check the two fields against the same limits the server enforces.
    pub fn validate(&self) -> Result<(), SyncError> {
        let username = normalize_username(&self.username);
        if username.is_empty() || username.chars().count() > MAX_USERNAME_CHARS {
            return Err(SyncError::Invalid(format!(
                "username must be 1 to {MAX_USERNAME_CHARS} characters"
            )));
        }
        if username.chars().any(char::is_control) {
            return Err(SyncError::Invalid(
                "username must not contain control characters".to_string(),
            ));
        }
        let device_name = self.device_name.trim();
        if device_name.is_empty() || device_name.chars().count() > MAX_DEVICE_NAME_CHARS {
            return Err(SyncError::Invalid(format!(
                "device name must be 1 to {MAX_DEVICE_NAME_CHARS} characters"
            )));
        }
        if device_name.chars().any(char::is_control) {
            return Err(SyncError::Invalid(
                "device name must not contain control characters".to_string(),
            ));
        }
        Ok(())
    }

    /// The auth request for setup and login.
    fn auth_request(&self, login_key: &[u8]) -> AuthRequest {
        AuthRequest {
            username: normalize_username(&self.username),
            login_key: Base64Bytes::new(login_key.to_vec()),
            device_name: self.device_name.trim().to_string(),
        }
    }
}

/// A device's sync client: one transport, one configuration, one session.
pub struct SyncClient<T: SyncTransport> {
    transport: T,
    config: SyncConfig,
    session: RefCell<Option<SyncSession>>,
}

impl<T: SyncTransport> SyncClient<T> {
    /// A client with no session yet.
    pub fn new(transport: T, config: SyncConfig) -> Self {
        Self {
            transport,
            config,
            session: RefCell::new(None),
        }
    }

    /// A client that already has a session, for example one loaded from a
    /// [`TokenStore`].
    pub fn with_session(transport: T, config: SyncConfig, session: Option<SyncSession>) -> Self {
        Self {
            transport,
            config,
            session: RefCell::new(session),
        }
    }

    /// The session in use, if any.
    pub fn session(&self) -> Option<SyncSession> {
        self.session.borrow().clone()
    }

    /// Replace the session without touching a [`TokenStore`].
    pub fn set_session(&self, session: Option<SyncSession>) {
        *self.session.borrow_mut() = session;
    }

    /// This client's configuration.
    pub fn config(&self) -> &SyncConfig {
        &self.config
    }

    /// The transport, for callers that need it directly.
    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Create the single account and log this device in.
    pub async fn setup(
        &self,
        setup_token: &str,
        password: &str,
        sessions: &dyn TokenStore,
    ) -> Result<SyncSession, SyncError> {
        self.config.validate()?;
        let key = derive_login_key(&self.config.username, password)?;
        let request = self.config.auth_request(&key);
        let response = self.transport.setup(setup_token, &request).await?;
        self.keep(sessions, response)
    }

    /// Log this device in against an account that already exists.
    pub async fn login(
        &self,
        password: &str,
        sessions: &dyn TokenStore,
    ) -> Result<SyncSession, SyncError> {
        self.config.validate()?;
        let key = derive_login_key(&self.config.username, password)?;
        let request = self.config.auth_request(&key);
        let response = self.transport.login(&request).await?;
        self.keep(sessions, response)
    }

    /// Change the account's username and/or password.
    ///
    /// The current session authenticates the request; `current_password`
    /// proves the caller knows the existing password. A blank `new_password`
    /// keeps the current one, which is what a username-only change needs.
    ///
    /// The returned username is the trimmed value the server stored. The
    /// caller must replace this client (or its configuration and session)
    /// before another credential operation, because the username is part of
    /// the next current-key derivation.
    pub async fn change_credentials(
        &self,
        current_password: &str,
        new_username: &str,
        new_password: &str,
    ) -> Result<CredentialUpdateResponse, SyncError> {
        self.config.validate()?;
        if current_password.is_empty() {
            return Err(SyncError::Invalid("enter the current password".to_string()));
        }
        let new_username = normalize_username(new_username);
        SyncConfig::new(new_username.clone(), self.config.device_name.clone()).validate()?;
        let new_password = if new_password.is_empty() {
            current_password
        } else {
            new_password
        };
        if new_password.chars().count() < MIN_PASSWORD_CHARS {
            return Err(SyncError::Invalid(format!(
                "password must be at least {MIN_PASSWORD_CHARS} characters"
            )));
        }

        let current_key = derive_login_key(&self.config.username, current_password)?;
        let new_key = derive_login_key(&new_username, new_password)?;
        let session = self.token()?;
        let request = CredentialUpdateRequest {
            current_login_key: Base64Bytes::new(current_key.to_vec()),
            new_username,
            new_login_key: Base64Bytes::new(new_key.to_vec()),
        };
        self.transport
            .change_credentials(&session.token, &request)
            .await
    }

    /// Forget the stored session. The server keeps the device until it is
    /// revoked, so logging in again works without touching the account.
    pub fn logout(&self, sessions: &dyn TokenStore) -> Result<(), SyncError> {
        sessions.clear()?;
        self.set_session(None);
        Ok(())
    }

    /// Notes and notebook-list changes waiting for a push.
    ///
    /// This is the offline queue: it shrinks only when the server accepts.
    pub fn pending(&self, store: &dyn NoteStore) -> Result<usize, SyncError> {
        let notes = store.unsynced_notes(0)?.len();
        let workspace = usize::from(store.workspace_pending()?.is_some());
        Ok(notes + workspace)
    }

    /// One sync attempt: push what the server has not seen, then pull.
    pub async fn sync(&self, store: &dyn NoteStore, now: i64) -> Result<SyncOutcome, SyncError> {
        let session = self.token()?;
        let mut cursor = store.sync_state()?.cursor.unwrap_or(0);
        let mut pending = self.collect_push(store)?;
        let mut outcome = SyncOutcome {
            pushed: 0,
            pulled: 0,
            pages: 0,
            cursor,
            more: false,
        };

        loop {
            let request = SyncRequest {
                cursor,
                updates: std::mem::take(&mut pending.updates),
            };
            let sent = request.updates.len();
            let response = self.transport.sync(&session.token, &request).await?;
            outcome.pushed += sent;

            // The server accepted the push that rode along with this request,
            // so the versions sent are on the server now. Recording them here,
            // before the next page, means a later failure in this run cannot
            // queue the same updates twice.
            for mark in pending.marks.drain(..) {
                match mark.note_id {
                    Some(id) => store.set_last_pushed(&id, &mark.version)?,
                    None => store.mark_workspace_pushed(&mark.version)?,
                }
            }

            for update in &response.updates {
                if update.note_id == workspace_note_id() {
                    store.import_workspace(update.blob.as_bytes())?;
                } else {
                    store.apply_update(&update.note_id, update.blob.as_bytes(), now)?;
                }
            }
            outcome.pulled += response.updates.len();
            outcome.pages += 1;

            cursor = response.cursor;
            outcome.cursor = cursor;
            store.set_sync_state(&SyncState {
                cursor: Some(cursor),
                updated_at: now,
            })?;

            if !response.more {
                break;
            }
            if outcome.pages >= MAX_PAGES {
                outcome.more = true;
                break;
            }
        }

        Ok(outcome)
    }

    /// Sync until it succeeds, waiting between attempts.
    ///
    /// `now` is called before every attempt, so the retried run records the
    /// time it actually happened. Give it a clock, not a constant.
    pub async fn sync_with_retry(
        &self,
        store: &dyn NoteStore,
        now: &dyn Fn() -> i64,
        sleeper: &dyn Sleeper,
    ) -> Result<SyncOutcome, SyncError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.sync(store, now()).await {
                Ok(outcome) => return Ok(outcome),
                Err(error) => {
                    if !error.is_retryable() || attempt >= retry::MAX_ATTEMPTS {
                        return Err(error);
                    }
                    let wait = retry::backoff_seconds(attempt, error.retry_after());
                    sleeper.sleep(wait).await;
                }
            }
        }
    }

    /// The devices that can reach the account.
    pub async fn devices(&self) -> Result<DeviceList, SyncError> {
        let session = self.token()?;
        self.transport.devices(&session.token).await
    }

    /// Revoke one device's token.
    ///
    /// Revoking the device this client is using leaves it logged out on its
    /// next request; the caller should then clear its [`TokenStore`].
    pub async fn revoke_device(&self, device_id: &str) -> Result<(), SyncError> {
        let session = self.token()?;
        self.transport
            .revoke_device(&session.token, device_id)
            .await
    }

    /// Upload an attachment and return its content address.
    pub async fn upload_attachment(
        &self,
        bytes: &[u8],
        mime: &str,
    ) -> Result<AttachmentRef, SyncError> {
        let session = self.token()?;
        blob::upload(&self.transport, &session.token, bytes, mime).await
    }

    /// Download an attachment and verify its SHA-256.
    pub async fn download_attachment(&self, hash: &str) -> Result<AttachmentData, SyncError> {
        let session = self.token()?;
        blob::download(&self.transport, &session.token, hash).await
    }

    /// Read how much the account stores on the server.
    pub async fn storage_usage(&self) -> Result<StorageUsage, SyncError> {
        let session = self.token()?;
        self.transport.usage(&session.token).await
    }

    /// Delete an attachment from the server.
    ///
    /// The local row and note references are not touched; callers remove those
    /// after this succeeds. Deleting a hash that is already gone succeeds.
    pub async fn delete_attachment(&self, hash: &str) -> Result<(), SyncError> {
        let session = self.token()?;
        self.transport.delete_blob(&session.token, hash).await
    }

    /// Move the attachments one note links to or from the server.
    ///
    /// Local bytes the server has not seen are uploaded; a placeholder whose
    /// bytes are missing locally is downloaded and verified. A placeholder the
    /// server does not have either stays a placeholder and is counted in
    /// [`AttachmentSyncOutcome::missing`], so one broken link cannot stop a
    /// sync run. Every attachment is content-addressed, so this is safe to
    /// call again after a partial run.
    pub async fn sync_note_attachments(
        &self,
        store: &dyn NoteStore,
        note: &NoteId,
        now: i64,
    ) -> Result<AttachmentSyncOutcome, SyncError> {
        let session = self.token()?;
        let mut outcome = AttachmentSyncOutcome::default();
        for attachment in store.note_attachments(note)? {
            if attachment.available {
                if attachment.uploaded {
                    continue;
                }
                let bytes = store.attachment_bytes(&attachment.hash)?.ok_or_else(|| {
                    SyncError::Protocol(format!(
                        "attachment {} is marked available but has no bytes",
                        attachment.hash
                    ))
                })?;
                let uploaded =
                    blob::upload(&self.transport, &session.token, &bytes, &attachment.mime).await?;
                if uploaded.hash != attachment.hash {
                    return Err(SyncError::Protocol(format!(
                        "uploaded {} bytes under hash {}",
                        uploaded.size, attachment.hash
                    )));
                }
                store.mark_attachment_uploaded(&attachment.hash, now)?;
                outcome.uploaded += 1;
                continue;
            }

            match blob::download(&self.transport, &session.token, &attachment.hash).await {
                Ok(data) => {
                    store.put_attachment(&attachment.name, &data.mime, &data.bytes, now)?;
                    store.mark_attachment_uploaded(&attachment.hash, now)?;
                    outcome.downloaded += 1;
                }
                Err(SyncError::Server { status: 404, .. }) => outcome.missing += 1,
                Err(error) => return Err(error),
            }
        }
        Ok(outcome)
    }

    fn token(&self) -> Result<SyncSession, SyncError> {
        self.session.borrow().clone().ok_or(SyncError::NoSession)
    }

    fn keep(
        &self,
        sessions: &dyn TokenStore,
        response: notes_proto::AuthResponse,
    ) -> Result<SyncSession, SyncError> {
        let session = SyncSession {
            username: normalize_username(&self.config.username),
            device_id: response.device_id,
            token: response.token,
        };
        sessions.save(&session)?;
        self.set_session(Some(session.clone()));
        Ok(session)
    }

    /// Build the push for this run and remember what the server will then have.
    fn collect_push(&self, store: &dyn NoteStore) -> Result<PendingPush, SyncError> {
        let mut pending = PendingPush::default();

        if let Some(workspace) = store.workspace_pending()? {
            pending.marks.push(PendingMark {
                note_id: None,
                version: workspace.version,
            });
            if !workspace.updates.is_empty() {
                pending.updates.push(PushUpdate {
                    note_id: workspace_note_id(),
                    blob: Base64Bytes::new(workspace.updates),
                });
            }
        }

        let capacity = MAX_PUSH_UPDATES.saturating_sub(pending.updates.len());
        if capacity == 0 {
            return Ok(pending);
        }
        for id in store.unsynced_notes(capacity)? {
            let note = store.open_note(&id)?;
            let version = encode_version_vector(&note.document().version());
            let since = decode_version(store.last_pushed(&id)?)?;
            let blob = note.document().updates_since(&since)?;
            pending.marks.push(PendingMark {
                note_id: Some(id),
                version,
            });
            if blob.is_empty() {
                // Only the bookkeeping was behind; nothing to send.
                continue;
            }
            if blob.len() > MAX_UPDATE_BYTES {
                return Err(SyncError::Invalid(format!(
                    "note {id} has a {} byte update, the limit is {MAX_UPDATE_BYTES}; \
                     compact the note before syncing",
                    blob.len()
                )));
            }
            pending.updates.push(PushUpdate {
                note_id: id,
                blob: Base64Bytes::new(blob),
            });
        }

        Ok(pending)
    }
}

/// The updates of one run, and the versions the server holds once it accepts.
#[derive(Debug, Default)]
struct PendingPush {
    updates: Vec<PushUpdate>,
    marks: Vec<PendingMark>,
}

#[derive(Debug)]
struct PendingMark {
    /// `None` marks the notebook-list document.
    note_id: Option<NoteId>,
    version: Vec<u8>,
}

/// Decode an encoded `VersionVector`, treating a missing one as "nothing yet".
fn decode_version(bytes: Option<Vec<u8>>) -> Result<VersionVector, SyncError> {
    match bytes {
        Some(bytes) => {
            decode_version_vector(&bytes).map_err(|error| SyncError::Store(error.into()))
        }
        None => Ok(VersionVector::default()),
    }
}
