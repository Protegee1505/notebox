//! The app's state and the actions the UI can take.
//!
//! State is plain data in one `Signal`; the store sits beside it behind a
//! `Rc<dyn NoteStore>`. Components read state and call these methods, which
//! keeps the view free of storage details and gives the browser worker in P7
//! one place to plug into.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use notes_core::{Note, NoteVersion};
use notes_proto::{NoteId, Notebook, NotebookId};
use notes_store::{
    Attachment, NewNote, NoteQuery, NoteStore, NoteSummary, NoteView, SearchQuery, TagCount,
};
use notes_sync::BoxedTransport;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::closure::Closure;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
use web_time::Instant;

use crate::images;
use crate::markdown_ops::{self, Mark};
use crate::media;
use crate::storage::{self, Persistence, StorageInfo};
use crate::sync::{self, Due, OpenNotePlan, SyncFailure, SyncReport, SyncStatus, SyncUi};
use crate::time::{now, relative_time};

/// Top-level screens. Notes and Settings keep the sidebar; Login is full page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Note list and editor.
    Notes,
    /// Preferences, account and storage.
    Settings,
    /// The account form.
    Login,
}

/// Colour theme choice. `System` follows the operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Follow `prefers-color-scheme`.
    System,
    /// Light palette.
    Light,
    /// Dark palette.
    Dark,
}

impl Theme {
    /// Value written to the root element.
    pub fn attr(self) -> &'static str {
        match self {
            Theme::System => "system",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    /// Label for the settings control.
    pub fn label(self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
        }
    }
}

/// Card grid or compact list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Keep-style cards.
    Grid,
    /// One row per note.
    List,
}

impl Layout {
    /// Label for the settings control.
    pub fn label(self) -> &'static str {
        match self {
            Layout::Grid => "Cards",
            Layout::List => "List",
        }
    }
}

/// Editor shows the Markdown source or the rendered note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    /// Editable Markdown text box.
    Write,
    /// Rendered, read-only view.
    Preview,
}

/// How the history panel shows a selected version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryMode {
    /// Unified diff from the old version to the current body.
    Changes,
    /// The note exactly as it was after that version.
    Preview,
}

impl HistoryMode {
    /// Label for the segmented control.
    pub fn label(self) -> &'static str {
        match self {
            HistoryMode::Changes => "Changes",
            HistoryMode::Preview => "Preview",
        }
    }
}

/// Tone of a status message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// Neutral information.
    Info,
    /// An action succeeded.
    Success,
    /// An action failed.
    Danger,
}

/// A transient message at the bottom of the screen.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Text to show.
    pub message: String,
    /// Colour and icon.
    pub kind: ToastKind,
    /// When it was raised, for expiry.
    pub at: Instant,
}

/// The local profile badge. Sync details live in [`SyncUi`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Display name.
    pub username: String,
    /// Whether a server session exists.
    pub sync_connected: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            username: "Local".to_string(),
            sync_connected: false,
        }
    }
}

/// Fields on the login screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoginForm {
    /// Account name.
    pub username: String,
    /// Cleared as soon as an attempt starts; never stored.
    pub passphrase: String,
    /// Worker base URL.
    pub server: String,
    /// Device name shown in the account's device list.
    pub device: String,
    /// Validation message.
    pub error: Option<String>,
}

/// Everything the view needs for one render.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Current screen.
    pub screen: Screen,
    /// List filter.
    pub view: NoteView,
    /// Notebook filter.
    pub notebook: Option<NotebookId>,
    /// Tag filter.
    pub tag: Option<String>,
    /// Search text.
    pub search: String,
    /// Notes matching the current filter and search.
    pub notes: Vec<NoteSummary>,
    /// Snippets for the current search, paired by note.
    pub snippets: Vec<(NoteId, String)>,
    /// Number of search hits, or `None` when the search box is empty.
    pub search_hits: Option<usize>,
    /// Notebooks, oldest first.
    pub notebooks: Vec<Notebook>,
    /// Tags with counts.
    pub tags: Vec<TagCount>,
    /// Notes outside the trash.
    pub note_count: u64,
    /// Open note.
    pub selected: Option<NoteId>,
    /// Attachments the open note links, in the order first added.
    pub attachments: Vec<Attachment>,
    /// Inline image URLs by content hash, for the rendered view.
    pub attachment_urls: HashMap<String, String>,
    /// Images being compressed before they are stored.
    pub attaching: usize,
    /// Editor body.
    pub draft: String,
    /// Body as last written to SQLite.
    pub saved_text: String,
    /// Whether the draft differs from the saved body.
    pub dirty: bool,
    /// Unix seconds of the last save.
    pub saved_at: i64,
    /// Inputs since this note was opened, used by the autosave ticker.
    pub edit_seq: u64,
    /// Input count the ticker last observed.
    pub seen_seq: u64,
    /// Cards or list.
    pub layout: Layout,
    /// Theme.
    pub theme: Theme,
    /// Markdown source or rendered view.
    pub editor_mode: EditorMode,
    /// Find-in-note bar.
    pub find_open: bool,
    /// Find text.
    pub find_query: String,
    /// Zero-based current match.
    pub find_index: usize,
    /// Command palette.
    pub palette_open: bool,
    /// Palette filter.
    pub palette_query: String,
    /// Highlighted palette row.
    pub palette_index: usize,
    /// Keyboard-shortcut sheet.
    pub shortcuts_open: bool,
    /// Sidebar drawer on narrow screens.
    pub sidebar_open: bool,
    /// Which single pane a phone shows.
    pub mobile_list: bool,
    /// Colour menu under the editor header.
    pub color_open: bool,
    /// History panel over the editor.
    pub history_open: bool,
    /// Versions of the open note, oldest first.
    pub history_versions: Vec<NoteVersion>,
    /// Version id shown in the history detail.
    pub history_selected: Option<String>,
    /// Version whose text matches the current body.
    pub history_current: Option<String>,
    /// Peer id this device writes history under.
    pub history_local_peer: u64,
    /// Diff or read-only preview.
    pub history_mode: HistoryMode,
    /// Body as it was after the selected version.
    pub history_preview: String,
    /// Unified diff from the selected version to the current body.
    pub history_diff: String,
    /// History load error, shown inside the panel.
    pub history_error: Option<String>,
    /// Inline notebook-name field in the sidebar.
    pub new_notebook: Option<String>,
    /// "Empty trash" requires a second click.
    pub confirm_empty_trash: bool,
    /// Latest toast.
    pub toast: Option<Toast>,
    /// Local undo snapshots.
    pub undo: Vec<String>,
    /// Local redo snapshots.
    pub redo: Vec<String>,
    /// When the last undo snapshot was taken.
    pub last_undo: Option<Instant>,
    /// Local storage description.
    pub storage: StorageInfo,
    /// Error opening storage, shown instead of the note list.
    pub storage_error: Option<String>,
    /// Waiting for the browser's saved database.
    pub starting: bool,
    /// Another tab owns browser storage, shown instead of the note list.
    pub storage_blocked: Option<String>,
    /// Local profile.
    pub profile: Profile,
    /// Login form.
    pub login: LoginForm,
    /// Connection state, last run, and devices.
    pub sync: SyncUi,
    /// Minute of the last clock tick, so relative times refresh.
    pub clock_minute: i64,
}

impl AppState {
    fn new(storage: StorageInfo, storage_error: Option<String>) -> Self {
        let saved = sync::load_account();
        let (server, username, device) = match saved {
            Some(account) => (account.server, account.username, account.device),
            None => (
                sync::default_server(),
                String::new(),
                sync::default_device_name(),
            ),
        };
        Self {
            screen: Screen::Notes,
            view: NoteView::Active,
            notebook: None,
            tag: None,
            search: String::new(),
            notes: Vec::new(),
            snippets: Vec::new(),
            search_hits: None,
            notebooks: Vec::new(),
            tags: Vec::new(),
            note_count: 0,
            selected: None,
            attachments: Vec::new(),
            attachment_urls: HashMap::new(),
            attaching: 0,
            draft: String::new(),
            saved_text: String::new(),
            dirty: false,
            saved_at: 0,
            edit_seq: 0,
            seen_seq: 0,
            layout: Layout::Grid,
            theme: Theme::System,
            editor_mode: EditorMode::Write,
            find_open: false,
            find_query: String::new(),
            find_index: 0,
            palette_open: false,
            palette_query: String::new(),
            palette_index: 0,
            shortcuts_open: false,
            sidebar_open: false,
            mobile_list: true,
            color_open: false,
            history_open: false,
            history_versions: Vec::new(),
            history_selected: None,
            history_current: None,
            history_local_peer: 0,
            history_mode: HistoryMode::Changes,
            history_preview: String::new(),
            history_diff: String::new(),
            history_error: None,
            new_notebook: None,
            confirm_empty_trash: false,
            toast: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_undo: None,
            storage,
            storage_error,
            starting: true,
            storage_blocked: None,
            profile: Profile::default(),
            login: LoginForm {
                server,
                username,
                device,
                ..LoginForm::default()
            },
            sync: SyncUi::default(),
            clock_minute: now() / 60,
        }
    }
}

/// Shared handle passed through the component tree.
#[derive(Clone)]
pub struct AppContext {
    store: Rc<OnceCell<StoreHandle>>,
    /// Reactive app state.
    pub state: Signal<AppState>,
    runtime: Rc<RefCell<sync::Runtime>>,
}

struct StoreHandle {
    store: Rc<dyn NoteStore>,
    persistence: Option<Rc<Persistence>>,
}

impl AppContext {
    /// Build the context immediately; storage opens in [`AppContext::start`].
    pub fn new() -> Self {
        let state = Signal::new(AppState::new(
            StorageInfo {
                label: "Opening".to_string(),
                detail: "Looking for your local notes.".to_string(),
                temporary: true,
            },
            None,
        ));
        Self {
            store: Rc::new(OnceCell::new()),
            state,
            runtime: Rc::new(RefCell::new(sync::Runtime::default())),
        }
    }

    /// Open storage, load the first screen and restore a saved session.
    pub async fn start(&self) {
        match storage::open().await {
            Ok(opened) => {
                let sqlite = opened.store;
                let store: Rc<dyn NoteStore> = sqlite.clone();
                let handle = StoreHandle {
                    store,
                    persistence: opened.persistence,
                };
                if self.store.set(handle).is_err() {
                    return;
                }
                {
                    let mut state = self.state.write_unchecked();
                    state.storage = opened.info;
                    state.starting = false;
                }
                self.bootstrap();
                self.restore_sync();
                #[cfg(target_arch = "wasm32")]
                self.install_pagehide();
            }
            Err(failure) => {
                let blocked = matches!(failure, storage::OpenFailure::Locked);
                let message = failure.message();
                let mut state = self.state.write_unchecked();
                state.starting = false;
                if blocked {
                    state.storage_blocked = Some(message);
                } else {
                    state.storage_error = Some(message);
                }
            }
        }
    }

