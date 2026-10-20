//! The app side of sync: status, schedule, saved account and background runs.
//!
//! The engine in `notes-sync` knows how to push, pull and merge. This module
//! decides when to run it, where the device keeps its session, and what the UI
//! shows while it happens. It is deliberately free of Dioxus types so the
//! scheduling and conflict rules can be tested on their own.

use std::rc::Rc;

use notes_proto::{DeviceInfo, NoteId};
#[cfg(not(target_arch = "wasm32"))]
use notes_store::NoteStore;
use notes_sync::{
    backoff_seconds, AttachmentSyncOutcome, BoxedTransport, SyncClient, SyncConfig, SyncError,
    SyncOutcome, SyncSession, EDIT_DEBOUNCE_SECS, SYNC_INTERVAL_SECS,
};
use serde::{Deserialize, Serialize};

/// Smallest quiet gap between two automatic runs.
///
/// A focus event, a save and the boot pass can all fall in the same second;
/// this keeps them to one request.
pub const MIN_RUN_GAP_SECS: i64 = 5;

/// Notes whose attachments one run will move.
///
/// Each note can need several chunk requests. Eight keeps one run well inside
/// the free request budget; the next run picks up the rest.
pub const ATTACHMENT_NOTES_PER_RUN: usize = 8;

/// What the status pill and the account card show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SyncStatus {
    /// No account on this device.
    #[default]
    Local,
    /// Login or a session probe is in flight.
    Connecting,
    /// A sync run is in flight.
    Syncing,
    /// Connected, with nothing running.
    Idle,
    /// The Worker could not be reached; the next retry is scheduled.
    Offline,
    /// The device token or cookie is no longer valid.
    NeedsLogin,
    /// The server rejected something this build cannot fix by retrying.
    Error,
}

impl SyncStatus {
    /// Short label for the top bar.
    pub fn short(self) -> &'static str {
        match self {
            SyncStatus::Local => "Local",
            SyncStatus::Connecting => "Connecting",
            SyncStatus::Syncing => "Syncing",
            SyncStatus::Idle => "Synced",
            SyncStatus::Offline => "Offline",
            SyncStatus::NeedsLogin => "Sign in",
            SyncStatus::Error => "Problem",
        }
    }

    /// Longer label for Settings.
    pub fn label(self) -> &'static str {
        match self {
            SyncStatus::Local => "Local only",
            SyncStatus::Connecting => "Connecting to your Worker",
            SyncStatus::Syncing => "Syncing",
            SyncStatus::Idle => "Connected",
            SyncStatus::Offline => "Offline, will retry",
            SyncStatus::NeedsLogin => "Sign in again",
            SyncStatus::Error => "Sync problem",
        }
    }

    /// CSS modifier for the status dot.
    pub fn tone(self) -> &'static str {
        match self {
            SyncStatus::Local => "is-local",
            SyncStatus::Connecting | SyncStatus::Syncing => "is-busy",
            SyncStatus::Idle => "is-ok",
            SyncStatus::Offline => "is-warn",
            SyncStatus::NeedsLogin | SyncStatus::Error => "is-error",
        }
    }

    /// Whether a request is in flight.
    pub fn busy(self) -> bool {
        matches!(self, SyncStatus::Connecting | SyncStatus::Syncing)
    }
}

/// Everything the UI needs about sync.
#[derive(Debug, Clone)]
pub struct SyncUi {
    /// Connection state.
    pub status: SyncStatus,
    /// Unix seconds of the last successful run, if any.
    pub last_synced: Option<i64>,
    /// Last failure, kept for Settings.
    pub error: Option<String>,
    /// Devices that can reach the account.
    pub devices: Vec<DeviceInfo>,
    /// A device-list request is in flight.
    pub devices_loading: bool,
    /// Last device-list failure.
    pub devices_error: Option<String>,
    /// Device id waiting for a second click before revoking.
    pub confirm_revoke: Option<String>,
}

impl Default for SyncUi {
    fn default() -> Self {
        Self {
            status: SyncStatus::Local,
            last_synced: None,
            error: None,
            devices: Vec::new(),
            devices_loading: false,
            devices_error: None,
            confirm_revoke: None,
        }
    }
}

