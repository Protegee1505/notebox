//! P3 engine tests: two headless clients against the real handler stack.
//!
//! The transport below calls [`notes_server::Server`] over
//! [`notes_server::MemoryStorage`] — the same handlers the Worker runs, so
//! validation, paging, own-update filtering and blob storage behave exactly as
//! they do in production. It also injects the two failures that matter: a
//! request that never arrives, and a response that is lost after the server
//! already applied it.
//!
//! The real Worker is covered by `worker_e2e.rs` and `scripts/sync-e2e.sh`.

use std::cell::Cell;
use std::rc::Rc;

use async_trait::async_trait;
use notes_core::Note;
use notes_proto::{
    AuthRequest, AuthResponse, Base64Bytes, BlobChunk, BlobStored, DeviceList, NoteId, SyncRequest,
    SyncResponse,
};
use notes_server::{MemoryStorage, Server, ServerConfig, ServerError};
use notes_store::{NewNote, NoteQuery, NoteStore, NoteView, SqliteStore, SyncState};
use notes_sync::{
    workspace_note_id, AttachmentSyncOutcome, MemoryTokenStore, NoSleep, SyncClient, SyncConfig,
    SyncError, SyncOutcome, SyncTransport, TokenStore,
};

const SETUP_TOKEN: &str = "setup-secret";
const USERNAME: &str = "alice";
const PASSPHRASE: &str = "correct horse battery staple";

/// One in-process server shared by every client in a test.
struct InProcess {
    server: Rc<Server<MemoryStorage>>,
    /// Server-side clock, moved by the tests so backoff is deterministic.
    clock: Rc<Cell<i64>>,
    /// Calls seen so far, counted across every transport method.
    calls: Rc<Cell<u32>>,
    /// Call number that fails before the server sees it.
    fail_at: Rc<Cell<Option<u32>>>,
    /// When set, every call fails as if the network were down.
    fail_all: Rc<Cell<bool>>,
    /// Call number whose response is thrown away after the server applied it.
    lose_at: Rc<Cell<Option<u32>>>,
    /// Call number answered with `429 rate_limited, retry after 3`.
    rate_limit_at: Rc<Cell<Option<u32>>>,
    /// When set, every downloaded chunk comes back with one byte flipped.
    corrupt_blobs: Rc<Cell<bool>>,
}

impl InProcess {
    fn new() -> Self {
        Self {
            server: Rc::new(Server::new(
                MemoryStorage::new(),
                ServerConfig::with_setup_token(SETUP_TOKEN),
            )),
            clock: Rc::new(Cell::new(1_000)),
            calls: Rc::new(Cell::new(0)),
            fail_at: Rc::new(Cell::new(None)),
            fail_all: Rc::new(Cell::new(false)),
            lose_at: Rc::new(Cell::new(None)),
            rate_limit_at: Rc::new(Cell::new(None)),
            corrupt_blobs: Rc::new(Cell::new(false)),
        }
    }

    fn transport(&self) -> TestTransport {
        TestTransport {
            server: self.server.clone(),
            clock: self.clock.clone(),
            calls: self.calls.clone(),
            fail_at: self.fail_at.clone(),
            fail_all: self.fail_all.clone(),
            lose_at: self.lose_at.clone(),
            rate_limit_at: self.rate_limit_at.clone(),
            corrupt_blobs: self.corrupt_blobs.clone(),
        }
    }

    /// Make the next call fail, as if the request never reached the server.
    fn break_the_next_call(&self) {
        self.fail_at.set(Some(self.calls.get() + 1));
    }

    /// Throw away the response of the next call, after the server applied it.
    fn lose_the_next_response(&self) {
        self.lose_at.set(Some(self.calls.get() + 1));
    }

    fn storage(&self) -> &MemoryStorage {
        self.server.storage()
    }
}