    /// Save the draft and the latest image when the tab goes away.
    ///
    /// The 700 ms tick normally covers this. A page hidden or closed inside
    /// that window would otherwise lose the newest edit; `flush_now` also
    /// sends past an image that is still being written.
    #[cfg(target_arch = "wasm32")]
    fn install_pagehide(&self) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let context = self.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            context.save_now();
            if let Some(handle) = context.store.get() {
                if let Some(persistence) = &handle.persistence {
                    let _ = persistence.flush_now();
                }
            }
        });
        let Ok(function) = closure.into_js_value().dyn_into::<js_sys::Function>() else {
            return;
        };
        let _ = window.add_event_listener_with_callback("pagehide", &function);
    }

    /// The store, when it opened.
    pub fn store(&self) -> Option<&dyn NoteStore> {
        self.store.get().map(|handle| handle.store.as_ref())
    }

    #[cfg(target_arch = "wasm32")]
    fn store_rc(&self) -> Option<Rc<dyn NoteStore>> {
        self.store.get().map(|handle| handle.store.clone())
    }

    /// Save the browser image if SQLite changed since the last one.
    fn persist_if_needed(&self) {
        let Some(handle) = self.store.get() else {
            return;
        };
        let Some(persistence) = handle.persistence.as_ref() else {
            return;
        };
        let Err(error) = persistence.flush_if_needed() else {
            return;
        };
        {
            let mut state = self.state.write_unchecked();
            if state.storage.temporary {
                return;
            }
            state.storage = StorageInfo {
                label: "Browser memory".to_string(),
                detail: format!(
                    "Could not save to browser storage ({error}). Notes stay in this tab only; export a zip to keep them."
                ),
                temporary: true,
            };
        }
        self.toast_danger("Could not save to browser storage. Export a zip to keep your notes.");
    }

    fn bootstrap(&self) {
        let Some(store) = self.store() else {
            return;
        };
        // With a saved account, an empty database means the account's notes
        // are about to be pulled. Seeding welcome notes there would push demo
        // content into the account, and on the in-memory web store it would
        // add a fresh copy on every reload.
        let account_saved = sync::load_account().is_some();
        match store.note_count() {
            Ok(0) if !account_saved => self.seed(store),
            Ok(_) => {}
            Err(error) => self.toast_danger(format!("Could not read notes: {error}")),
        }
        self.reload();
        let first = self.state.peek().notes.first().map(|note| note.id);
        if let Some(id) = first {
            self.select_note(id);
        }
    }

    /// Pick up a saved account and token, if both exist.
    ///
    /// The web build has no token file: its credential is the `HttpOnly`
    /// cookie, so an empty bearer token is enough. A missing or expired cookie
    /// shows up as `Unauthorized` on the first run and turns into “Sign in”.
    fn restore_sync(&self) {
        if let Some(store) = self.store() {
            let last = store
                .sync_state()
                .ok()
                .and_then(|state| (state.updated_at > 0).then_some(state.updated_at));
            self.state.write_unchecked().sync.last_synced = last;
        }
        let Some(account) = sync::load_account() else {
            return;
        };

        #[cfg(not(target_arch = "wasm32"))]
        let session = match sync::load_session() {
            Some(session) if session.username == account.username => Some(session),
            Some(_) => {
                // A token from another account cannot be used with this one.
                sync::clear_session();
                None
            }
            None => None,
        };
        #[cfg(target_arch = "wasm32")]
        let session = Some(sync::cookie_session(&account.username));

        let Some(session) = session else {
            return;
        };
        let client = match sync::client(
            &account.server,
            &account.username,
            &account.device,
            Some(session),
        ) {
            Ok(client) => client,
            Err(error) => {
                self.state.write_unchecked().sync.error = Some(sync::friendly_login_error(&error));
                return;
            }
        };

        {
            let mut runtime = self.runtime.borrow_mut();
            runtime.client = Some(client);
            runtime.account = Some(account.clone());
            // The first tick runs the boot sync once the UI is interactive.
            runtime.focus_pending = true;
        }
        let mut state = self.state.write_unchecked();
        state.profile.username = account.username.clone();
        state.profile.sync_connected = true;
        // A saved account with no past run has not synced yet; the boot run
        // is about to start on the first tick.
        state.sync.status = if state.sync.last_synced.is_some() {
            SyncStatus::Idle
        } else {
            SyncStatus::Connecting
        };
        state.login.server = account.server;
        state.login.username = account.username;
        state.login.device = account.device;
    }

    /// A small welcome set, only on a brand-new database.
    fn seed(&self, store: &dyn NoteStore) {
        let timestamp = now();
        let personal = store
            .create_notebook("Personal", timestamp - 86_400 * 3)
            .ok();
        let projects = store
            .create_notebook("Projects", timestamp - 86_400 * 2)
            .ok();

        let welcome = "Welcome to Notebox\n\n\
            A local-first place for your notes. Everything you write stays on this device.\n\n\
            ## Try the basics\n\n\
            - [ ] Press **Ctrl K** to open the command palette\n\
            - [ ] Type Markdown and switch to the rendered view with the eye button\n\
            - [ ] Pin a note to keep it at the top of the list\n\n\
            > Edits are saved automatically as you type.\n\n\
            ## Shortcuts\n\n\
            | Keys | Action |\n\
            | --- | --- |\n\
            | Ctrl N | New note |\n\
            | Ctrl F | Find in note |\n\
            | Ctrl S | Save now |\n\
            | Ctrl Z | Undo |";

        let reading = "Reading list\n\n\
            - [x] The Pragmatic Programmer\n\
            - [x] A Philosophy of Software Design\n\
            - [ ] The Design of Everyday Things\n\n\
            #books #design";

        let weekend = "Weekend ideas\n\n\
            - Walk the coastal path before the rain\n\
            - Try the new bakery on Mill Street\n\
            - Finish the sketchbook page\n\n\
            A slow Saturday, no alarm.";

        if let Ok(note) = store.create_note(NewNote {
            text: welcome.to_string(),
            created_at: timestamp - 3_600,
            notebook_id: projects.as_ref().map(|notebook| notebook.id),
            tags: vec!["welcome".to_string()],
        }) {
            if let Ok(opened) = store.open_note(&note.id()) {
                let _ = opened.set_pinned(true);
                let _ = opened.set_color(Some("sage"));
                let _ = store.save_note(&opened, timestamp - 3_600);
            }
        }

        if let Ok(note) = store.create_note(NewNote {
            text: reading.to_string(),
            created_at: timestamp - 86_400,
            notebook_id: personal.as_ref().map(|notebook| notebook.id),
            tags: vec!["books".to_string(), "design".to_string()],
        }) {
            if let Ok(opened) = store.open_note(&note.id()) {
                let _ = opened.set_color(Some("sky"));
                let _ = store.save_note(&opened, timestamp - 86_400);
            }
        }

        if let Ok(note) = store.create_note(NewNote {
            text: weekend.to_string(),
            created_at: timestamp - 3_600 * 8,
            notebook_id: personal.as_ref().map(|notebook| notebook.id),
            tags: vec!["personal".to_string()],
        }) {
            if let Ok(opened) = store.open_note(&note.id()) {
                let _ = opened.set_color(Some("amber"));
                let _ = store.save_note(&opened, timestamp - 3_600 * 8);
            }
        }
    }

    /// Re-read the list, filter controls and sidebar counts.
    pub fn reload(&self) {
        let Some(store) = self.store() else {
            return;
        };
        let (view, notebook, tag, search) = {
            let state = self.state.read();
            (
                state.view,
                state.notebook,
                state.tag.clone(),
                state.search.trim().to_string(),
            )
        };

        let query = NoteQuery {
            view,
            notebook_id: notebook,
            tag: tag.clone(),
            limit: 500,
            offset: 0,
        };

        let mut search_hits = None;
        let (notes, snippets) = if search.is_empty() {
            match store.list_notes(&query) {
                Ok(notes) => (notes, Vec::new()),
                Err(error) => {
                    self.toast_danger(format!("Could not list notes: {error}"));
                    (Vec::new(), Vec::new())
                }
            }
        } else {
            match store.search(&SearchQuery {
                text: search,
                view,
                notebook_id: notebook,
                tag,
                limit: 100,
            }) {
                Ok(hits) => {
                    search_hits = Some(hits.len());
                    let snippets = hits
                        .iter()
                        .map(|hit| (hit.note.id, hit.snippet.clone()))
                        .collect();
                    (hits.into_iter().map(|hit| hit.note).collect(), snippets)
                }
                Err(error) => {
                    self.toast_danger(format!("Search failed: {error}"));
                    (Vec::new(), Vec::new())
                }
            }
        };

        let notebooks = store.list_notebooks().unwrap_or_default();
        let tags = store.list_tags().unwrap_or_default();
        let note_count = store.note_count().unwrap_or(0);

        let mut state = self.state.write_unchecked();
        state.notes = notes;
        state.snippets = snippets;
        state.search_hits = search_hits;
        state.notebooks = notebooks;
        state.tags = tags;
        state.note_count = note_count;
        if let Some(notebook) = state.notebook {
            if !state.notebooks.iter().any(|item| item.id == notebook) {
                state.notebook = None;
            }
        }
    }

    /// Update the FTS query. An empty box goes back to the filtered list.
    pub fn set_search(&self, text: String) {
        {
            let mut state = self.state.write_unchecked();
            state.search = text;
            state.mobile_list = true;
            hide_history(&mut state);
        }
        self.reload();
    }

    /// Switch between active, archived and trash.
    pub fn set_view(&self, view: NoteView) {
        {
            let mut state = self.state.write_unchecked();
            state.view = view;
            state.mobile_list = true;
            state.screen = Screen::Notes;
            state.confirm_empty_trash = false;
            hide_history(&mut state);
        }
        self.reload();
    }

    /// Filter the list to one notebook, or clear the filter.
    pub fn filter_notebook(&self, notebook: Option<NotebookId>) {
        {
            let mut state = self.state.write_unchecked();
            state.notebook = notebook;
            state.tag = None;
            state.mobile_list = true;
            state.screen = Screen::Notes;
            hide_history(&mut state);
        }
        self.reload();
    }

    /// Filter the list to one tag, or clear the filter.
    pub fn filter_tag(&self, tag: Option<String>) {
        {
            let mut state = self.state.write_unchecked();
            state.tag = tag;
            state.notebook = None;
            state.mobile_list = true;
            state.screen = Screen::Notes;
            hide_history(&mut state);
        }
        self.reload();
    }

    /// Open a note in the editor, saving the current one first.
    pub fn select_note(&self, id: NoteId) {
        self.save_now();
        let Some(store) = self.store() else {
            return;
        };
        let opened = match store.open_note(&id) {
            Ok(note) => {
                let saved_at = store
                    .note_summary(&id)
                    .map(|summary| summary.updated_at)
                    .unwrap_or_else(|_| now());
                let text = note.text();
                let mut state = self.state.write_unchecked();
                state.selected = Some(id);
                state.draft = text.clone();
                state.saved_text = text;
                state.dirty = false;
                state.saved_at = saved_at;
                state.find_open = false;
                state.find_query.clear();
                state.find_index = 0;
                state.color_open = false;
                state.undo.clear();
                state.redo.clear();
                state.last_undo = None;
                state.mobile_list = false;
                state.editor_mode = EditorMode::Write;
                hide_history(&mut state);
                state.attachments.clear();
                state.attachment_urls.clear();
                true
            }
            Err(error) => {
                self.toast_danger(format!("Could not open note: {error}"));
                false
            }
        };
        if opened {
            self.load_attachments(id);
        }
    }

    /// Create an empty note in the selected notebook and open it.
    pub fn new_note(&self) {
        self.save_now();
        let Some(store) = self.store() else {
            return;
        };
        let (notebook, tag) = {
            let state = self.state.read();
            (state.notebook, state.tag.clone())
        };
        match store.create_note(NewNote {
            text: String::new(),
            created_at: now(),
            notebook_id: notebook,
            tags: tag.into_iter().collect(),
        }) {
            Ok(note) => {
                {
                    let mut state = self.state.write_unchecked();
                    state.search.clear();
                    state.view = NoteView::Active;
                    state.screen = Screen::Notes;
                }
                self.reload();
                self.select_note(note.id());
                self.focus_editor();
                self.toast_info("New note");
            }
            Err(error) => self.toast_danger(format!("Could not create a note: {error}")),
        }
    }

    /// Change the body from the text box.
    pub fn note_input(&self, value: String) {
        let mut state = self.state.write_unchecked();
        if value == state.draft {
            return;
        }
        let should_snapshot = state
            .last_undo
            .map(|last| last.elapsed().as_millis() > 600)
            .unwrap_or(true);
        if should_snapshot {
            let snapshot = state.draft.clone();
            state.undo.push(snapshot);
            if state.undo.len() > 200 {
                state.undo.remove(0);
            }
            state.last_undo = Some(Instant::now());
        }
        state.redo.clear();
        state.dirty = value != state.saved_text;
        state.edit_seq = state.edit_seq.wrapping_add(1);
        state.draft = value;
    }

    /// Write the draft to SQLite now.
    pub fn save_now(&self) {
        let (selected, draft, dirty, saved_text) = {
            let state = self.state.read();
            (
                state.selected,
                state.draft.clone(),
                state.dirty,
                state.saved_text.clone(),
            )
        };
        let Some(id) = selected else {
            return;
        };
        if !dirty && draft == saved_text {
            return;
        }
        // A run may have pulled a newer body for this note. Writing the stale
        // draft now could erase it; hold the edit until the report arrives,
        // then either save it (no remote change) or keep it as a conflict copy.
        {
            let mut runtime = self.runtime.borrow_mut();
            let in_flight = runtime
                .open_base
                .as_ref()
                .is_some_and(|(open, _)| *open == id);
            if runtime.busy && in_flight {
                runtime.deferred_save = Some((id, draft, now()));
                return;
            }
        }
        let Some(store) = self.store() else {
            return;
        };
        let stamp = now();
        let result = (|| -> Result<NoteSummary, String> {
            let note = store
                .open_note(&id)
                .map_err(|error| format!("Could not load the note: {error}"))?;
            if note.text() != draft {
                // Timestamp the edit when the user made it, not when the
                // snapshot happens to be written, so history labels match.
                note.set_next_commit_timestamp(stamp);
                note.replace_text(&draft)
                    .map_err(|error| format!("Could not apply the edit: {error}"))?;
            }
            store
                .save_note(&note, stamp)
                .map_err(|error| format!("Could not save: {error}"))
        })();

        match result {
            Ok(summary) => {
                let mut state = self.state.write_unchecked();
                state.dirty = false;
                state.saved_text = draft.clone();
                state.saved_at = stamp;
                state.seen_seq = state.edit_seq;
                let view = state.view;
                replace_summary(&mut state.notes, summary, view);
                let attachments_changed = attachment_set_changed(&draft, &state.attachments);
                drop(state);
                if attachments_changed {
                    self.load_attachments(id);
                }
            }
            Err(message) => {
                let edit_seq = self.state.peek().edit_seq;
                self.state.write_unchecked().seen_seq = edit_seq;
                self.toast_danger(message);
            }
        }
    }

    /// Refresh the attachment strip and the inline image cache for a note.
    ///
    /// A note can mention a hash whose bytes are not here yet; that row stays
    /// in the strip with a "not downloaded" state. Data URLs are built once per
    /// hash and reused until the note changes.
    fn load_attachments(&self, id: NoteId) {
        let Some(store) = self.store() else {
            return;
        };
        let attachments = match store.note_attachments(&id) {
            Ok(attachments) => attachments,
            Err(error) => {
                self.toast_danger(format!("Could not list attachments: {error}"));
                return;
            }
        };

        let missing: Vec<Attachment> = {
            let state = self.state.read();
            attachments
                .iter()
                .filter(|attachment| {
                    attachment.available
                        && attachment.mime.starts_with("image/")
                        && !state.attachment_urls.contains_key(&attachment.hash)
                })
                .cloned()
                .collect()
        };
        let mut fresh_urls = Vec::with_capacity(missing.len());
        for attachment in missing {
            if let Ok(Some(bytes)) = store.attachment_bytes(&attachment.hash) {
                fresh_urls.push((
                    attachment.hash.clone(),
                    media::image_data_url(&attachment.mime, &bytes),
                ));
            }
        }

        let mut state = self.state.write_unchecked();
        if state.selected != Some(id) {
            return;
        }
        state.attachment_urls.retain(|hash, _| {
            attachments.iter().any(|attachment| {
                &attachment.hash == hash
                    && attachment.available
                    && attachment.mime.starts_with("image/")
            })
        });
        for (hash, url) in fresh_urls {
            state.attachment_urls.insert(hash, url);
        }
        state.attachments = attachments;
    }

    /// Called about once a second: saves after a quiet moment, expires toasts
    /// and refreshes relative times.
    pub fn tick(&self) {
        let save = {
            let mut state = self.state.write_unchecked();
            let save = if !state.dirty {
                false
            } else if state.seen_seq != state.edit_seq {
                state.seen_seq = state.edit_seq;
                false
            } else {
                state.selected.is_some()
            };
            let minute = now() / 60;
            if minute != state.clock_minute {
                state.clock_minute = minute;
            }
            if state
                .toast
                .as_ref()
                .is_some_and(|toast| toast.at.elapsed().as_secs() >= 4)
            {
                state.toast = None;
            }
            save
        };
        if save {
            self.save_now();
        }
        self.poll_background();
        self.persist_if_needed();
        self.maybe_sync();
    }

    /// Toolbar undo.
    pub fn undo(&self) {
        let mut state = self.state.write_unchecked();
        let Some(previous) = state.undo.pop() else {
            drop(state);
            self.toast_info("Nothing to undo");
            return;
        };
        let current = state.draft.clone();
        state.redo.push(current);
        state.draft = previous;
        state.dirty = state.draft != state.saved_text;
        state.edit_seq = state.edit_seq.wrapping_add(1);
        state.last_undo = Some(Instant::now());
    }

    /// Toolbar redo.
    pub fn redo(&self) {
        let mut state = self.state.write_unchecked();
        let Some(next) = state.redo.pop() else {
            drop(state);
            self.toast_info("Nothing to redo");
            return;
        };
        let current = state.draft.clone();
        state.undo.push(current);
        state.draft = next;
        state.dirty = state.draft != state.saved_text;
        state.edit_seq = state.edit_seq.wrapping_add(1);
        state.last_undo = Some(Instant::now());
    }

    /// Run a formatting action around the browser's selection.
    pub fn apply_mark(&self, mark: Mark) {
        if self.state.peek().editor_mode == EditorMode::Preview {
            self.state.write_unchecked().editor_mode = EditorMode::Write;
        }
        let eval = document::eval(
            "const el = document.getElementById('note-editor');\
             return el ? [el.selectionStart, el.selectionEnd] : [0, 0];",
        );
        let context = self.clone();
        spawn(async move {
            let selection = match eval.join::<(usize, usize)>().await {
                Ok(selection) => selection,
                Err(_) => {
                    let length = context.state.peek().draft.encode_utf16().count();
                    (length, length)
                }
            };
            let text = context.state.peek().draft.clone();
            let edit = markdown_ops::apply(&text, selection.0, selection.1, mark);
            context.note_input(edit.text);
            context.restore_selection(edit.selection);
            context.save_now();
        });
    }

    /// Compress picked or dropped files and insert them at the caret.
    ///
    /// File reading is async; compression and the SQLite write are synchronous
    /// on the UI thread for now. A 1600 px photo takes a moment, and the
    /// toolbar shows an "Adding" state while it runs.
    pub fn attach_files(&self, files: Vec<dioxus::html::FileData>) {
        if files.is_empty() {
            return;
        }
        if self.state.peek().selected.is_none() {
            self.toast_info("Open a note before adding an image");
            return;
        }
        let count = files.len();
        self.state.write_unchecked().attaching += count;

        let context = self.clone();
        spawn(async move {
            let mut snippets = Vec::new();
            let mut failures = Vec::new();
            for file in files {
                let name = file.name();
                let bytes = match read_file_bytes(&file).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        failures.push(format!("{name}: {error}"));
                        continue;
                    }
                };
                let image = match images::ingest(&name, &bytes) {
                    Ok(image) => image,
                    Err(error) => {
                        failures.push(format!("{name}: {error}"));
                        continue;
                    }
                };
                let Some(store) = context.store() else {
                    failures.push("storage is not open".to_string());
                    break;
                };
                let stored =
                    match store.put_attachment(&image.name, image.mime, &image.bytes, now()) {
                        Ok(stored) => stored,
                        Err(error) => {
                            failures.push(format!("{name}: could not store it: {error}"));
                            continue;
                        }
                    };
                let url = notes_core::attachment_url(&stored.hash, &stored.mime);
                snippets.push(format!("![{}]({url})", attachment_label(&stored.name)));
            }

            if !snippets.is_empty() {
                let fallback = context.state.peek().draft.encode_utf16().count();
                let (start, end) = selected_range(fallback).await;
                let block = snippets.join("\n\n");
                let text = context.state.peek().draft.clone();
                let edit = markdown_ops::insert_block(&text, start, end, &block);
                context.note_input(edit.text);
                context.restore_selection(edit.selection);
                context.save_now();
                context.toast_success(format!(
                    "Added {}{}",
                    snippets.len(),
                    if snippets.len() == 1 {
                        " image"
                    } else {
                        " images"
                    }
                ));
            }
            if !failures.is_empty() {
                let message = if failures.len() == 1 {
                    failures.remove(0)
                } else {
                    format!(
                        "{} images could not be added. {}",
                        failures.len(),
                        failures.remove(0)
                    )
                };
                context.toast_danger(message);
            }

            let mut state = context.state.write_unchecked();
            state.attaching = state.attaching.saturating_sub(count);
        });
    }

    /// Insert an existing attachment's Markdown at the caret.
    pub fn insert_attachment(&self, attachment: Attachment) {
        if !attachment.available {
            self.toast_info("That attachment has not downloaded yet");
            return;
        }
        let block = format!(
            "![{}]({})",
            attachment_label(&attachment.name),
            notes_core::attachment_url(&attachment.hash, &attachment.mime)
        );
        let context = self.clone();
        spawn(async move {
            let fallback = context.state.peek().draft.encode_utf16().count();
            let (start, end) = selected_range(fallback).await;
            let text = context.state.peek().draft.clone();
            let edit = markdown_ops::insert_block(&text, start, end, &block);
            context.note_input(edit.text);
            context.restore_selection(edit.selection);
            context.save_now();
        });
    }

    /// Put the caret after a snippet inserted into the open note.
    fn restore_selection(&self, selection: (usize, usize)) {
        let (start, end) = selection;
        let _ = document::eval(&format!(
            "setTimeout(() => {{\
                const el = document.getElementById('note-editor');\
                if (!el) return;\
                el.focus();\
                el.setSelectionRange({start}, {end});\
             }}, 20);"
        ));
    }

    /// Focus the editor after creating a note.
    pub fn focus_editor(&self) {
        let _ = document::eval(
            "setTimeout(() => document.getElementById('note-editor')?.focus(), 30);",
        );
    }

    /// Open or close the find bar.
    pub fn toggle_find(&self) {
        let open = !self.state.peek().find_open;
        {
            let mut state = self.state.write_unchecked();
            state.find_open = open;
            if open {
                state.editor_mode = EditorMode::Write;
            } else {
                state.find_query.clear();
                state.find_index = 0;
            }
        }
        if open {
            self.focus_find();
        }
    }

    /// Put the caret in the find box after it appears.
    fn focus_find(&self) {
        let _ = document::eval(
            "setTimeout(() => { const el = document.getElementById('find-input'); if (el) { el.focus(); el.select(); } }, 30);",
        );
    }

    /// Update the find query and select the first match.
    pub fn set_find_query(&self, value: String) {
        {
            let mut state = self.state.write_unchecked();
            state.find_query = value;
            state.find_index = 0;
        }
        self.find_select(0);
    }

    /// Move to the next (`1`) or previous (`-1`) match.
    pub fn find_next(&self, direction: isize) {
        let (query, current) = {
            let state = self.state.read();
            (state.find_query.clone(), state.find_index)
        };
        let count = find_match_count(&self.state.peek().draft, &query);
        if count == 0 {
            return;
        }
        let next = if direction >= 0 {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        self.state.write_unchecked().find_index = next;
        self.find_select(next);
        self.focus_editor();
    }

    fn find_select(&self, index: usize) {
        let query = self.state.peek().find_query.clone();
        if query.is_empty() {
            return;
        }
        let needle = serde_json::to_string(&query).unwrap_or_else(|_| "\"\"".to_string());
        let _ = document::eval(&format!(
            "const el = document.getElementById('note-editor');\
             if (!el) return;\
             const needle = {needle}.toLowerCase();\
             const hay = el.value.toLowerCase();\
             let index = -1, from = 0;\
             for (let i = 0; i <= {index}; i++) {{\
                index = hay.indexOf(needle, from);\
                if (index < 0) break;\
                from = index + needle.length;\
             }}\
             if (index >= 0) {{\
                el.setSelectionRange(index, index + needle.length);\
             }}"
        ));
    }

    /// Toggle pin on the open note.
    pub fn toggle_pin(&self) {
        self.mutate_selected(|note| {
            let pinned = note.meta().pinned;
            note.set_pinned(!pinned)?;
            Ok(true)
        });
    }

    /// Toggle archive on the open note.
    pub fn toggle_archive(&self) {
        self.mutate_selected(|note| {
            let archived = note.meta().archived;
            note.set_archived(!archived)?;
            Ok(true)
        });
    }

    /// Set or clear the card colour.
    pub fn set_color(&self, color: Option<&str>) {
        let color = color.map(str::to_string);
        self.mutate_selected(move |note| {
            note.set_color(color.as_deref())?;
            Ok(true)
        });
        self.state.write_unchecked().color_open = false;
    }

    /// Move the open note to a notebook, or out of every notebook.
    pub fn set_notebook(&self, notebook_id: Option<NotebookId>) {
        self.mutate_selected(move |note| {
            note.set_notebook(notebook_id)?;
            Ok(true)
        });
    }

    /// Add a tag to the open note.
    pub fn add_tag(&self, tag: String) {
        let tag = tag.trim().to_string();
        if tag.is_empty() {
            return;
        }
        self.mutate_selected(move |note| note.add_tag(&tag));
    }

    /// Remove a tag from the open note.
    pub fn remove_tag(&self, tag: String) {
        self.mutate_selected(move |note| note.remove_tag(&tag));
    }

    /// Move the open note to the trash and show the next note.
    pub fn trash_selected(&self) {
        self.save_now();
        let Some(id) = self.state.peek().selected else {
            return;
        };
        let Some(store) = self.store() else {
            return;
        };
        match store.trash_note(&id, now()) {
            Ok(true) => {
                self.reload();
                let next = self.state.peek().notes.first().map(|note| note.id);
                match next {
                    Some(next) => self.select_note(next),
                    None => self.clear_selection(),
                }
                self.toast_info("Moved to trash");
            }
            Ok(false) => self.toast_info("Note is already in the trash"),
            Err(error) => self.toast_danger(format!("Could not move the note: {error}")),
        }
    }

    /// Bring a trashed note back.
    pub fn restore_note(&self, id: NoteId) {
        let Some(store) = self.store() else {
            return;
        };
        match store.restore_note(&id, now()) {
            Ok(true) => {
                {
                    let mut state = self.state.write_unchecked();
                    // Restore lands in the normal notes view so the note is
                    // visible and editable again, not just open behind the
                    // trash list.
                    state.view = NoteView::Active;
                    state.tag = None;
                    state.notebook = None;
                    state.search.clear();
                    state.search_hits = None;
                    state.snippets.clear();
                    state.confirm_empty_trash = false;
                    state.mobile_list = false;
                    hide_history(&mut state);
                }
                self.reload();
                self.select_note(id);
                self.toast_success("Note restored");
            }
            Ok(false) => {}
            Err(error) => self.toast_danger(format!("Could not restore the note: {error}")),
        }
    }

    /// Empty the trash after the Settings confirmation.
    pub fn empty_trash(&self) {
        let Some(store) = self.store() else {
            return;
        };
        let selected = self.state.peek().selected;
        match store.empty_trash() {
            Ok(count) => {
                let selected_was_erased = selected
                    .map(|id| store.note_summary(&id).is_err())
                    .unwrap_or(false);
                {
                    let mut state = self.state.write_unchecked();
                    state.confirm_empty_trash = false;
                    hide_history(&mut state);
                }
                self.reload();
                if selected_was_erased {
                    self.clear_selection();
                    let next = self.state.peek().notes.first().map(|note| note.id);
                    if let Some(next) = next {
                        self.select_note(next);
                    }
                }
                self.toast_success(format!(
                    "{count} note{} erased",
                    if count == 1 { "" } else { "s" }
                ));
            }
            Err(error) => self.toast_danger(format!("Could not empty the trash: {error}")),
        }
    }

    fn clear_selection(&self) {
        let mut state = self.state.write_unchecked();
        state.selected = None;
        state.draft.clear();
        state.saved_text.clear();
        state.dirty = false;
        state.undo.clear();
        state.redo.clear();
        state.mobile_list = true;
        hide_history(&mut state);
    }

    fn mutate_selected(&self, edit: impl FnOnce(&Note) -> Result<bool, notes_core::NoteError>) {
        self.save_now();
        let Some(id) = self.state.peek().selected else {
            return;
        };
        let Some(store) = self.store() else {
            return;
        };
        let result = (|| -> Result<bool, String> {
            let note = store
                .open_note(&id)
                .map_err(|error| format!("Could not load the note: {error}"))?;
            let changed = edit(&note).map_err(|error| format!("Could not update: {error}"))?;
            if changed {
                note.commit_at(now());
                store
                    .save_note(&note, now())
                    .map_err(|error| format!("Could not save: {error}"))?;
            }
            Ok(changed)
        })();
        match result {
            Ok(changed) => {
                if changed {
                    self.reload();
                }
            }
            Err(message) => self.toast_danger(message),
        }
    }

    /// Create a notebook and clear the inline field.
    pub fn create_notebook(&self, name: String) {
        let Some(store) = self.store() else {
            return;
        };
        match store.create_notebook(name.trim(), now()) {
            Ok(notebook) => {
                self.state.write_unchecked().new_notebook = None;
                self.reload();
                self.toast_success(format!(
                    "Notebook \u{201c}{}\u{201d} created",
                    notebook.name
                ));
            }
            Err(error) => self.toast_danger(format!("Could not create the notebook: {error}")),
        }
    }

    /// Rename a notebook from Settings.
    pub fn rename_notebook(&self, id: NotebookId, name: String) {
        let Some(store) = self.store() else {
            return;
        };
        match store.rename_notebook(&id, name.trim()) {
            Ok(true) => self.reload(),
            Ok(false) => self.toast_info("That notebook no longer exists"),
            Err(error) => self.toast_danger(format!("Could not rename the notebook: {error}")),
        }
    }

    /// Remove a notebook from the workspace. Its notes are kept.
    pub fn delete_notebook(&self, id: NotebookId) {
        let Some(store) = self.store() else {
            return;
        };
        match store.delete_notebook(&id) {
            Ok(true) => {
                if self.state.peek().notebook == Some(id) {
                    self.state.write_unchecked().notebook = None;
                }
                self.reload();
                self.toast_info("Notebook removed. Its notes were kept.");
            }
            Ok(false) => {}
            Err(error) => self.toast_danger(format!("Could not remove the notebook: {error}")),
        }
    }

    /// Switch to a screen.
    pub fn go(&self, screen: Screen) {
        self.save_now();
        // The login form starts a new device session. Replacing a live one
        // without signing out would leave the old device listed forever.
        if screen == Screen::Login && self.runtime.borrow().client.is_some() {
            self.toast_info("Sign out first to use another account.");
            self.state.write_unchecked().screen = Screen::Settings;
            return;
        }
        let mut state = self.state.write_unchecked();
        state.screen = screen;
        state.palette_open = false;
        state.shortcuts_open = false;
        state.sidebar_open = false;
        hide_history(&mut state);
        if screen == Screen::Notes {
            state.mobile_list = state.selected.is_none();
        }
        if screen == Screen::Login {
            state.login.error = None;
        }
    }

    /// Cycle the topbar theme button.
    pub fn toggle_theme(&self) {
        let next = match self.state.peek().theme {
            Theme::System | Theme::Dark => Theme::Light,
            Theme::Light => Theme::Dark,
        };
        self.state.write_unchecked().theme = next;
    }

    /// Pick a specific theme.
    pub fn set_theme(&self, theme: Theme) {
        self.state.write_unchecked().theme = theme;
    }

    /// Switch between cards and list.
    pub fn toggle_layout(&self) {
        let mut state = self.state.write_unchecked();
        state.layout = match state.layout {
            Layout::Grid => Layout::List,
            Layout::List => Layout::Grid,
        };
    }

    /// Set the card layout from Settings.
    pub fn set_layout(&self, layout: Layout) {
        self.state.write_unchecked().layout = layout;
    }

    /// Source or preview.
    pub fn toggle_preview(&self) {
        let writing = {
            let mut state = self.state.write_unchecked();
            state.editor_mode = match state.editor_mode {
                EditorMode::Write => EditorMode::Preview,
                EditorMode::Preview => EditorMode::Write,
            };
            state.editor_mode == EditorMode::Write
        };
        if writing {
            self.focus_editor();
        }
    }

    /// Load the open note's versions and show the history panel.
    pub fn open_history(&self) {
        // Capture the text box first: the panel shows saved versions, not a
        // draft that only exists in the DOM.
        self.save_now();
        let Some(note_id) = self.state.peek().selected else {
            self.toast_info("Open a note to see its history");
            return;
        };
        let Some(store) = self.store() else {
            return;
        };
        let loaded = (|| -> Result<(Vec<NoteVersion>, u64, Option<String>), String> {
            let note = store
                .open_note(&note_id)
                .map_err(|error| format!("Could not load the note: {error}"))?;
            let versions = note
                .history()
                .map_err(|error| format!("Could not read the history: {error}"))?;
            // Only the newest version can be "Current"; a merge of concurrent
            // branches may match none, and then no row gets the pill.
            let current_text = note.text();
            let current = versions
                .iter()
                .max_by_key(|version| version.ended_at)
                .filter(|version| {
                    note.text_at(version)
                        .map(|text| text == current_text)
                        .unwrap_or(false)
                })
                .map(|version| version.id.clone());
            Ok((versions, note.peer_id(), current))
        })();

        let (versions, local_peer, current) = match loaded {
            Ok(loaded) => loaded,
            Err(message) => {
                self.toast_danger(message);
                return;
            }
        };

        // Start on the version before the current one when there is one, so
        // the panel opens on a visible change instead of an empty diff.
        let selected = versions
            .iter()
            .filter(|version| Some(&version.id) != current.as_ref())
            .max_by_key(|version| version.ended_at)
            .or_else(|| versions.iter().max_by_key(|version| version.ended_at))
            .map(|version| version.id.clone());

        {
            let mut state = self.state.write_unchecked();
            state.history_open = true;
            state.history_versions = versions;
            state.history_selected = selected.clone();
            state.history_current = current;
            state.history_local_peer = local_peer;
            state.history_mode = HistoryMode::Changes;
            state.history_preview.clear();
            state.history_diff.clear();
            state.history_error = None;
            state.find_open = false;
            state.color_open = false;
        }
        if let Some(version_id) = selected {
            self.load_history_version(&version_id);
        }
    }

    /// Close the history panel.
    pub fn close_history(&self) {
        hide_history(&mut self.state.write_unchecked());
    }

    /// Switch the detail between the diff and the read-only preview.
    pub fn set_history_mode(&self, mode: HistoryMode) {
        self.state.write_unchecked().history_mode = mode;
    }

    /// Show one version in the detail pane.
    pub fn select_history_version(&self, version_id: &str) {
        {
            let mut state = self.state.write_unchecked();
            if state.history_selected.as_deref() == Some(version_id) {
                // A second click is harmless; the detail body scrolls rather
                // than reloading.
                if !state.history_preview.is_empty() || !state.history_diff.is_empty() {
                    return;
                }
            }
            state.history_selected = Some(version_id.to_string());
            state.history_error = None;
        }
        self.load_history_version(version_id);
    }

    /// Compute the preview and diff for a version already in the panel.
    fn load_history_version(&self, version_id: &str) {
        let version = {
            let state = self.state.read();
            state
                .history_versions
                .iter()
                .find(|version| version.id == version_id)
                .cloned()
        };
        let Some(version) = version else {
            return;
        };
        let Some(note_id) = self.state.peek().selected else {
            return;
        };
        let Some(store) = self.store() else {
            return;
        };
        let loaded = (|| -> Result<(String, String), String> {
            let note = store
                .open_note(&note_id)
                .map_err(|error| format!("Could not load the note: {error}"))?;
            let preview = note
                .text_at(&version)
                .map_err(|error| format!("Could not read that version: {error}"))?;
            let diff = note
                .diff_from_current(&version)
                .map_err(|error| format!("Could not compare that version: {error}"))?;
            Ok((preview, diff))
        })();

        let mut state = self.state.write_unchecked();
        match loaded {
            Ok((preview, diff)) => {
                state.history_preview = preview;
                state.history_diff = diff;
                state.history_error = None;
            }
            Err(message) => {
                state.history_preview.clear();
                state.history_diff.clear();
                state.history_error = Some(message);
            }
        }
    }

    /// Restore a version's body as a new edit. History keeps both versions.
    pub fn restore_history_version(&self, version_id: &str) {
        self.save_now();
        let (note_id, version) = {
            let state = self.state.read();
            let Some(note_id) = state.selected else {
                return;
            };
            let Some(version) = state
                .history_versions
                .iter()
                .find(|version| version.id == version_id)
                .cloned()
            else {
                return;
            };
            (note_id, version)
        };
        let Some(store) = self.store() else {
            return;
        };
        let stamp = now();
        let result = (|| -> Result<bool, String> {
            let note = store
                .open_note(&note_id)
                .map_err(|error| format!("Could not load the note: {error}"))?;
            note.set_next_commit_timestamp(stamp);
            let changed = note
                .restore_version(&version)
                .map_err(|error| format!("Could not restore that version: {error}"))?;
            if changed {
                store
                    .save_note(&note, stamp)
                    .map_err(|error| format!("Could not save the restored note: {error}"))?;
            }
            Ok(changed)
        })();

        match result {
            Ok(true) => {
                self.close_history();
                self.reload();
                self.select_note(note_id);
                self.toast_success("Version restored. The old edit stays in history.");
            }
            Ok(false) => self.toast_info("That version matches the current note"),
            Err(message) => self.toast_danger(message),
        }
    }

    /// Open Settings and refresh the device list when connected.
    pub fn open_settings(&self) {
        self.go(Screen::Settings);
        if self.state.peek().profile.sync_connected {
            self.refresh_devices();
        }
    }

    /// Close overlays, or leave the editor on a phone.
    pub fn escape(&self) {
        let mut state = self.state.write_unchecked();
        if state.palette_open {
            state.palette_open = false;
        } else if state.shortcuts_open {
            state.shortcuts_open = false;
        } else if state.find_open {
            state.find_open = false;
        } else if state.sidebar_open {
            state.sidebar_open = false;
        } else if state.color_open {
            state.color_open = false;
        } else if state.history_open {
            hide_history(&mut state);
        } else if state.screen == Screen::Settings || state.screen == Screen::Login {
            state.screen = Screen::Notes;
            state.mobile_list = state.selected.is_none();
        } else if !state.mobile_list {
            state.mobile_list = true;
        }
    }

    /// Open the command palette.
    pub fn open_palette(&self) {
        {
            let mut state = self.state.write_unchecked();
            state.palette_open = true;
            state.palette_query.clear();
            state.palette_index = 0;
        }
        self.focus_palette();
    }

    /// Put the caret in the palette after it appears.
    fn focus_palette(&self) {
        let _ = document::eval(
            "setTimeout(() => { const el = document.getElementById('palette-input'); if (el) { el.focus(); } }, 30);",
        );
    }

    /// Close the command palette.
    pub fn close_palette(&self) {
        self.state.write_unchecked().palette_open = false;
    }

    /// Filter palette rows.
    pub fn set_palette_query(&self, value: String) {
        let mut state = self.state.write_unchecked();
        state.palette_query = value;
        state.palette_index = 0;
    }

    /// Highlight a palette row.
    pub fn set_palette_index(&self, index: usize) {
        self.state.write_unchecked().palette_index = index;
    }

    /// Show the keyboard sheet.
    pub fn show_shortcuts(&self) {
        let mut state = self.state.write_unchecked();
        state.shortcuts_open = true;
        state.palette_open = false;
    }

    /// Submit the login form and open a real session.
    pub fn submit_login(&self) {
        if self.runtime.borrow().busy {
            self.toast_info("Wait for the current sync to finish.");
            return;
        }
        let (username, passphrase, server, device) = {
            let state = self.state.read();
            (
                state.login.username.trim().to_string(),
                state.login.passphrase.clone(),
                state.login.server.trim().trim_end_matches('/').to_string(),
                state.login.device.trim().to_string(),
            )
        };
        if username.is_empty() || passphrase.is_empty() || server.is_empty() || device.is_empty() {
            self.state.write_unchecked().login.error =
                Some("Enter a username, passphrase, server URL and device name.".to_string());
            return;
        }
        {
            let mut state = self.state.write_unchecked();
            state.login.passphrase.clear();
            state.login.error = None;
            // Keep the form on screen while connecting so the button and the
            // error live in one place.
            state.sync.status = SyncStatus::Connecting;
            state.sync.error = None;
        }
        self.runtime.borrow_mut().busy = true;
        self.start_login(server, username, device, passphrase);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_login(&self, server: String, username: String, device: String, passphrase: String) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("notebox-login".to_string())
            .spawn(move || {
                let result = sync::login_blocking(&server, &username, &device, &passphrase)
                    .map_err(|error| sync::friendly_login_error(&error));
                let _ = sender.send(result);
            });
        match spawned {
            Ok(_) => self.runtime.borrow_mut().login_rx = Some(receiver),
            Err(error) => self.login_failed(format!("Could not start the login thread: {error}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_login(&self, server: String, username: String, device: String, passphrase: String) {
        let client = match sync::client(&server, &username, &device, None) {
            Ok(client) => client,
            Err(error) => {
                self.login_failed(sync::friendly_login_error(&error));
                return;
            }
        };
        let context = self.clone();
        let task_client = client.clone();
        // This task outlives the login screen: finish_login replaces it with
        // Notes, and a scope-bound task would be cancelled mid-login.
        spawn_forever(async move {
            let sessions = notes_sync::NullTokenStore;
            match task_client.login(&passphrase, &sessions).await {
                Ok(session) => context.finish_login(
                    sync::LoginSuccess {
                        session,
                        account: sync::SavedAccount {
                            server,
                            username,
                            device,
                        },
                    },
                    Some(client),
                ),
                Err(error) => context.login_failed(sync::friendly_login_error(&error)),
            }
        });
    }

    /// Install a successful login and start the first sync.
    fn finish_login(
        &self,
        success: sync::LoginSuccess,
        provided: Option<Rc<notes_sync::SyncClient<BoxedTransport>>>,
    ) {
        let client = match provided {
            Some(client) => client,
            None => match sync::client(
                &success.account.server,
                &success.account.username,
                &success.account.device,
                Some(success.session.clone()),
            ) {
                Ok(client) => client,
                Err(error) => {
                    self.login_failed(sync::friendly_login_error(&error));
                    return;
                }
            },
        };
        if let Err(error) = sync::save_account(&success.account) {
            self.toast_danger(format!(
                "Signed in, but this device could not remember the account: {error}"
            ));
        }
        {
            let mut runtime = self.runtime.borrow_mut();
            runtime.client = Some(client);
            runtime.account = Some(success.account.clone());
            runtime.busy = false;
            runtime.failures = 0;
            runtime.retry_after = None;
            runtime.stopped = false;
            runtime.focus_pending = true;
        }
        {
            let mut state = self.state.write_unchecked();
            state.profile.username = success.account.username.clone();
            state.profile.sync_connected = true;
            state.login.server = success.account.server.clone();
            state.login.username = success.account.username.clone();
            state.login.device = success.account.device.clone();
            state.login.passphrase.clear();
            state.login.error = None;
            state.sync.status = SyncStatus::Idle;
            state.sync.error = None;
            state.sync.devices.clear();
            state.sync.devices_error = None;
            state.sync.confirm_revoke = None;
            state.screen = Screen::Notes;
            state.mobile_list = state.selected.is_none();
        }
        self.toast_success(format!("Connected as {}", success.account.username));
        // Do not show “Synced” before anything has been synced. Starting the
        // first run here also removes the tick delay after login.
        self.start_sync(false);
    }

    fn login_failed(&self, message: String) {
        {
            let mut runtime = self.runtime.borrow_mut();
            runtime.busy = false;
            runtime.client = None;
            runtime.account = None;
            runtime.failures = 0;
            runtime.retry_after = None;
            runtime.stopped = false;
        }
        let mut state = self.state.write_unchecked();
        state.profile.sync_connected = false;
        state.login.passphrase.clear();
        state.login.error = Some(message.clone());
        state.sync.status = SyncStatus::Local;
        state.sync.error = Some(message);
    }

    /// Continue without an account.
    pub fn continue_local(&self) {
        {
            let mut state = self.state.write_unchecked();
            state.login.passphrase.clear();
            state.login.error = None;
            state.profile.sync_connected = false;
            state.sync.status = SyncStatus::Local;
            state.screen = Screen::Notes;
            state.mobile_list = state.selected.is_none();
        }
        self.toast_info("Your notes stay on this device.");
    }

    /// Ask for a sync now.
    pub fn sync_now(&self) {
        self.start_sync(true);
    }

    /// Save the editor, then run the engine on a thread (native) or spawned
    /// future (web).
    fn start_sync(&self, manual: bool) {
        if self.store().is_none() {
            return;
        }
        let (client, connected) = {
            let runtime = self.runtime.borrow();
            (runtime.client.clone(), runtime.account.is_some())
        };
        let Some(client) = client.filter(|_| connected) else {
            if manual {
                self.toast_info("Sign in to sync your notes.");
            }
            return;
        };
        if self.runtime.borrow().busy {
            if manual {
                self.toast_info("Sync is already running");
            }
            return;
        }
        // The draft must be in SQLite before the merge runs; otherwise the
        // engine would pull a body the editor never contributed.
        self.save_now();
        let (dirty, selected, saved_text) = {
            let state = self.state.peek();
            (state.dirty, state.selected, state.saved_text.clone())
        };
        if dirty {
            if manual {
                self.toast_danger("The note could not be saved, so sync did not start.");
            }
            return;
        }
        let started = now();
        let open_base = selected.map(|id| (id, saved_text));
        {
            let mut runtime = self.runtime.borrow_mut();
            runtime.busy = true;
            runtime.manual_run = manual;
            runtime.last_attempt = started;
            // A manual Sync now is the way out of a permanent error.
            runtime.stopped = false;
            runtime.focus_pending = false;
            runtime.open_base = open_base.clone();
            runtime.deferred_save = None;
        }
        self.state.write_unchecked().sync.status = SyncStatus::Syncing;

        #[cfg(not(target_arch = "wasm32"))]
        {
            let account = self.runtime.borrow().account.clone();
            let session = client.session();
            let (Some(account), Some(session)) = (account, session) else {
                self.apply_sync_failure(SyncFailure {
                    message: "The device session is missing. Sign in again.".to_string(),
                    unauthorized: true,
                    retryable: false,
                    retry_after: None,
                });
                return;
            };
            let job = sync::NativeSyncJob {
                database: storage::database_path(),
                account,
                session,
                open_note: open_base.as_ref().map(|(id, _)| *id),
                started_at: started,
            };
            let (sender, receiver) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("notebox-sync".to_string())
                .spawn(move || {
                    let report = sync::sync_blocking(job).map_err(sync::SyncFailure::from);
                    let _ = sender.send(report);
                });
            match spawned {
                Ok(_) => self.runtime.borrow_mut().sync_rx = Some(receiver),
                Err(error) => self.apply_sync_failure(SyncFailure {
                    message: format!("Could not start the sync thread: {error}"),
                    unauthorized: false,
                    retryable: true,
                    retry_after: None,
                }),
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let Some(store) = self.store_rc() else {
                return;
            };
            let context = self.clone();
            let open_note = open_base.as_ref().map(|(id, _)| *id);
            // A sync started from Settings must survive leaving Settings; the
            // status would otherwise stay "Syncing" forever.
            spawn_forever(async move {
                let result = async {
                    let outcome = client.sync(store.as_ref(), started).await?;
                    let mut attachments = notes_sync::AttachmentSyncOutcome::default();
                    let mut attachment_error = None;
                    match store.notes_with_attachment_work(sync::ATTACHMENT_NOTES_PER_RUN) {
                        Ok(notes) => {
                            for note in notes {
                                match client
                                    .sync_note_attachments(store.as_ref(), &note, started)
                                    .await
                                {
                                    Ok(pass) => {
                                        attachments.uploaded += pass.uploaded;
                                        attachments.downloaded += pass.downloaded;
                                        attachments.missing += pass.missing;
                                    }
                                    Err(error) if error.is_unauthorized() => return Err(error),
                                    Err(error) => {
                                        attachment_error = Some(sync::friendly_sync_error(&error));
                                        break;
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            attachment_error =
                                Some(format!("Could not read the attachment queue: {error}"));
                        }
                    }
                    let open_note = open_note
                        .and_then(|id| store.open_note(&id).ok())
                        .map(|note| note.text());
                    Ok::<_, notes_sync::SyncError>(SyncReport {
                        outcome,
                        attachments,
                        attachment_error,
                        open_note,
                        finished_at: now(),
                    })
                }
                .await;
                match result {
                    Ok(report) => context.apply_sync_report(report),
                    Err(error) => context.apply_sync_failure(SyncFailure::from(error)),
                }
            });
        }
    }

    /// Poll the result channels used by the native worker threads.
    fn poll_background(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_native();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn poll_native(&self) {
        use std::sync::mpsc::TryRecvError;

        let login = {
            let mut runtime = self.runtime.borrow_mut();
            match runtime.login_rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(value)) => {
                    runtime.login_rx = None;
                    Some(value)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    runtime.login_rx = None;
                    Some(Err(
                        "The login thread stopped without an answer.".to_string()
                    ))
                }
                Some(Err(TryRecvError::Empty)) | None => None,
            }
        };
        if let Some(result) = login {
            match result {
                Ok(success) => self.finish_login(success, None),
                Err(message) => self.login_failed(message),
            }
        }

        let run = {
            let mut runtime = self.runtime.borrow_mut();
            match runtime.sync_rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(value)) => {
                    runtime.sync_rx = None;
                    Some(value)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    runtime.sync_rx = None;
                    Some(Err(SyncFailure {
                        message: "The sync thread stopped without an answer.".to_string(),
                        unauthorized: false,
                        retryable: true,
                        retry_after: None,
                    }))
                }
                Some(Err(TryRecvError::Empty)) | None => None,
            }
        };
        if let Some(result) = run {
            match result {
                Ok(report) => self.apply_sync_report(report),
                Err(failure) => self.apply_sync_failure(failure),
            }
        }

        let devices = {
            let mut runtime = self.runtime.borrow_mut();
            match runtime.devices_rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(value)) => {
                    runtime.devices_rx = None;
                    Some(value)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    runtime.devices_rx = None;
                    Some(Err(
                        "The device thread stopped without an answer.".to_string()
                    ))
                }
                Some(Err(TryRecvError::Empty)) | None => None,
            }
        };
        if let Some(result) = devices {
            self.finish_devices(result);
        }
    }

    /// PLAN.md's loop: start, three seconds after an edit, every minute while
    /// visible, and on focus. Never while hidden or dirty.
    fn maybe_sync(&self) {
        let visible = sync::document_visible();
        let focused = sync::document_focused();
        let (connected, busy, last_attempt, failures, retry_after, stopped, focus) = {
            let mut runtime = self.runtime.borrow_mut();
            if focused && !runtime.was_focused {
                runtime.focus_pending = true;
            }
            runtime.was_focused = focused;
            (
                runtime.client.is_some() && runtime.account.is_some(),
                runtime.busy,
                runtime.last_attempt,
                runtime.failures,
                runtime.retry_after,
                runtime.stopped,
                runtime.focus_pending,
            )
        };
        let (dirty, last_synced, last_saved) = {
            let state = self.state.peek();
            (
                state.dirty,
                state.sync.last_synced.unwrap_or(0),
                state.saved_at,
            )
        };
        let due = Due {
            connected,
            busy,
            visible,
            dirty,
            last_synced,
            last_saved,
            last_attempt,
            failures,
            retry_after,
            stopped,
            focus,
        };
        if sync::is_due(now(), &due) {
            self.start_sync(false);
        }
    }

    /// Install a successful run and reconcile the editor.
    fn apply_sync_report(&self, report: SyncReport) {
        let (base, deferred, manual) = {
            let mut runtime = self.runtime.borrow_mut();
            runtime.busy = false;
            runtime.failures = 0;
            runtime.retry_after = None;
            runtime.stopped = false;
            let manual = runtime.manual_run;
            runtime.manual_run = false;
            (
                runtime.open_base.take(),
                runtime.deferred_save.take(),
                manual,
            )
        };
        {
            let mut state = self.state.write_unchecked();
            state.sync.status = SyncStatus::Idle;
            state.sync.last_synced = Some(report.finished_at);
            state.sync.error = report.attachment_error.clone();
            state.profile.sync_connected = true;
        }
        if report.outcome.pulled > 0 {
            self.reload();
        }
        // Raise the summary before resolving the open note: a reload or a
        // conflict has a more useful message and must not be hidden by it.
        if manual {
            match &report.attachment_error {
                Some(error) => self.toast_danger(error.clone()),
                None => self.toast_success(sync::run_summary(&report)),
            }
        }
        let base_id = base.as_ref().map(|(id, _)| *id);
        if let Some((id, base_text)) = base {
            self.resolve_open_note(id, &base_text, &report, deferred);
        } else if let Some((id, text, at)) = deferred {
            // No base means the run and the editor got out of step; still keep
            // the unsigned edit rather than dropping it.
            let current = self
                .store()
                .and_then(|store| store.open_note(&id).ok())
                .map(|note| note.text())
                .unwrap_or_default();
            if text != current {
                let _ = self.keep_conflict_copy(id, &text, at);
            }
        }
        // The user may have switched to another note while the run was in
        // flight. Its merged body is newer than what the editor read.
        self.refresh_selected_after_run(base_id, report.finished_at);
        if report.attachments.downloaded > 0 {
            if let Some(id) = self.state.peek().selected {
                self.load_attachments(id);
            }
        }
        if self.state.peek().history_open && report.outcome.pulled > 0 {
            self.open_history();
        }
    }

    /// Compare the merged body with what the editor holds and never overwrite
    /// a local edit.
    fn resolve_open_note(
        &self,
        id: NoteId,
        base: &str,
        report: &SyncReport,
        deferred: Option<(NoteId, String, i64)>,
    ) {
        let live = self.state.peek().selected == Some(id);
        let (ours, at) = if live {
            (self.state.peek().draft.clone(), report.finished_at)
        } else if let Some((_, text, at)) = deferred.filter(|(note, _, _)| *note == id) {
            (text, at)
        } else {
            (base.to_string(), report.finished_at)
        };
        let Some(remote) = report.open_note.as_deref() else {
            if live && ours != base {
                self.state.write_unchecked().sync.error =
                    Some("Could not compare the open note after sync.".to_string());
            }
            return;
        };
        match sync::plan_open_note(base, &ours, remote) {
            OpenNotePlan::Unchanged => {
                if !live && ours != base {
                    self.save_detached(id, ours, at);
                }
            }
            OpenNotePlan::Reload(merged) => {
                if live {
                    self.adopt_open_note(id, &merged, report.finished_at);
                    self.toast_info("Updated from another device");
                }
            }
            OpenNotePlan::Conflict { keep, remote } => {
                let copy = self.keep_conflict_copy(id, &keep, at);
                if live {
                    self.adopt_open_note(id, &remote, report.finished_at);
                }
                match copy {
                    Some(title) => self.toast_info(format!(
                        "Remote changes arrived while you were typing. Your version is in \"{title}\"."
                    )),
                    None => self.toast_danger(
                        "Remote changes arrived and the local copy could not be saved. Your text is still in the editor.",
                    ),
                }
            }
        }
    }

    /// Show a merged body for a note the user switched to during the run.
    ///
    /// Only a clean editor is refreshed; a dirty one keeps the user's text and
    /// the next save merges it, so nothing typed is thrown away.
    fn refresh_selected_after_run(&self, base: Option<NoteId>, at: i64) {
        let Some(store) = self.store() else {
            return;
        };
        let (selected, draft, saved_text, dirty) = {
            let state = self.state.peek();
            (
                state.selected,
                state.draft.clone(),
                state.saved_text.clone(),
                state.dirty,
            )
        };
        let Some(selected) = selected else {
            return;
        };
        if base == Some(selected) || dirty || draft != saved_text {
            return;
        }
        if let Ok(note) = store.open_note(&selected) {
            let text = note.text();
            if text != draft {
                self.adopt_open_note(selected, &text, at);
            }
        }
    }

    /// Put the merged body into the editor without saving the stale draft.
    fn adopt_open_note(&self, id: NoteId, text: &str, at: i64) {
        {
            let mut state = self.state.write_unchecked();
            if state.selected != Some(id) {
                return;
            }
            state.draft = text.to_string();
            state.saved_text = text.to_string();
            state.dirty = false;
            state.saved_at = at;
            state.seen_seq = state.edit_seq;
            state.undo.clear();
            state.redo.clear();
            hide_history(&mut state);
        }
        self.load_attachments(id);
    }

    /// Keep a local body as a new note tagged `conflict`, under the merged one.
    fn keep_conflict_copy(&self, original: NoteId, text: &str, at: i64) -> Option<String> {
        let store = self.store()?;
        let meta = store.note_summary(&original).ok();
        let original_title = meta
            .as_ref()
            .map(|summary| summary.title.trim().to_string())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "untitled note".to_string());
        let body = format!("Conflict copy of {original_title}\n\n{text}");
        let mut tags = meta
            .as_ref()
            .map(|summary| summary.tags.clone())
            .unwrap_or_default();
        if !tags.iter().any(|tag| tag == "conflict") {
            tags.push("conflict".to_string());
        }
        let created = store.create_note(NewNote {
            text: body,
            created_at: at,
            notebook_id: meta.as_ref().and_then(|summary| summary.notebook_id),
            tags,
        });
        match created {
            Ok(note) => {
                // The copy is a local change outside the editor's save path.
                // Ask for a follow-up run after the minimum gap so it reaches
                // the Worker before a web reload can drop the in-memory store.
                self.runtime.borrow_mut().focus_pending = true;
                self.reload();
                Some(note.title())
            }
            Err(error) => {
                self.toast_danger(format!("Could not keep the local version: {error}"));
                None
            }
        }
    }

    /// Save a deferred edit for a note that is no longer open, when the merge
    /// did not touch it.
    fn save_detached(&self, id: NoteId, text: String, at: i64) {
        let Some(store) = self.store() else {
            return;
        };
        let result = (|| -> Result<(), String> {
            let note = store
                .open_note(&id)
                .map_err(|error| format!("Could not load the note: {error}"))?;
            if note.text() == text {
                return Ok(());
            }
            note.set_next_commit_timestamp(at);
            note.replace_text(&text)
                .map_err(|error| format!("Could not apply the edit: {error}"))?;
            store
                .save_note(&note, at)
                .map_err(|error| format!("Could not save: {error}"))?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.reload();
                self.toast_info("Saved an edit made while sync was running.");
            }
            Err(message) => self.toast_danger(message),
        }
    }

    /// Record a failed run, preserve any held-back edit, and schedule a retry.
    fn apply_sync_failure(&self, failure: SyncFailure) {
        let (base, deferred, manual) = {
            let mut runtime = self.runtime.borrow_mut();
            runtime.busy = false;
            runtime.failures = runtime.failures.saturating_add(1);
            runtime.retry_after = failure.retry_after;
            // A permanent error waits for the user instead of quietly
            // spending a request every few minutes.
            runtime.stopped = !failure.retryable;
            let manual = runtime.manual_run;
            runtime.manual_run = false;
            (
                runtime.open_base.take(),
                runtime.deferred_save.take(),
                manual,
            )
        };

        let base_id = base.as_ref().map(|(id, _)| *id);
        let mut preserved = None;
        match (base, deferred) {
            (Some((id, base_text)), Some((deferred_id, text, at))) if deferred_id == id => {
                let live = self.state.peek().selected == Some(id);
                let ours = if live {
                    self.state.peek().draft.clone()
                } else {
                    text
                };
                if ours != base_text {
                    preserved = self.keep_conflict_copy(id, &ours, at);
                    if live {
                        let current = self
                            .store()
                            .and_then(|store| store.open_note(&id).ok())
                            .map(|note| note.text());
                        if let Some(current) = current {
                            self.adopt_open_note(id, &current, now());
                        }
                    }
                }
            }
            (_, Some((id, text, at))) => {
                let current = self
                    .store()
                    .and_then(|store| store.open_note(&id).ok())
                    .map(|note| note.text())
                    .unwrap_or_default();
                if text != current {
                    preserved = self.keep_conflict_copy(id, &text, at);
                }
            }
            _ => {}
        }

        // A partially applied pull can have changed the note the user switched
        // to. Show its committed body when the editor is clean.
        self.refresh_selected_after_run(base_id, now());

        let message = match &preserved {
            Some(title) => format!(
                "{} Your last edit was kept as \"{title}\".",
                failure.message
            ),
            None => failure.message.clone(),
        };
        if failure.unauthorized {
            #[cfg(not(target_arch = "wasm32"))]
            sync::clear_session();
            self.runtime.borrow_mut().client = None;
        }
        {
            let mut state = self.state.write_unchecked();
            state.sync.error = Some(message.clone());
            state.profile.sync_connected = !failure.unauthorized;
            state.sync.status = if failure.unauthorized {
                SyncStatus::NeedsLogin
            } else if failure.retryable {
                SyncStatus::Offline
            } else {
                SyncStatus::Error
            };
        }
        if manual || preserved.is_some() {
            self.toast_danger(message);
        }
    }

    /// Fetch the account's device list.
    pub fn refresh_devices(&self) {
        if self.state.peek().sync.devices_loading {
            return;
        }
        let client = {
            let runtime = self.runtime.borrow();
            if runtime.busy {
                return;
            }
            runtime.client.clone()
        };
        let Some(client) = client else {
            return;
        };
        {
            let mut state = self.state.write_unchecked();
            state.sync.devices_loading = true;
            state.sync.devices_error = None;
            state.sync.confirm_revoke = None;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let account = self.runtime.borrow().account.clone();
            let session = client.session();
            let (Some(account), Some(session)) = (account, session) else {
                self.state.write_unchecked().sync.devices_loading = false;
                return;
            };
            let (sender, receiver) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("notebox-devices".to_string())
                .spawn(move || {
                    let result = sync::devices_blocking(&account, &session)
                        .map(|list| (list, false))
                        .map_err(|error| sync::friendly_sync_error(&error));
                    let _ = sender.send(result);
                });
            match spawned {
                Ok(_) => self.runtime.borrow_mut().devices_rx = Some(receiver),
                Err(error) => {
                    self.finish_devices(Err(format!("Could not start the device thread: {error}")))
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let context = self.clone();
            spawn_forever(async move {
                let result = client
                    .devices()
                    .await
                    .map(|list| (list, false))
                    .map_err(|error| sync::friendly_sync_error(&error));
                context.finish_devices(result);
            });
        }
    }

    fn finish_devices(&self, result: Result<(notes_proto::DeviceList, bool), String>) {
        if !self.state.peek().profile.sync_connected {
            return;
        }
        let mut state = self.state.write_unchecked();
        state.sync.devices_loading = false;
        match result {
            Ok((list, revoked)) => {
                state.sync.devices = list.devices;
                state.sync.devices_error = None;
                if revoked {
                    state.sync.confirm_revoke = None;
                    drop(state);
                    self.toast_success("Device revoked");
                }
            }
            Err(error) => state.sync.devices_error = Some(error),
        }
    }

    /// Revoke one device; the first click asks for confirmation.
    pub fn revoke_device(&self, id: String) {
        if self.state.peek().sync.devices_loading {
            return;
        }
        if self.state.peek().sync.confirm_revoke.as_deref() != Some(id.as_str()) {
            self.state.write_unchecked().sync.confirm_revoke = Some(id);
            return;
        }
        let client = {
            let runtime = self.runtime.borrow();
            if runtime.busy {
                return;
            }
            runtime.client.clone()
        };
        let Some(client) = client else {
            return;
        };
        {
            let mut state = self.state.write_unchecked();
            state.sync.confirm_revoke = None;
            state.sync.devices_loading = true;
            state.sync.devices_error = None;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let account = self.runtime.borrow().account.clone();
            let session = client.session();
            let (Some(account), Some(session)) = (account, session) else {
                self.state.write_unchecked().sync.devices_loading = false;
                return;
            };
            let (sender, receiver) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("notebox-revoke".to_string())
                .spawn(move || {
                    let result = (|| -> Result<notes_proto::DeviceList, notes_sync::SyncError> {
                        sync::revoke_blocking(&account, &session, &id)?;
                        sync::devices_blocking(&account, &session)
                    })()
                    .map(|list| (list, true))
                    .map_err(|error| sync::friendly_sync_error(&error));
                    let _ = sender.send(result);
                });
            match spawned {
                Ok(_) => self.runtime.borrow_mut().devices_rx = Some(receiver),
                Err(error) => {
                    self.finish_devices(Err(format!("Could not start the device thread: {error}")))
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let context = self.clone();
            spawn_forever(async move {
                let result = async {
                    client.revoke_device(&id).await?;
                    client.devices().await
                }
                .await
                .map(|list| (list, true))
                .map_err(|error| sync::friendly_sync_error(&error));
                context.finish_devices(result);
            });
        }
    }

    /// Forget the local session. The web cookie is cleared by the Worker so a
    /// reload cannot sign the browser straight back in.
    pub fn sign_out(&self) {
        if self.runtime.borrow().busy {
            self.toast_info("Wait for the current sync to finish.");
            return;
        }
        let client = self.runtime.borrow().client.clone();

        #[cfg(target_arch = "wasm32")]
        if let Some(client) = client {
            use notes_sync::SyncTransport as _;

            spawn_forever(async move {
                let _ = client.transport().logout().await;
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = client;
            sync::clear_session();
        }
        let _ = sync::clear_account();
        {
            let mut runtime = self.runtime.borrow_mut();
            runtime.client = None;
            runtime.account = None;
            runtime.failures = 0;
            runtime.retry_after = None;
            runtime.stopped = false;
            runtime.focus_pending = false;
            runtime.open_base = None;
            runtime.deferred_save = None;
        }
        let (server, device) = {
            let state = self.state.peek();
            (state.login.server.clone(), state.login.device.clone())
        };
        {
            let mut state = self.state.write_unchecked();
            state.profile = Profile::default();
            state.sync = SyncUi::default();
            state.login.username.clear();
            state.login.passphrase.clear();
            state.login.error = None;
            state.login.server = server;
            state.login.device = device;
        }
        self.toast_info("Signed out. Your notes stay on this device.");
    }

    /// Write a zip of all notes and show where it went.
    pub fn export_notes(&self) {
        let Some(store) = self.store() else {
            return;
        };
        match store.export_zip(now()) {
            Ok(bytes) => self.deliver_export(bytes),
            Err(error) => self.toast_danger(format!("Export failed: {error}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn deliver_export(&self, bytes: Vec<u8>) {
        let directory = storage::export_directory();
        let name = format!("notebox-export-{}.zip", crate::time::iso_date(now()));
        let path = directory.join(name);
        let result = std::fs::create_dir_all(&directory).and_then(|_| std::fs::write(&path, bytes));
        match result {
            Ok(()) => self.toast_success(format!("Exported to {}", path.display())),
            Err(error) => self.toast_danger(format!("Could not write the export: {error}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn deliver_export(&self, bytes: Vec<u8>) {
        let encoded = serde_json::to_string(&notes_proto::Base64Bytes::new(bytes))
            .unwrap_or_else(|_| "\"\"".to_string());
        let name = format!("notebox-export-{}.zip", crate::time::iso_date(now()));
        let _ = document::eval(&format!(
            "const data = {encoded};\
             const bytes = Uint8Array.from(atob(data), c => c.charCodeAt(0));\
             const blob = new Blob([bytes], {{type: 'application/zip'}});\
             const link = document.createElement('a');\
             link.href = URL.createObjectURL(blob);\
             link.download = '{name}';\
             link.click();\
             setTimeout(() => URL.revokeObjectURL(link.href), 1000);"
        ));
        self.toast_success("Export downloaded");
    }

    /// Show an informational message.
    pub fn toast_info(&self, message: impl Into<String>) {
        self.push_toast(message.into(), ToastKind::Info);
    }

    /// Show a success message.
    pub fn toast_success(&self, message: impl Into<String>) {
        self.push_toast(message.into(), ToastKind::Success);
    }

    /// Show a failure message.
    pub fn toast_danger(&self, message: impl Into<String>) {
        self.push_toast(message.into(), ToastKind::Danger);
    }

    fn push_toast(&self, message: String, kind: ToastKind) {
        self.state.write_unchecked().toast = Some(Toast {
            message,
            kind,
            at: Instant::now(),
        });
    }
}

/// Read a picked or dropped file.
///
/// Dioxus 0.7.10's web `read_bytes` waits on a FileReader event that never
/// fires through the app's executor, and its `byte_stream` unwraps an
/// `ArrayBuffer` as a `Uint8Array`. Reading the `File` here avoids both bugs.
#[cfg(target_arch = "wasm32")]
async fn read_file_bytes(file: &dioxus::html::FileData) -> Result<Vec<u8>, String> {
    let file = file
        .inner()
        .downcast_ref::<web_sys::File>()
        .ok_or_else(|| "the browser did not provide a file".to_string())?
        .clone();
    let buffer = wasm_bindgen_futures::JsFuture::from(file.array_buffer())
        .await
        .map_err(|error| format!("could not read the file: {error:?}"))?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// Read a picked or dropped file on desktop and mobile.
#[cfg(not(target_arch = "wasm32"))]
async fn read_file_bytes(file: &dioxus::html::FileData) -> Result<Vec<u8>, String> {
    file.read_bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| format!("could not read the file: {error}"))
}

/// The text box selection, or the end of the draft when there is no text box.
async fn selected_range(fallback: usize) -> (usize, usize) {
    let eval = document::eval(
        "const el = document.getElementById('note-editor');\
         return el ? [el.selectionStart, el.selectionEnd] : null;",
    );
    match eval.join::<Option<(usize, usize)>>().await {
        Ok(Some(selection)) => selection,
        _ => (fallback, fallback),
    }
}

/// Display name for an attachment's image tag: no suffix, no brackets.
fn attachment_label(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let label: String = stem
        .chars()
        .filter(|ch| !matches!(ch, '[' | ']' | '\n' | '\r'))
        .take(80)
        .collect();
    let label = label.trim();
    if label.is_empty() {
        "image".to_string()
    } else {
        label.to_string()
    }
}

/// Whether a saved draft refers to a different set of attachments.
fn attachment_set_changed(draft: &str, attachments: &[Attachment]) -> bool {
    let references = notes_core::references(draft);
    references.len() != attachments.len()
        || references
            .iter()
            .any(|reference| !attachments.iter().any(|item| item.hash == reference.hash))
}

/// Clear the history panel when the note or the list changes underneath it.
///
/// Separate from [`AppContext::close_history`] so callers already holding the
/// state write lock can use it without a second borrow.
fn hide_history(state: &mut AppState) {
    state.history_open = false;
    state.history_versions.clear();
    state.history_selected = None;
    state.history_current = None;
    state.history_local_peer = 0;
    state.history_preview.clear();
    state.history_diff.clear();
    state.history_error = None;
}

/// Replace a summary in a list and keep the list order the store uses.
fn replace_summary(notes: &mut Vec<NoteSummary>, summary: NoteSummary, view: NoteView) {
    match notes.iter_mut().find(|note| note.id == summary.id) {
        Some(slot) => *slot = summary,
        None => notes.push(summary),
    }
    sort_summaries(notes, view);
}

fn sort_summaries(notes: &mut [NoteSummary], view: NoteView) {
    match view {
        NoteView::Trash => notes.sort_by(|left, right| {
            right
                .trashed_at
                .cmp(&left.trashed_at)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.cmp(&right.id))
        }),
        _ => notes.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then_with(|| right.updated_at.cmp(&left.updated_at))
                .then_with(|| left.id.cmp(&right.id))
        }),
    }
}

/// Number of words in a Markdown body.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Number of non-overlapping, case-insensitive matches.
pub fn find_match_count(text: &str, needle: &str) -> usize {
    let needle = needle.trim();
    if needle.is_empty() {
        return 0;
    }
    let haystack = text.to_lowercase();
    let needle = needle.to_lowercase();
    haystack.match_indices(&needle).count()
}

/// A CSS class for a stored colour token, or the default card tone.
pub fn tone_class(color: Option<&str>) -> &'static str {
    match color.unwrap_or("") {
        "sand" => "tone-sand",
        "amber" => "tone-amber",
        "rose" => "tone-rose",
        "sage" => "tone-sage",
        "sky" => "tone-sky",
        "lilac" => "tone-lilac",
        "slate" => "tone-slate",
        _ => "tone-default",
    }
}

/// Colours offered by the editor menu.
pub const COLORS: [(&str, &str); 7] = [
    ("sand", "Sand"),
    ("amber", "Amber"),
    ("rose", "Rose"),
    ("sage", "Sage"),
    ("sky", "Sky"),
    ("lilac", "Lilac"),
    ("slate", "Slate"),
];

/// Title for a note, with the placeholder the list uses.
pub fn note_title(note: &NoteSummary) -> String {
    if note.title.trim().is_empty() {
        "Untitled note".to_string()
    } else {
        note.title.clone()
    }
}

/// A one-line description of when a note was last edited.
pub fn edited_label(updated_at: i64) -> String {
    let label = relative_time(now(), updated_at);
    if label.is_empty() {
        String::new()
    } else {
        format!("Edited {label}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attachment_label, attachment_set_changed, find_match_count, tone_class, word_count,
    };
    use notes_store::Attachment;

    fn attachment(hash: &str) -> Attachment {
        Attachment {
            hash: hash.to_string(),
            name: "photo.webp".to_string(),
            mime: "image/webp".to_string(),
            size: 10,
            available: true,
            uploaded: false,
            created_at: 1_000,
        }
    }

    #[test]
    fn attachment_labels_drop_the_suffix_and_brackets() {
        assert_eq!(attachment_label("IMG 1.webp"), "IMG 1");
        assert_eq!(attachment_label("[draft].webp"), "draft");
        assert_eq!(attachment_label(".webp"), "image");
    }

    #[test]
    fn attachment_changes_are_detected_by_hash() {
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let body = format!("![pic](attachments/{hash}.webp)");
        assert!(!attachment_set_changed("plain text", &[]));
        assert!(attachment_set_changed(&body, &[]));
        assert!(!attachment_set_changed(&body, &[attachment(hash)]));
        assert!(attachment_set_changed(&body, &[attachment("other")]));
    }

    #[test]
    fn word_count_ignores_extra_whitespace() {
        assert_eq!(word_count("  one\ntwo\tthree  "), 3);
        assert_eq!(word_count(""), 0);
    }

    #[test]
    fn find_counts_case_insensitive_matches() {
        assert_eq!(find_match_count("One one ONE", "one"), 3);
        assert_eq!(find_match_count("hello", "zzz"), 0);
        assert_eq!(find_match_count("hello", "  "), 0);
    }

    #[test]
    fn unknown_colours_fall_back_to_the_default_tone() {
        assert_eq!(tone_class(Some("amber")), "tone-amber");
        assert_eq!(tone_class(Some("tone-rose")), "tone-default");
        assert_eq!(tone_class(None), "tone-default");
    }
}
