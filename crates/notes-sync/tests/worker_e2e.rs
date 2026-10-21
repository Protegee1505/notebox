//! P3 end-to-end: two headless clients against a real Worker and D1.
//!
//! Ignored by default. `scripts/sync-e2e.sh` boots `wrangler dev --local`,
//! applies both migrations to a throwaway directory, and runs
//!
//! ```text
//! cargo test -p notes-sync --test worker_e2e -- --ignored --nocapture
//! ```
//!
//! with `NOTEBOX_SYNC_E2E_URL` and `NOTEBOX_SYNC_E2E_SETUP_TOKEN` set. The
//! account must be fresh: setup succeeds once per database.

#![cfg(not(target_arch = "wasm32"))]

use std::time::{SystemTime, UNIX_EPOCH};

use notes_core::Note;
use notes_proto::NoteId;
use notes_store::{NewNote, NoteStore, SqliteStore};
use notes_sync::{
    HttpTransport, MemoryTokenStore, SyncClient, SyncConfig, SyncError, SyncOutcome, TokenStore,
};

const USERNAME: &str = "sync-e2e@notebox";
const PASSWORD: &str = "correct horse battery staple";

/// Seconds since the Unix epoch, used for note and cursor timestamps.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_secs() as i64
}

fn required(name: &str) -> String {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("{name} is not set; run the test through scripts/sync-e2e.sh"))
}

/// One headless device: its database, session and HTTP client.
struct Device {
    client: SyncClient<HttpTransport>,
    store: SqliteStore,
    sessions: MemoryTokenStore,
}

impl Device {
    fn new(name: &str) -> Self {
        let transport =
            HttpTransport::new(&required("NOTEBOX_SYNC_E2E_URL")).expect("the server URL is valid");
        Self {
            client: SyncClient::new(transport, SyncConfig::new(USERNAME, name)),
            store: SqliteStore::open_in_memory().expect("store"),
            sessions: MemoryTokenStore::new(),
        }
    }

    /// Create the account and this device, once per database.
    fn setup(&self) {
        pollster::block_on(self.client.setup(
            &required("NOTEBOX_SYNC_E2E_SETUP_TOKEN"),
            PASSWORD,
            &self.sessions,
        ))
        .expect("setup");
    }

    /// Log in against the account the laptop created.
    fn login(&self) {
        pollster::block_on(self.client.login(PASSWORD, &self.sessions)).expect("login");
    }

    fn sync(&self) -> SyncOutcome {
        pollster::block_on(self.client.sync(&self.store, now())).expect("sync")
    }

    fn create(&self, text: &str) -> Note {
        self.store
            .create_note(NewNote::new(text, now()))
            .expect("create note")
    }

    fn edit(&self, id: &NoteId, text: &str) {
        let note = self.store.open_note(id).expect("open note");
        note.insert(note.char_len(), text).expect("insert");
        self.store.save_note(&note, now()).expect("save note");
    }

    fn text(&self, id: &NoteId) -> String {
        self.store.open_note(id).expect("open note").text()
    }

    fn assert_empty_queue(&self) {
        let unsynced = self.store.unsynced_notes(0).expect("queue");
        let workspace = self.store.workspace_pending().expect("workspace").is_some();
        assert!(
            unsynced.is_empty() && !workspace,
            "unsynced notes {unsynced:?}, workspace pending: {workspace}"
        );
    }
}

#[test]
#[ignore = "needs scripts/sync-e2e.sh"]
fn two_headless_clients_converge_against_the_worker() {
    let laptop = Device::new("laptop");
    laptop.setup();
    let note = laptop.create("shared start");
    let id = note.id();
    let notebook = laptop
        .store
        .create_notebook("Work", now())
        .expect("notebook");
    laptop.sync();
    laptop.assert_empty_queue();

    // The phone joins an existing account and pulls what the laptop pushed.
    let phone = Device::new("phone");
    phone.login();
    let first = phone.sync();
    assert_eq!(first.pulled, 2, "the note and the notebook list arrive");
    assert_eq!(phone.text(&id), "shared start");
    assert_eq!(
        phone
            .store
            .list_notebooks()
            .expect("notebooks")
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>(),
        vec!["Work".to_string()]
    );
    phone.assert_empty_queue();

    // Both devices edit with no connection between them, then catch up.
    laptop.edit(&id, " laptop");
    phone.edit(&id, " phone");
    let laptop_round = laptop.sync();
    let phone_round = phone.sync();
    let laptop_second = laptop.sync();
    assert_eq!(laptop_round.pushed, 1);
    assert_eq!(phone_round.pushed, 1);
    assert_eq!(
        laptop_second.pulled, 1,
        "the phone's edit reaches the laptop"
    );
    let text = laptop.text(&id);
    assert_eq!(text, phone.text(&id), "both devices hold the same text");
    assert!(text.contains("laptop") && text.contains("phone"), "{text}");
    assert_eq!(
        laptop
            .store
            .open_note(&id)
            .expect("open")
            .history()
            .expect("history")
            .len(),
        phone
            .store
            .open_note(&id)
            .expect("open")
            .history()
            .expect("history")
            .len(),
        "converged notes share one history"
    );

    // A notebook rename travels as the reserved workspace document.
    assert!(phone
        .store
        .rename_notebook(&notebook.id, "Work notes")
        .expect("rename"));
    phone.sync();
    laptop.sync();
    assert_eq!(
        laptop
            .store
            .list_notebooks()
            .expect("notebooks")
            .into_iter()
            .map(|entry| entry.name)
            .collect::<Vec<_>>(),
        vec!["Work notes".to_string()]
    );

    // Trash on the phone, restore on the laptop.
    assert!(phone.store.trash_note(&id, now()).expect("trash"));
    phone.sync();
    laptop.sync();
    assert!(laptop
        .store
        .open_note(&id)
        .expect("open")
        .meta()
        .is_trashed());
    assert!(laptop.store.restore_note(&id, now()).expect("restore"));
    laptop.sync();
    phone.sync();
    assert!(!phone
        .store
        .open_note(&id)
        .expect("open")
        .meta()
        .is_trashed());
    assert_eq!(phone.text(&id), text, "trash and restore keep the text");

    // A content-addressed attachment round-trips through D1.
    let payload: Vec<u8> = (0..150_000u32).map(|index| (index % 251) as u8).collect();
    let reference = pollster::block_on(laptop.client.upload_attachment(&payload, "image/webp"))
        .expect("upload attachment");
    assert_eq!(reference.size, payload.len() as u64);
    assert_eq!(reference.chunks, 1);
    let fetched = pollster::block_on(phone.client.download_attachment(&reference.hash))
        .expect("download attachment");
    assert_eq!(fetched.bytes, payload);
    assert_eq!(fetched.mime, "image/webp");

    // The phone is listed, then revoked; its queue survives a new login.
    let devices = pollster::block_on(laptop.client.devices()).expect("devices");
    assert_eq!(devices.devices.len(), 2);
    let phone_id = devices
        .devices
        .iter()
        .find(|device| device.name == "phone")
        .expect("the phone is listed")
        .id
        .clone();
    pollster::block_on(laptop.client.revoke_device(&phone_id)).expect("revoke");
    let error = pollster::block_on(phone.client.sync(&phone.store, now()))
        .expect_err("the token was revoked");
    assert!(matches!(error, SyncError::Unauthorized), "{error}");

    phone.sessions.clear().expect("clear session");
    phone.client.set_session(None);
    phone.login();
    phone.sync();
    phone.assert_empty_queue();
    laptop.assert_empty_queue();
    assert_eq!(phone.text(&id), text);
}