/// The transport: the real handlers, plus fault injection.
#[derive(Clone)]
struct TestTransport {
    server: Rc<Server<MemoryStorage>>,
    clock: Rc<Cell<i64>>,
    calls: Rc<Cell<u32>>,
    fail_at: Rc<Cell<Option<u32>>>,
    fail_all: Rc<Cell<bool>>,
    lose_at: Rc<Cell<Option<u32>>>,
    rate_limit_at: Rc<Cell<Option<u32>>>,
    corrupt_blobs: Rc<Cell<bool>>,
}

impl TestTransport {
    /// Count the call and apply the "never arrived" and rate-limit faults.
    fn begin(&self) -> Result<u32, SyncError> {
        let call = self.calls.get() + 1;
        self.calls.set(call);
        if self.fail_all.get() || self.fail_at.get() == Some(call) {
            return Err(SyncError::Transport("test: the network is down".into()));
        }
        if self.rate_limit_at.get() == Some(call) {
            return Err(SyncError::Server {
                status: 429,
                code: "rate_limited".to_string(),
                message: "test: slow down".to_string(),
                retry_after: Some(3),
            });
        }
        Ok(call)
    }

    /// Apply the "the response was lost on the way back" fault.
    fn finish<T>(&self, call: u32, value: T) -> Result<T, SyncError> {
        if self.lose_at.get() == Some(call) {
            return Err(SyncError::Transport("test: the response was lost".into()));
        }
        Ok(value)
    }

    async fn device(&self, token: &str) -> Result<notes_server::DeviceRow, SyncError> {
        self.server
            .authenticate(token, self.clock.get())
            .await
            .map_err(map_error)
    }
}

/// Map a handler error the way the Worker adapter maps it.
fn map_error(error: ServerError) -> SyncError {
    match error {
        ServerError::Unauthorized => SyncError::Unauthorized,
        other => SyncError::Server {
            status: other.status(),
            code: other.code().to_string(),
            message: other.to_string(),
            retry_after: other.retry_after(),
        },
    }
}

#[async_trait(?Send)]
impl SyncTransport for TestTransport {
    async fn setup(
        &self,
        setup_token: &str,
        request: &AuthRequest,
    ) -> Result<AuthResponse, SyncError> {
        let call = self.begin()?;
        let response = self
            .server
            .setup(Some(setup_token), request, self.clock.get())
            .await
            .map_err(map_error)?;
        self.finish(call, response)
    }

    async fn login(&self, request: &AuthRequest) -> Result<AuthResponse, SyncError> {
        let call = self.begin()?;
        let response = self
            .server
            .login(request, self.clock.get())
            .await
            .map_err(map_error)?;
        self.finish(call, response)
    }

    async fn sync(&self, token: &str, request: &SyncRequest) -> Result<SyncResponse, SyncError> {
        let call = self.begin()?;
        let device = self.device(token).await?;
        let response = self
            .server
            .sync(&device, request, self.clock.get())
            .await
            .map_err(map_error)?;
        self.finish(call, response)
    }

    async fn put_blob(&self, token: &str, chunk: &BlobChunk) -> Result<BlobStored, SyncError> {
        let call = self.begin()?;
        self.device(token).await?;
        let stored = self
            .server
            .put_blob(chunk, self.clock.get())
            .await
            .map_err(map_error)?;
        self.finish(call, stored)
    }

    async fn get_blob(&self, token: &str, hash: &str, index: u32) -> Result<BlobChunk, SyncError> {
        let call = self.begin()?;
        self.device(token).await?;
        let mut chunk = self.server.get_blob(hash, index).await.map_err(map_error)?;
        if self.corrupt_blobs.get() {
            let mut data = chunk.data.as_bytes().to_vec();
            if let Some(byte) = data.first_mut() {
                *byte ^= 0xff;
            }
            chunk.data = Base64Bytes::new(data);
        }
        self.finish(call, chunk)
    }

    async fn devices(&self, token: &str) -> Result<DeviceList, SyncError> {
        let call = self.begin()?;
        let device = self.device(token).await?;
        let list = self
            .server
            .list_devices(&device.id)
            .await
            .map_err(map_error)?;
        self.finish(call, list)
    }