/// Server URL, account and device name. Nothing secret lives here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedAccount {
    /// Worker base URL, without a trailing slash.
    pub server: String,
    /// Account name.
    pub username: String,
    /// Name this device shows in the device list.
    pub device: String,
}

/// A successful login, ready to be put in the runtime.
pub struct LoginSuccess {
    /// Token and device id.
    pub session: SyncSession,
    /// What to remember for the next boot.
    pub account: SavedAccount,
}

/// What one background run did.
#[derive(Debug, Clone)]
pub struct SyncReport {
    /// Push and pull counts.
    pub outcome: SyncOutcome,
    /// Attachment bytes moved.
    pub attachments: AttachmentSyncOutcome,
    /// A note sync succeeded but the attachment pass did not.
    pub attachment_error: Option<String>,
    /// Body of the open note after the merge, when one was open.
    pub open_note: Option<String>,
    /// When the run finished.
    pub finished_at: i64,
}

/// A failure in the form the UI needs.
#[derive(Debug, Clone)]
pub struct SyncFailure {
    /// Readable message for Settings and toasts.
    pub message: String,
    /// The session must be replaced.
    pub unauthorized: bool,
    /// Waiting and trying again can help.
    pub retryable: bool,
    /// Wait the server asked for.
    pub retry_after: Option<i64>,
}

impl From<SyncError> for SyncFailure {
    fn from(error: SyncError) -> Self {
        Self {
            message: friendly_sync_error(&error),
            unauthorized: error.is_unauthorized(),
            retryable: error.is_retryable(),
            retry_after: error.retry_after(),
        }
    }
}

/// State the controller keeps between runs.
#[derive(Default)]
pub struct Runtime {
    /// The client, when a session exists.
    pub client: Option<Rc<SyncClient<BoxedTransport>>>,
    /// The account the client belongs to.
    pub account: Option<SavedAccount>,
    /// A login or sync is in flight.
    pub busy: bool,
    /// Start of the last attempt, for the retry and minimum-gap rules.
    pub last_attempt: i64,
    /// Consecutive failed runs.
    pub failures: u32,
    /// Wait the server asked for on the last failure.
    pub retry_after: Option<i64>,
    /// A permanent failure stopped automatic retries; a manual Sync now
    /// clears it.
    pub stopped: bool,
    /// A run was requested outside the timer (boot, focus or a preserved
    /// conflict).
    pub focus_pending: bool,
    /// Whether the window had focus on the previous tick.
    pub was_focused: bool,
    /// Open note and its body when the run started.
    pub open_base: Option<(NoteId, String)>,
    /// A save held back while the run is in flight.
    pub deferred_save: Option<(NoteId, String, i64)>,
    /// Whether the current run was asked for by the user.
    pub manual_run: bool,
    /// Native login result.
    #[cfg(not(target_arch = "wasm32"))]
    pub login_rx: Option<std::sync::mpsc::Receiver<Result<LoginSuccess, String>>>,
    /// Native sync result.
    #[cfg(not(target_arch = "wasm32"))]
    pub sync_rx: Option<std::sync::mpsc::Receiver<Result<SyncReport, SyncFailure>>>,
    /// Native device-list result: the list, and whether a revoke just ran.
    #[cfg(not(target_arch = "wasm32"))]
    pub devices_rx:
        Option<std::sync::mpsc::Receiver<Result<(notes_proto::DeviceList, bool), String>>>,
}

/// Inputs to the scheduling decision.
#[derive(Debug, Clone, Copy)]
pub struct Due {
    /// A client and account exist.
    pub connected: bool,
    /// A login or run is in flight.
    pub busy: bool,
    /// The page is visible (native builds are always visible).
    pub visible: bool,
    /// The editor has unsaved text.
    pub dirty: bool,
    /// Unix seconds of the last success, or 0.
    pub last_synced: i64,
    /// Unix seconds of the last save, or 0.
    pub last_saved: i64,
    /// Unix seconds the last attempt started, or 0.
    pub last_attempt: i64,
    /// Consecutive failures.
    pub failures: u32,
    /// Server wait from the last failure.
    pub retry_after: Option<i64>,
    /// A permanent failure stopped automatic retries.
    pub stopped: bool,
    /// A focus event asked for a run.
    pub focus: bool,
}