    async fn revoke_device(&self, token: &str, device_id: &str) -> Result<(), SyncError> {
        let call = self.begin()?;
        self.device(token).await?;
        let revoked = self
            .server
            .revoke_device(device_id)
            .await
            .map_err(map_error)?;
        if !revoked {
            return Err(SyncError::Server {
                status: 404,
                code: "not_found".to_string(),
                message: "device".to_string(),
                retry_after: None,
            });
        }
        self.finish(call, ())
    }
}

/// One device: its client, its notes database, its stored session and a clock.
struct Device {
    client: SyncClient<TestTransport>,
    store: SqliteStore,
    sessions: MemoryTokenStore,
    clock: Cell<i64>,
}

impl Device {
    fn new(server: &InProcess, name: &str) -> Self {
        Self {
            client: SyncClient::new(server.transport(), SyncConfig::new(USERNAME, name)),
            store: SqliteStore::open_in_memory().expect("store"),
            sessions: MemoryTokenStore::new(),
            clock: Cell::new(1_000),
        }
    }

    /// Create the account and log this device in.
    fn setup(&self) {
        self.advance();
        pollster::block_on(self.client.setup(SETUP_TOKEN, PASSPHRASE, &self.sessions))
            .expect("setup");
    }

    /// Log in against an account that already exists.
    fn login(&self) {
        self.advance();
        pollster::block_on(self.client.login(PASSPHRASE, &self.sessions)).expect("login");
    }

    fn advance(&self) {
        self.clock.set(self.clock.get() + 1);
    }

    fn now(&self) -> i64 {
        self.clock.get()
    }

    fn sync(&self) -> SyncOutcome {
        self.advance();
        pollster::block_on(self.client.sync(&self.store, self.now())).expect("sync")
    }

    fn try_sync(&self) -> Result<SyncOutcome, SyncError> {
        self.advance();
        pollster::block_on(self.client.sync(&self.store, self.now()))
    }

    fn sync_with_retry(&self, sleeper: &NoSleep) -> Result<SyncOutcome, SyncError> {
        pollster::block_on(
            self.client
                .sync_with_retry(&self.store, &|| self.now(), sleeper),
        )
    }

    fn pending(&self) -> usize {
        self.client.pending(&self.store).expect("pending")
    }

    fn create(&self, text: &str) -> Note {
        self.store
            .create_note(NewNote::new(text, self.now()))
            .expect("create")
    }

    fn edit(&self, id: &NoteId, text: &str) {
        let note = self.store.open_note(id).expect("open");
        note.insert(note.char_len(), text).expect("insert");
        self.advance();
        self.store.save_note(&note, self.now()).expect("save");
    }

    fn text(&self, id: &NoteId) -> String {
        self.store.open_note(id).expect("open").text()
    }

    fn history(&self, id: &NoteId) -> Vec<String> {
        self.store
            .open_note(id)
            .expect("open")
            .history()
            .expect("history")
            .into_iter()
            .map(|version| version.id)
            .collect()
    }

    fn assert_empty_queue(&self) {
        assert_eq!(self.pending(), 0, "the offline queue should be empty");
    }
}

fn device(server: &InProcess, name: &str) -> Device {
    Device::new(server, name)
}

#[test]
fn two_devices_converge_after_offline_edits_trash_and_restore() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    let id = laptop.create("shared start").id();
    laptop.sync();
    laptop.assert_empty_queue();

    let phone = device(&server, "phone");
    phone.login();
    phone.sync();
    assert_eq!(phone.text(&id), "shared start");
    assert_eq!(
        phone.pending(),
        0,
        "a pulled note is not queued for a push of its own"
    );

    // Both edit the same note with no connection between them.
    laptop.edit(&id, " laptop");
    phone.edit(&id, " phone");
    laptop.sync();
    phone.sync();
    laptop.sync();

    let laptop_text = laptop.text(&id);
    assert_eq!(laptop_text, phone.text(&id), "text diverged");
    assert!(laptop_text.contains("laptop") && laptop_text.contains("phone"));
    assert_eq!(laptop.history(&id), phone.history(&id), "history diverged");

    // The phone trashes the note; the laptop must see the trash, not a copy.
    phone.advance();
    assert!(phone.store.trash_note(&id, phone.now()).expect("trash"));
    phone.sync();
    laptop.sync();
    assert!(
        laptop
            .store
            .open_note(&id)
            .expect("open")
            .meta()
            .is_trashed(),
        "the trash flag travelled"
    );
    assert_eq!(
        laptop.store.note_count().expect("count"),
        0,
        "a trashed note leaves the active count on both devices"
    );
    assert_eq!(phone.store.note_count().expect("count"), 0);

    // The laptop restores it, and the phone follows.
    laptop.advance();
    assert!(laptop
        .store
        .restore_note(&id, laptop.now())
        .expect("restore"));
    laptop.sync();
    phone.sync();
    assert!(!phone
        .store
        .open_note(&id)
        .expect("open")
        .meta()
        .is_trashed());
    assert_eq!(phone.text(&id), laptop_text, "restore changed no text");
    assert_eq!(laptop.history(&id), phone.history(&id));
    laptop.assert_empty_queue();
    phone.assert_empty_queue();
}

#[test]
fn a_lost_response_does_not_lose_or_duplicate_text() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    let id = laptop.create("hello").id();

    // The server applies the push, then the answer vanishes.
    server.lose_the_next_response();
    let error = laptop.try_sync().expect_err("the response was lost");
    assert!(error.is_retryable());
    assert_eq!(laptop.pending(), 1, "the push is still owed");

    laptop.sync();
    laptop.assert_empty_queue();
    assert_eq!(
        server.storage().update_count(),
        2,
        "the retry re-sends, and an append-only log keeps both copies"
    );

    let phone = device(&server, "phone");
    phone.login();
    let outcome = phone.sync();
    assert_eq!(outcome.pulled, 2);
    assert_eq!(
        phone.text(&id),
        "hello",
        "importing the same update twice is harmless"
    );
    assert_eq!(phone.history(&id).len(), laptop.history(&id).len());
}

#[test]
fn a_failed_request_is_retried_and_then_reported() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    let id = laptop.create("retry me").id();

    // One failed attempt, then success: the waits follow the backoff table.
    server.break_the_next_call();
    let sleeper = NoSleep::new();
    let outcome = laptop.sync_with_retry(&sleeper).expect("retry succeeds");
    assert_eq!(sleeper.waits(), vec![1]);
    assert_eq!(outcome.pushed, 1);
    assert_eq!(laptop.text(&id), "retry me");

    // A rate-limited answer wins over the backoff table.
    let phone = device(&server, "phone");
    phone.login();
    let next = server.calls.get() + 1;
    server.rate_limit_at.set(Some(next));
    let sleeper = NoSleep::new();
    phone.sync_with_retry(&sleeper).expect("retry succeeds");
    assert_eq!(sleeper.waits(), vec![3], "the server's wait is used");

    // A network that never comes back stops after the attempt limit.
    server.fail_all.set(true);
    let sleeper = NoSleep::new();
    let error = laptop
        .sync_with_retry(&sleeper)
        .expect_err("the network stays down");
    assert!(error.is_retryable());
    assert_eq!(
        sleeper.waits(),
        vec![1, 2, 4, 8],
        "{} attempts, {} waits",
        notes_sync::MAX_ATTEMPTS,
        notes_sync::MAX_ATTEMPTS - 1
    );
}

#[test]
fn pushes_are_capped_at_the_server_limit() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    for index in 0..30 {
        laptop.create(&format!("note {index}"));
    }
    assert_eq!(laptop.pending(), 30);

    let first = laptop.sync();
    assert_eq!(first.pushed, notes_proto::MAX_PUSH_UPDATES);
    assert_eq!(laptop.pending(), 5, "the rest waits for the next run");

    let second = laptop.sync();
    assert_eq!(second.pushed, 5);
    assert_eq!(server.storage().update_count(), 30);
    laptop.assert_empty_queue();
}