impl Due {
    /// A connected, idle, visible app with no edits.
    #[cfg(test)]
    pub fn connected() -> Self {
        Self {
            connected: true,
            busy: false,
            visible: true,
            dirty: false,
            last_synced: 1_000,
            last_saved: 1_000,
            last_attempt: 0,
            failures: 0,
            retry_after: None,
            stopped: false,
            focus: false,
        }
    }
}

/// Whether a run should start now.
///
/// The rules are PLAN.md's loop: on start, three seconds after the last edit,
/// every sixty seconds while visible, and on focus. A dirty editor waits for
/// autosave; a hidden page never runs; failures back off.
pub fn is_due(now: i64, due: &Due) -> bool {
    if !due.connected || due.busy || !due.visible || due.dirty || due.stopped {
        return false;
    }
    if due.last_attempt > 0 && now - due.last_attempt < MIN_RUN_GAP_SECS {
        return false;
    }
    if due.failures > 0 {
        let wait = backoff_seconds(due.failures, due.retry_after);
        return now >= due.last_attempt + wait;
    }
    if due.focus || due.last_synced == 0 {
        return true;
    }
    if due.last_saved > due.last_synced && now - due.last_saved >= EDIT_DEBOUNCE_SECS {
        return true;
    }
    now - due.last_synced >= SYNC_INTERVAL_SECS
}

/// What to do with the open editor after a run pulled changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenNotePlan {
    /// The remote body matches the base; nothing changed.
    Unchanged,
    /// The editor stayed clean, so the merged body can replace it.
    Reload(String),
    /// Both sides changed. Neither body may be thrown away.
    Conflict {
        /// The text the user typed during the run.
        keep: String,
        /// The merged body from the store.
        remote: String,
    },
}

/// Decide without touching the store.
pub fn plan_open_note(base: &str, draft: &str, remote: &str) -> OpenNotePlan {
    if remote == base {
        OpenNotePlan::Unchanged
    } else if draft == base || draft == remote {
        OpenNotePlan::Reload(remote.to_string())
    } else {
        OpenNotePlan::Conflict {
            keep: draft.to_string(),
            remote: remote.to_string(),
        }
    }
}

/// A readable message for the account screen.
pub fn friendly_login_error(error: &SyncError) -> String {
    match error {
        SyncError::Unauthorized => "That username and passphrase did not match.".to_string(),
        SyncError::Transport(_) => {
            "Could not reach your Worker. Check the URL and your connection.".to_string()
        }
        SyncError::Server {
            status: 429,
            retry_after,
            ..
        } => match retry_after {
            Some(seconds) => format!("Too many attempts. Try again in {seconds} seconds."),
            None => "Too many attempts. Wait a moment and try again.".to_string(),
        },
        SyncError::Invalid(message) => message.clone(),
        other => other.to_string(),
    }
}

/// A readable message for the status card.
pub fn friendly_sync_error(error: &SyncError) -> String {
    match error {
        SyncError::Unauthorized => {
            "This device was signed out. Sign in with your passphrase again.".to_string()
        }
        SyncError::Transport(_) => {
            "Could not reach your Worker. Your notes stay here and will sync when it is back."
                .to_string()
        }
        SyncError::Server { status, .. } if *status >= 500 => {
            "The Worker had a problem. Your notes are safe here; the next attempt is scheduled."
                .to_string()
        }
        SyncError::Server {
            status: 429,
            retry_after,
            ..
        } => match retry_after {
            Some(seconds) => format!("The Worker asked for a {seconds} second pause."),
            None => "The Worker asked for a pause before the next attempt.".to_string(),
        },
        SyncError::Invalid(message) => message.clone(),
        other => other.to_string(),
    }
}

/// Build the platform transport for a Worker URL.
pub fn transport(server: &str) -> Result<BoxedTransport, SyncError> {
    #[cfg(target_arch = "wasm32")]
    {
        Ok(Box::new(notes_sync::WebTransport::new(server)?))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(Box::new(notes_sync::HttpTransport::new(server)?))
    }
}