#[test]
fn a_pull_pages_until_the_server_says_there_is_no_more() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    for index in 0..130 {
        laptop.create(&format!("note {index}"));
    }
    for _ in 0..6 {
        laptop.sync();
    }
    laptop.assert_empty_queue();
    assert_eq!(server.storage().update_count(), 130);

    let phone = device(&server, "phone");
    phone.login();
    let outcome = phone.sync();
    assert_eq!(outcome.pulled, 130);
    assert_eq!(
        outcome.pages, 2,
        "100 rows fit in a page, the rest follow in the next"
    );
    assert!(!outcome.more);
    assert_eq!(
        phone.store.note_count().expect("count"),
        130,
        "every pulled note is stored"
    );
}

#[test]
fn a_page_of_only_own_updates_still_moves_the_cursor() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    for index in 0..130 {
        laptop.create(&format!("note {index}"));
    }
    for _ in 0..6 {
        laptop.sync();
    }

    // Rewind the cursor: every row in the log is this device's own.
    laptop
        .store
        .set_sync_state(&SyncState {
            cursor: Some(0),
            updated_at: 0,
        })
        .expect("rewind");
    let outcome = laptop.sync();
    assert_eq!(outcome.pulled, 0, "own updates are filtered out");
    assert_eq!(outcome.pages, 2);
    assert_eq!(outcome.cursor, 130, "the cursor moves past them anyway");
    assert!(!outcome.more);
}

#[test]
fn a_revoked_device_is_told_to_log_in_again_and_can_log_in() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    let phone = device(&server, "phone");
    phone.login();
    phone.sync();

    let devices = pollster::block_on(laptop.client.devices()).expect("devices");
    assert_eq!(devices.devices.len(), 2);
    let phone_id = devices
        .devices
        .iter()
        .find(|info| info.name == "phone")
        .expect("the phone is listed")
        .id
        .clone();
    assert!(
        devices
            .devices
            .iter()
            .any(|info| info.current && info.name == "laptop"),
        "the calling device is marked as current"
    );

    pollster::block_on(laptop.client.revoke_device(&phone_id)).expect("revoke");
    let error = phone.try_sync().expect_err("the token is gone");
    assert!(error.is_unauthorized(), "the engine says to log in again");
    assert_eq!(phone.pending(), 0, "the notes stay in the local store");

    // Logging in again works without touching the notes.
    phone.clear_session_for_renewal();
    phone.login();
    phone.sync();
}

#[test]
fn the_notebook_list_travels_as_its_own_document() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    laptop.advance();
    let notebook = laptop
        .store
        .create_notebook("Work", laptop.now())
        .expect("notebook");
    assert_eq!(
        laptop.store.note_count().expect("count"),
        0,
        "a notebook is not a note"
    );
    laptop.sync();
    laptop.assert_empty_queue();

    let phone = device(&server, "phone");
    phone.login();
    phone.sync();
    let names: Vec<String> = phone
        .store
        .list_notebooks()
        .expect("notebooks")
        .into_iter()
        .map(|notebook| notebook.name)
        .collect();
    assert_eq!(names, vec!["Work".to_string()]);
    phone.assert_empty_queue();

    // A rename on the phone reaches the laptop.
    phone.advance();
    assert!(phone
        .store
        .rename_notebook(&notebook.id, "Work notes")
        .expect("rename"));
    phone.sync();
    laptop.sync();
    let names: Vec<String> = laptop
        .store
        .list_notebooks()
        .expect("notebooks")
        .into_iter()
        .map(|notebook| notebook.name)
        .collect();
    assert_eq!(names, vec!["Work notes".to_string()]);
}

#[test]
fn the_workspace_document_id_is_stable() {
    assert_eq!(
        workspace_note_id().to_string(),
        "6e6f7465-626f-782d-776f-726b646f6373",
        "every device must agree on the notebook-list id"
    );
    // The id must not collide with a note the store created.
    let store = SqliteStore::open_in_memory().expect("store");
    let note = store
        .create_note(NewNote::new("note", 1))
        .expect("create note");
    assert_ne!(note.id(), workspace_note_id());
}

#[test]
fn attachments_round_trip_between_devices_and_are_verified() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    let phone = device(&server, "phone");
    phone.login();

    let payload: Vec<u8> = (0..2_500_000u32).map(|index| (index % 251) as u8).collect();
    let reference = pollster::block_on(laptop.client.upload_attachment(&payload, "image/webp"))
        .expect("upload");
    assert_eq!(reference.chunks, 3, "2.5 MiB is one chunk short of three");
    assert_eq!(reference.size, payload.len() as u64);
    assert_eq!(reference.hash.len(), 64);
    assert_eq!(server.storage().chunk_count(), 3);

    // Re-uploading is idempotent: the content address is the identity.
    let again = pollster::block_on(laptop.client.upload_attachment(&payload, "image/webp"))
        .expect("upload again");
    assert_eq!(again.hash, reference.hash);
    assert_eq!(server.storage().chunk_count(), 3);

    let fetched = pollster::block_on(phone.client.download_attachment(&reference.hash))
        .expect("download on the other device");
    assert_eq!(fetched.bytes, payload);
    assert_eq!(fetched.mime, "image/webp");

    // A single small attachment is one chunk.
    let small = pollster::block_on(laptop.client.upload_attachment(b"tiny", "text/plain"))
        .expect("upload small");
    assert_eq!(small.chunks, 1);
    assert_eq!(small.size, 4);

    // A server that returns different bytes cannot hide behind the address.
    server.corrupt_blobs.set(true);
    let error = pollster::block_on(phone.client.download_attachment(&reference.hash))
        .expect_err("corrupted bytes must not pass");
    assert!(
        matches!(error, SyncError::HashMismatch { .. }),
        "unexpected error: {error}"
    );
}

#[test]
fn a_note_photo_reaches_the_other_device() {
    let server = InProcess::new();
    let phone = device(&server, "phone");
    phone.setup();
    let desktop = device(&server, "desktop");
    desktop.login();

    // The phone compresses a photo, stores it and writes a note that links it.
    let photo: Vec<u8> = (0..180_000u32).map(|index| (index % 253) as u8).collect();
    let stored = phone
        .store
        .put_attachment("holiday.webp", "image/webp", &photo, phone.now())
        .expect("store the photo");
    let url = notes_core::attachment_url(&stored.hash, &stored.mime);
    let note = phone
        .store
        .create_note(NewNote::new(
            format!("Trip\n\n![holiday]({url})"),
            phone.now(),
        ))
        .expect("create the note");

    let uploaded = pollster::block_on(phone.client.sync_note_attachments(
        &phone.store,
        &note.id(),
        phone.now(),
    ))
    .expect("upload the note's photo");
    assert_eq!(uploaded.uploaded, 1);
    phone.sync();

    // The desktop learns the Markdown link but not the bytes.
    desktop.sync();
    let pulled = desktop.store.open_note(&note.id()).expect("open the note");
    assert_eq!(pulled.text(), note.text());
    let pending = desktop
        .store
        .note_attachments(&note.id())
        .expect("list attachments");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].hash, stored.hash);
    assert!(
        !pending[0].available,
        "a link alone must not pretend to have bytes"
    );
    assert_eq!(
        desktop.store.attachment_bytes(&stored.hash).expect("bytes"),
        None
    );

    // One attachment pass downloads and verifies it; the preview can now show it.
    let downloaded = pollster::block_on(desktop.client.sync_note_attachments(
        &desktop.store,
        &note.id(),
        desktop.now(),
    ))
    .expect("download the photo");
    assert_eq!(downloaded.downloaded, 1);
    let ready = desktop
        .store
        .note_attachments(&note.id())
        .expect("list attachments");
    assert!(ready[0].available);
    assert_eq!(
        desktop.store.attachment_bytes(&stored.hash).expect("bytes"),
        Some(photo)
    );
    assert!(ready[0].uploaded, "downloaded bytes need no re-upload");

    // A second pass is a no-op, and the server has one copy.
    let again = pollster::block_on(desktop.client.sync_note_attachments(
        &desktop.store,
        &note.id(),
        desktop.now(),
    ))
    .expect("no-op pass");
    assert_eq!(again, AttachmentSyncOutcome::default());

    // A broken link stays a placeholder instead of stopping the pass.
    let missing_hash = "c".repeat(64);
    let broken = phone
        .store
        .create_note(NewNote::new(
            format!(
                "Broken\n\n![missing]({})",
                notes_core::attachment_url(&missing_hash, "image/webp")
            ),
            phone.now(),
        ))
        .expect("create a note with a broken link");
    phone.sync();
    desktop.sync();
    let missing = pollster::block_on(desktop.client.sync_note_attachments(
        &desktop.store,
        &broken.id(),
        desktop.now(),
    ))
    .expect("a missing blob is not a failed sync");
    assert_eq!(missing.missing, 1);
    assert!(
        !desktop
            .store
            .note_attachments(&broken.id())
            .expect("list")
            .first()
            .expect("one link")
            .available
    );
}