/// Build a client with an optional session.
pub fn client(
    server: &str,
    username: &str,
    device: &str,
    session: Option<SyncSession>,
) -> Result<Rc<SyncClient<BoxedTransport>>, SyncError> {
    let transport = transport(server)?;
    Ok(Rc::new(SyncClient::with_session(
        transport,
        SyncConfig::new(username, device),
        session,
    )))
}

/// A placeholder session for the browser, where the cookie is the credential.
///
/// The transport ignores an empty bearer token, and `fetch` sends the
/// `HttpOnly` cookie by itself.
#[cfg(target_arch = "wasm32")]
pub fn cookie_session(username: &str) -> SyncSession {
    SyncSession {
        username: username.to_string(),
        device_id: String::new(),
        token: String::new(),
    }
}

/// The browser origin, which is the Worker when the app is served by it.
#[cfg(target_arch = "wasm32")]
pub fn default_server() -> String {
    web_sys::window()
        .map(|window| window.origin())
        .unwrap_or_default()
}

/// Native builds have no origin to guess from.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_server() -> String {
    String::new()
}

/// A useful default for the device list.
pub fn default_device_name() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        "Web browser".to_string()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::env::consts::OS {
            "linux" => "Linux desktop",
            "windows" => "Windows desktop",
            "macos" => "Mac",
            "android" => "Android",
            other => other,
        }
        .to_string()
    }
}

/// Whether the page is visible. Native windows are always visible to us.
#[cfg(target_arch = "wasm32")]
pub fn document_visible() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .map(|document| document.visibility_state() == web_sys::VisibilityState::Visible)
        .unwrap_or(true)
}

/// Native builds have no page visibility to consult.
#[cfg(not(target_arch = "wasm32"))]
pub fn document_visible() -> bool {
    true
}

/// Whether the window has keyboard focus.
#[cfg(target_arch = "wasm32")]
pub fn document_focused() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .map(|document| document.has_focus().unwrap_or(true))
        .unwrap_or(true)
}

/// Native focus events are not exposed here; the timer covers this target.
#[cfg(not(target_arch = "wasm32"))]
pub fn document_focused() -> bool {
    true
}

#[cfg(target_arch = "wasm32")]
const ACCOUNT_KEY: &str = "notebox.account";

/// Read the saved account, if there is one.
pub fn load_account() -> Option<SavedAccount> {
    #[cfg(target_arch = "wasm32")]
    {
        let storage = web_sys::window()?.local_storage().ok()??;
        let text = storage.get_item(ACCOUNT_KEY).ok()??;
        serde_json::from_str(&text).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let bytes = std::fs::read(crate::storage::account_path()).ok()?;
        serde_json::from_slice(&bytes).ok()
    }
}

/// Remember the account for the next boot.
pub fn save_account(account: &SavedAccount) -> Result<(), String> {
    let json = serde_json::to_vec(account).map_err(|error| error.to_string())?;
    #[cfg(target_arch = "wasm32")]
    {
        let storage = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or_else(|| "this browser does not allow local storage".to_string())?;
        let text = String::from_utf8(json).map_err(|error| error.to_string())?;
        storage
            .set_item(ACCOUNT_KEY, &text)
            .map_err(|_| "could not save the account in this browser".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        write_atomic(&crate::storage::account_path(), &json)
    }
}

/// Forget the saved account.
pub fn clear_account() -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        let storage = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or_else(|| "this browser does not allow local storage".to_string())?;
        storage
            .remove_item(ACCOUNT_KEY)
            .map_err(|_| "could not clear the account in this browser".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match std::fs::remove_file(crate::storage::account_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Load the native device token.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_session() -> Option<SyncSession> {
    use notes_sync::TokenStore as _;

    notes_sync::FileTokenStore::new(crate::storage::session_path())
        .load()
        .ok()
        .flatten()
}

/// Forget the native device token.
#[cfg(not(target_arch = "wasm32"))]
pub fn clear_session() {
    use notes_sync::TokenStore as _;

    let _ = notes_sync::FileTokenStore::new(crate::storage::session_path()).clear();
}

/// Log in on a native build. Blocking; call it on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn login_blocking(
    server: &str,
    username: &str,
    device: &str,
    passphrase: &str,
) -> Result<LoginSuccess, SyncError> {
    let account = SavedAccount {
        server: server.trim().trim_end_matches('/').to_string(),
        username: username.trim().to_string(),
        device: device.trim().to_string(),
    };
    let transport = notes_sync::HttpTransport::new(&account.server)?;
    let client = SyncClient::new(
        transport,
        SyncConfig::new(account.username.as_str(), account.device.as_str()),
    );
    let sessions = notes_sync::FileTokenStore::new(crate::storage::session_path());
    let session = pollster::block_on(client.login(passphrase, &sessions))?;
    save_account(&account).map_err(SyncError::Session)?;
    Ok(LoginSuccess { session, account })
}

/// One full native run on its own database connection. Blocking; use a thread.
#[cfg(not(target_arch = "wasm32"))]
pub struct NativeSyncJob {
    /// Database file, opened separately from the UI connection.
    pub database: std::path::PathBuf,
    /// Account and server.
    pub account: SavedAccount,
    /// Device token.
    pub session: SyncSession,
    /// Note open in the editor, so its merged body can be compared.
    pub open_note: Option<NoteId>,
    /// Start time, used for imported commits.
    pub started_at: i64,
}

/// Run the engine and the attachment pass against a native database.
#[cfg(not(target_arch = "wasm32"))]
pub fn sync_blocking(job: NativeSyncJob) -> Result<SyncReport, SyncError> {
    let store = notes_store::SqliteStore::open(&job.database)?;
    let transport = notes_sync::HttpTransport::new(&job.account.server)?;
    let client = SyncClient::with_session(
        transport,
        SyncConfig::new(job.account.username.as_str(), job.account.device.as_str()),
        Some(job.session),
    );
    let outcome = pollster::block_on(client.sync(&store, job.started_at))?;

    // The note cursor is already committed. An attachment that fails on this
    // run stays in the work queue and is reported without undoing the notes.
    let mut attachments = AttachmentSyncOutcome::default();
    let mut attachment_error = None;
    for note in store.notes_with_attachment_work(ATTACHMENT_NOTES_PER_RUN)? {
        match pollster::block_on(client.sync_note_attachments(&store, &note, job.started_at)) {
            Ok(pass) => {
                attachments.uploaded += pass.uploaded;
                attachments.downloaded += pass.downloaded;
                attachments.missing += pass.missing;
            }
            Err(error) if error.is_unauthorized() => return Err(error),
            Err(error) => {
                attachment_error = Some(friendly_sync_error(&error));
                break;
            }
        }
    }

    let open_note = job
        .open_note
        .and_then(|id| store.open_note(&id).ok())
        .map(|note| note.text());
    Ok(SyncReport {
        outcome,
        attachments,
        attachment_error,
        open_note,
        finished_at: crate::time::now(),
    })
}

/// Fetch the device list on a native build. Blocking; use a thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn devices_blocking(
    account: &SavedAccount,
    session: &SyncSession,
) -> Result<notes_proto::DeviceList, SyncError> {
    let client = client(
        &account.server,
        &account.username,
        &account.device,
        Some(session.clone()),
    )?;
    pollster::block_on(client.devices())
}

/// Revoke one device on a native build. Blocking; use a thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn revoke_blocking(
    account: &SavedAccount,
    session: &SyncSession,
    device_id: &str,
) -> Result<(), SyncError> {
    let client = client(
        &account.server,
        &account.username,
        &account.device,
        Some(session.clone()),
    )?;
    pollster::block_on(client.revoke_device(device_id))
}

/// Atomically write a small JSON file.
#[cfg(not(target_arch = "wasm32"))]
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