#[test]
fn a_bad_passphrase_and_a_second_setup_are_refused_with_codes() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();

    let phone = device(&server, "phone");
    phone.advance();
    let error = pollster::block_on(phone.client.login("the wrong words here", &phone.sessions))
        .expect_err("the server refuses");
    match error {
        SyncError::Server { status, code, .. } => {
            assert_eq!(status, 401);
            assert_eq!(code, "invalid_credentials");
        }
        other => panic!("unexpected error: {other}"),
    }

    let second = device(&server, "tablet");
    second.advance();
    let error = pollster::block_on(
        second
            .client
            .setup(SETUP_TOKEN, PASSPHRASE, &second.sessions),
    )
    .expect_err("setup runs once");
    match error {
        SyncError::Server { status, code, .. } => {
            assert_eq!(status, 409);
            assert_eq!(code, "setup_closed");
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn a_client_without_a_session_asks_for_a_login() {
    let server = InProcess::new();
    let device = device(&server, "laptop");
    let error = device.try_sync().expect_err("no session");
    assert!(matches!(error, SyncError::NoSession), "unexpected: {error}");

    // The stored session is cleared on logout, and the notes stay put.
    device.setup();
    let id = device.create("local note").id();
    device.sync();
    device.client.logout(&device.sessions).expect("logout");
    assert_eq!(device.sessions.load().expect("load"), None);
    assert!(device.try_sync().is_err());
    assert_eq!(device.text(&id), "local note");
}

#[test]
fn a_second_login_keeps_the_first_device_working() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    let first = laptop.sessions.load().expect("load").expect("session");

    let phone = device(&server, "phone");
    phone.login();
    let second = phone.sessions.load().expect("load").expect("session");
    assert_ne!(first.token, second.token);
    assert_ne!(first.device_id, second.device_id);
    assert_eq!(first.username, USERNAME);

    // Both tokens keep working.
    laptop.sync();
    phone.sync();
    assert_eq!(server.storage().device_count(), 2);
}

/// List one note's summary, used to prove the store still has the note after
/// a session change.
#[test]
fn a_trashed_note_stays_searchable_until_the_trash_is_emptied() {
    let server = InProcess::new();
    let laptop = device(&server, "laptop");
    laptop.setup();
    let id = laptop.create("find me").id();
    laptop.sync();
    laptop.advance();
    laptop.store.trash_note(&id, laptop.now()).expect("trash");
    laptop.sync();

    let trash = laptop
        .store
        .list_notes(&NoteQuery::page(NoteView::Trash))
        .expect("trash list");
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].id, id);
    assert_eq!(trash[0].title, "find me");
}

impl Device {
    /// Drop the stored session, as a UI would when the server rejects it.
    fn clear_session_for_renewal(&self) {
        self.sessions.clear().expect("clear");
        self.client.set_session(None);
    }
}