/// Attachments moved and notes changed, for a toast.
pub fn run_summary(report: &SyncReport) -> String {
    let mut parts = Vec::new();
    if report.outcome.pushed > 0 {
        parts.push(format!("{} sent", report.outcome.pushed));
    }
    if report.outcome.pulled > 0 {
        parts.push(format!("{} received", report.outcome.pulled));
    }
    if report.attachments.uploaded > 0 || report.attachments.downloaded > 0 {
        parts.push(format!(
            "{} image{} moved",
            report.attachments.uploaded + report.attachments.downloaded,
            if report.attachments.uploaded + report.attachments.downloaded == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if parts.is_empty() {
        "Up to date".to_string()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::{is_due, plan_open_note, Due, OpenNotePlan, MIN_RUN_GAP_SECS};

    #[test]
    fn a_fresh_account_syncs_on_first_tick() {
        let due = Due {
            last_synced: 0,
            ..Due::connected()
        };
        assert!(is_due(1_001, &due));
    }

    #[test]
    fn an_edit_waits_for_the_quiet_gap() {
        let due = Due {
            last_synced: 1_000,
            last_saved: 1_010,
            last_attempt: 0,
            ..Due::connected()
        };
        assert!(!is_due(1_012, &due), "less than three seconds of quiet");
        assert!(is_due(1_013, &due));
    }

    #[test]
    fn the_minute_timer_covers_an_idle_app() {
        let due = Due {
            last_synced: 1_000,
            last_saved: 1_000,
            ..Due::connected()
        };
        assert!(!is_due(1_030, &due));
        assert!(is_due(1_060, &due));
    }

    #[test]
    fn a_dirty_or_hidden_editor_never_runs() {
        let dirty = Due {
            dirty: true,
            last_synced: 0,
            ..Due::connected()
        };
        assert!(!is_due(1_000, &dirty));
        let hidden = Due {
            visible: false,
            last_synced: 0,
            ..Due::connected()
        };
        assert!(!is_due(1_000, &hidden));
    }

    #[test]
    fn focus_asks_for_a_run_but_the_minimum_gap_still_applies() {
        let due = Due {
            focus: true,
            last_synced: 1_000,
            last_attempt: 1_000,
            ..Due::connected()
        };
        assert!(!is_due(1_000 + MIN_RUN_GAP_SECS - 1, &due));
        assert!(is_due(1_000 + MIN_RUN_GAP_SECS, &due));
    }

    #[test]
    fn failures_back_off_and_then_try_again() {
        let due = Due {
            failures: 2,
            last_synced: 1_000,
            last_attempt: 1_000,
            ..Due::connected()
        };
        assert!(!is_due(1_001, &due));
        assert!(
            !is_due(1_004, &due),
            "the five-second minimum gap still applies"
        );
        assert!(is_due(1_005, &due));
    }

    #[test]
    fn a_permanent_failure_waits_for_a_manual_retry() {
        let due = Due {
            failures: 1,
            last_attempt: 1_000,
            stopped: true,
            ..Due::connected()
        };
        assert!(!is_due(2_000, &due));
    }

    #[test]
    fn the_server_wait_wins_over_the_exponential_backoff() {
        let due = Due {
            failures: 1,
            last_synced: 1_000,
            last_attempt: 1_000,
            retry_after: Some(30),
            ..Due::connected()
        };
        assert!(!is_due(1_020, &due));
        assert!(is_due(1_030, &due));
    }

    #[test]
    fn a_clean_editor_takes_the_merged_body() {
        assert_eq!(
            plan_open_note("old", "old", "old + remote"),
            OpenNotePlan::Reload("old + remote".to_string())
        );
    }

    #[test]
    fn a_dirty_editor_keeps_both_bodies() {
        assert_eq!(
            plan_open_note("old", "old + mine", "old + remote"),
            OpenNotePlan::Conflict {
                keep: "old + mine".to_string(),
                remote: "old + remote".to_string()
            }
        );
    }

    #[test]
    fn identical_edits_on_both_sides_are_not_a_conflict() {
        assert_eq!(
            plan_open_note("old", "old + same", "old + same"),
            OpenNotePlan::Reload("old + same".to_string())
        );
    }

    #[test]
    fn an_unrelated_pull_leaves_the_editor_alone() {
        assert_eq!(
            plan_open_note("same", "same", "same"),
            OpenNotePlan::Unchanged
        );
    }
}
