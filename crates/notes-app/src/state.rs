//! The app's state and the actions the UI can take.
//!
//! State is plain data in one `Signal`; the store sits beside it behind a
//! `Rc<dyn NoteStore>`. Components read state and call these methods, which
//! keeps the view free of storage details and gives the browser worker in P7
//! one place to plug into.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use notes_core::{plain_line, remove_attachment_references, Note, NoteVersion};
use notes_proto::{NoteId, Notebook, NotebookId, StorageUsage};
use notes_store::{
    Attachment, AttachmentUsage, ImportReport, NewNote, NoteQuery, NoteStore, NoteSummary,
    NoteView, SearchQuery, TagCount,
};
use notes_sync::BoxedTransport;
use serde::{Deserialize, Serialize};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::closure::Closure;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
use web_time::Instant;

use crate::images;
use crate::markdown_ops::{self, Mark};
use crate::media;
use crate::prefs::{SavedScreen, SavedView};
use crate::storage::{self, Persistence, StorageInfo};
use crate::sync::{self, Due, OpenNotePlan, SyncFailure, SyncReport, SyncStatus, SyncUi};
use crate::time::{now, relative_time};

/// Most card thumbnails kept in memory at once. A large gallery still marks
/// every image note with its fallback tile, but only these load the bytes.
const CARD_IMAGE_LIMIT: usize = 12;
/// Skip thumbnails larger than this. The full image still opens in the note.
const CARD_IMAGE_MAX_BYTES: usize = 800 * 1024;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// A backup archive chosen for restore, waiting for confirmation.
#[derive(Debug, Clone)]
pub struct PendingRestore {
    /// File name the user picked.
    pub name: String,
    /// Raw zip bytes, shared so a render does not copy the archive.
    pub bytes: Rc<Vec<u8>>,
    /// Notes in the archive.
    pub notes: usize,
    /// Notebooks in the archive.
    pub notebooks: usize,
    /// Attachments with bytes in the archive.
    pub attachments: usize,
    /// Whether the archive carries Loro snapshots for the version timeline.
    pub has_history: bool,
}

impl PendingRestore {
    /// The counts line in the confirmation.
    pub fn summary(&self) -> String {
        format!(
            "{}, {}, {}",
            count_label(self.notes, "note", "notes"),
            count_label(self.notebooks, "notebook", "notebooks"),
            count_label(self.attachments, "attachment", "attachments")
        )
    }
}

/// Note actions opened from a card with the secondary mouse button.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteMenu {
    /// Note the menu acts on.
    pub note_id: NoteId,
    /// Viewport coordinate of the pointer.
    pub x: f64,
    /// Viewport coordinate of the pointer.
    pub y: f64,
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
    pub password: String,
    /// Worker base URL.
    pub server: String,
    /// Device name shown in the account's device list.
    pub device: String,
    /// Validation message.
    pub error: Option<String>,
}

/// Fields on the “Sign-in details” form in Settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialsForm {
    /// Whether the form is expanded.
    pub open: bool,
    /// New account name, prefilled with the current one.
    pub username: String,
    /// Cleared as soon as an attempt starts; never stored.
    pub current_password: String,
    /// Blank keeps the current password.
    pub new_password: String,
    /// Must match [`Self::new_password`] when that field is filled.
    pub confirm: String,
    /// A request is in flight.
    pub busy: bool,
    /// Validation or server message.
    pub error: Option<String>,
}

/// Everything the view needs for one render.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Current screen.
    pub screen: Screen,
    /// First launch is asking for an account or the offline path.
    pub login_first_run: bool,
    /// The owner chose local-only use (or signed out).
    pub offline: bool,
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
    /// Whether the attachment panel is open below the editor.
    pub attachments_open: bool,
    /// Inline image URLs by content hash, for the rendered view.
    pub attachment_urls: HashMap<String, String>,
    /// The open note's attachment grid and full-image viewer.
    pub gallery_open: bool,
    /// Selected attachment in the gallery.
    pub gallery_index: usize,
    /// Delete confirmation for the selected attachment.
    pub gallery_confirm_delete: bool,
    /// How many notes link the selected attachment.
    pub gallery_note_count: u64,
    /// Whether the gallery was opened from a token in Write mode.
    pub gallery_from_editor: bool,
    /// UTF-16 caret to restore after the editor-opened gallery closes.
    pub gallery_editor_caret: Option<usize>,
    /// Attachment whose cloud copy is being deleted.
    pub attachment_deleting: Option<String>,
    /// Local attachment bytes, shown in Settings.
    pub attachment_usage: AttachmentUsage,
    /// First-image thumbnails for cards and search results, by content hash.
    pub card_images: HashMap<String, Rc<str>>,
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
    /// Whether the wide-screen sidebar is collapsed.
    pub sidebar_collapsed: bool,
    /// Which single pane a phone shows.
    pub mobile_list: bool,
    /// The wide note gallery, with the editor hidden, opened by Back.
    pub browsing: bool,
    /// Account panel under the avatar.
    pub account_open: bool,
    /// Note actions menu opened from a card.
    pub note_menu: Option<NoteMenu>,
    /// Card temporarily made selectable from its actions menu.
    pub text_select_note: Option<NoteId>,
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
    /// Ask before moving the open note to the trash.
    pub confirm_delete: bool,
    /// Open note waiting for the delete confirmation.
    pub pending_delete: Option<NoteId>,
    /// Backup chosen from Settings and waiting for confirmation.
    pub pending_restore: Option<PendingRestore>,
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
    /// A new web build is waiting to take over.
    pub update_ready: bool,
    /// The user chose to update later in this session.
    pub update_dismissed: bool,
    /// The page is waiting for the new worker to activate.
    pub update_applying: bool,
    /// Local profile.
    pub profile: Profile,
    /// Login form.
    pub login: LoginForm,
    /// Sign-in details form in Settings.
    pub credentials: CredentialsForm,
    /// Connection state, last run, and devices.
    pub sync: SyncUi,
    /// Minute of the last clock tick, so relative times refresh.
    pub clock_minute: i64,
}

impl AppState {
    fn new(storage: StorageInfo, storage_error: Option<String>) -> Self {
        let preferences = crate::prefs::load();
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
            login_first_run: false,
            offline: preferences.offline,
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
            attachments_open: false,
            attachment_urls: HashMap::new(),
            gallery_open: false,
            gallery_index: 0,
            gallery_confirm_delete: false,
            gallery_note_count: 0,
            gallery_from_editor: false,
            gallery_editor_caret: None,
            attachment_deleting: None,
            attachment_usage: AttachmentUsage::default(),
            card_images: HashMap::new(),
            attaching: 0,
            draft: String::new(),
            saved_text: String::new(),
            dirty: false,
            saved_at: 0,
            edit_seq: 0,
            seen_seq: 0,
            layout: preferences.layout,
            theme: preferences.theme,
            editor_mode: EditorMode::Write,
            find_open: false,
            find_query: String::new(),
            find_index: 0,
            palette_open: false,
            palette_query: String::new(),
            palette_index: 0,
            shortcuts_open: false,
            sidebar_open: false,
            sidebar_collapsed: false,
            mobile_list: true,
            browsing: false,
            account_open: false,
            note_menu: None,
            text_select_note: None,
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
            confirm_delete: preferences.confirm_delete,
            pending_delete: None,
            pending_restore: None,
            toast: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_undo: None,
            storage,
            storage_error,
            starting: true,
            storage_blocked: None,
            update_ready: false,
            update_dismissed: false,
            update_applying: false,
            profile: Profile::default(),
            login: LoginForm {
                server,
                username,
                device,
                ..LoginForm::default()
            },
            credentials: CredentialsForm::default(),
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

/// Build thumbnails for the first images on the current cards.
///
/// Data URLs are cached by hash across reloads. The cap keeps a search with
/// hundreds of image notes from turning every keystroke into a large decode;
/// notes beyond it still show the image fallback tile.
fn load_card_images(
    store: &dyn NoteStore,
    notes: &[NoteSummary],
    cached: &HashMap<String, Rc<str>>,
) -> HashMap<String, Rc<str>> {
    let mut images = HashMap::new();
    let mut seen = HashSet::new();
    for note in notes {
        let Some(image) = note.first_image.as_ref() else {
            continue;
        };
        if !image.available || !media::is_previewable_image(&image.mime) {
            continue;
        }
        if !seen.insert(image.hash.as_str()) {
            continue;
        }
        if images.len() >= CARD_IMAGE_LIMIT {
            break;
        }
        if let Some(url) = cached.get(&image.hash) {
            images.insert(image.hash.clone(), url.clone());
            continue;
        }
        if let Ok(Some(bytes)) = store.attachment_bytes(&image.hash) {
            if bytes.len() <= CARD_IMAGE_MAX_BYTES {
                let url = media::image_data_url(&image.mime, &bytes);
                images.insert(image.hash.clone(), Rc::from(url));
            }
        }
    }
    images
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
                    "Could not save to browser storage ({error}). Notes stay in this tab only; export a zip to keep them and restore it later in Settings."
                ),
                temporary: true,
            };
        }
        self.toast_danger(
            "Could not save to browser storage. Export a zip to keep your notes and restore it later in Settings.",
        );
    }

    fn bootstrap(&self) {
        let Some(store) = self.store() else {
            return;
        };
        let preferences = crate::prefs::load();
        let has_notes = match store.list_notes(&NoteQuery {
            view: NoteView::All,
            limit: 1,
            ..NoteQuery::default()
        }) {
            Ok(notes) => !notes.is_empty(),
            Err(error) => {
                self.toast_danger(format!("Could not read notes: {error}"));
                true
            }
        };
        let current_offline = self.state.peek().offline;
        let start = startup_choice(sync::load_account().is_some(), current_offline, has_notes);
        let first_run = start.login_first_run;

        // Apply the saved list slice before the first reload, so the sidebar
        // and the note list agree from the first frame.
        if !first_run {
            self.state.write_unchecked().view = preferences.last_view.into();
        }
        self.reload();

        {
            let mut state = self.state.write_unchecked();
            state.offline = start.offline;
            state.login_first_run = first_run;
            if first_run {
                state.selected = None;
                state.mobile_list = true;
                state.screen = Screen::Login;
                hide_history(&mut state);
                hide_editor_overlays(&mut state);
            } else {
                state.screen = Screen::Notes;
            }
        }
        if start.offline != current_offline {
            self.save_preferences();
        }
        if !first_run {
            let saved = preferences
                .last_note
                .filter(|id| store.note_summary(id).is_ok());
            let first = self.state.peek().notes.first().map(|note| note.id);
            if let Some(id) = saved.or(first) {
                self.select_note(id);
            }
            {
                let mut state = self.state.write_unchecked();
                state.screen = Screen::from(preferences.last_screen);
                state.mobile_list = preferences.mobile_list;
                state.browsing = preferences.browsing;
                if state.selected.is_none() {
                    state.mobile_list = true;
                    state.browsing = false;
                }
            }
            // Normalize the file after a fallback (missing note, deleted view)
            // so the next launch starts from what is on screen now.
            self.save_preferences();
        }
    }

    /// Add the optional tour to an empty local workspace.
    ///
    /// The notes get fresh random ids. Fixed ids made two devices merge their
    /// welcome text into the same Loro document, and concurrent inserts from
    /// both peers repeated the body. Ordinary notes avoid that merge shape.
    pub fn add_welcome_tour(&self) {
        self.save_now();
        let Some(store) = self.store() else {
            return;
        };
        let timestamp = now();
        let result: Result<NoteId, String> = (|| {
            let notebooks = store
                .list_notebooks()
                .map_err(|error| format!("Could not read notebooks: {error}"))?;
            let personal = match notebooks.iter().find(|item| item.name == "Personal") {
                Some(notebook) => notebook.id,
                None => {
                    store
                        .create_notebook("Personal", timestamp - 86_400 * 3)
                        .map_err(|error| format!("Could not create a notebook: {error}"))?
                        .id
                }
            };
            let projects = match notebooks.iter().find(|item| item.name == "Projects") {
                Some(notebook) => notebook.id,
                None => {
                    store
                        .create_notebook("Projects", timestamp - 86_400 * 2)
                        .map_err(|error| format!("Could not create a notebook: {error}"))?
                        .id
                }
            };

            let welcome = "Welcome to Notebox\n\n\
                A local-first place for your notes. Everything you write stays on this device.\n\n\
                ## Try the basics\n\n\
                - [ ] Press **Ctrl K** to search your notes\n\
                - [ ] Press **Ctrl P** to open the command palette\n\
                - [ ] Type Markdown and switch to the rendered view with the eye button\n\
                - [ ] Pin a note to keep it at the top of the list\n\n\
                > Edits are saved automatically as you type.\n\n\
                ## Shortcuts\n\n\
                | Keys | Action |\n\
                | --- | --- |\n\
                | Ctrl K | Search notes |\n\
                | Ctrl P | Command palette |\n\
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

            let welcome_id = create_tour_note(
                store,
                welcome,
                &["welcome"],
                Some(projects),
                "sage",
                timestamp - 3_600,
                true,
            )?;
            create_tour_note(
                store,
                reading,
                &["books", "design"],
                Some(personal),
                "sky",
                timestamp - 86_400,
                false,
            )?;
            create_tour_note(
                store,
                weekend,
                &["personal"],
                Some(personal),
                "amber",
                timestamp - 3_600 * 8,
                false,
            )?;
            Ok(welcome_id)
        })();

        match result {
            Ok(id) => {
                {
                    let mut state = self.state.write_unchecked();
                    state.screen = Screen::Notes;
                    state.view = NoteView::Active;
                    state.notebook = None;
                    state.tag = None;
                    state.search.clear();
                    state.search_hits = None;
                    state.snippets.clear();
                    state.login_first_run = false;
                }
                self.reload();
                self.select_note(id);
                self.toast_success("Welcome tour added");
            }
            Err(message) => self.toast_danger(message),
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

        let card_images = {
            let cached = self.state.peek();
            load_card_images(store, &notes, &cached.card_images)
        };
        let notebooks = store.list_notebooks().unwrap_or_default();
        let tags = store.list_tags().unwrap_or_default();
        let note_count = store.note_count().unwrap_or(0);

        let attachment_usage = store.attachment_usage().unwrap_or_default();
        let mut state = self.state.write_unchecked();
        state.notes = notes;
        state.snippets = snippets;
        state.search_hits = search_hits;
        state.card_images = card_images;
        state.notebooks = notebooks;
        state.tags = tags;
        state.note_count = note_count;
        state.attachment_usage = attachment_usage;
        if state
            .note_menu
            .as_ref()
            .is_some_and(|menu| !state.notes.iter().any(|note| note.id == menu.note_id))
        {
            state.note_menu = None;
        }
        if state
            .text_select_note
            .is_some_and(|id| !state.notes.iter().any(|note| note.id == id))
        {
            state.text_select_note = None;
        }
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
            state.note_menu = None;
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
            state.note_menu = None;
            hide_history(&mut state);
        }
        self.reload();
        self.save_preferences();
    }

    /// Filter the list to one notebook, or clear the filter.
    pub fn filter_notebook(&self, notebook: Option<NotebookId>) {
        {
            let mut state = self.state.write_unchecked();
            state.notebook = notebook;
            state.tag = None;
            state.mobile_list = true;
            state.screen = Screen::Notes;
            state.note_menu = None;
            hide_history(&mut state);
        }
        self.reload();
        self.save_preferences();
    }

    /// Filter the list to one tag, or clear the filter.
    pub fn filter_tag(&self, tag: Option<String>) {
        {
            let mut state = self.state.write_unchecked();
            state.tag = tag;
            state.notebook = None;
            state.mobile_list = true;
            state.screen = Screen::Notes;
            state.note_menu = None;
            hide_history(&mut state);
        }
        self.reload();
        self.save_preferences();
    }

    /// Open a note in the editor, saving the current one first.
    pub fn select_note(&self, id: NoteId) {
        self.end_text_selection();
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
                hide_editor_overlays(&mut state);
                state.undo.clear();
                state.redo.clear();
                state.last_undo = None;
                state.mobile_list = false;
                state.browsing = false;
                state.editor_mode = EditorMode::Write;
                hide_history(&mut state);
                state.attachments.clear();
                state.attachments_open = false;
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
            self.reset_editor_scroll();
            self.save_preferences();
        }
    }

    /// Open a search result at its first matching body text.
    ///
    /// Find stays closed and nothing is selected: the search highlight belongs
    /// to the result cards. Tag-only matches have no body text, so the note
    /// opens at the top.
    pub fn open_search_result(&self, id: NoteId, query: String) {
        self.select_note(id);
        if self.state.peek().selected != Some(id) {
            return;
        }
        let term = {
            let state = self.state.read();
            best_body_term(&state.draft, &query)
        };
        if let Some(term) = term {
            self.reveal_editor_term(&term);
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
            id: None,
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
                    // The card marker comes from the store summary, and the
                    // thumbnail cache is built with a list reload. Refresh both
                    // as soon as an image is added, not only after Back.
                    self.reload();
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
                        && media::is_previewable_image(&attachment.mime)
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

        let usage = store.attachment_usage().unwrap_or_default();
        let mut gallery_closed = false;
        let mut state = self.state.write_unchecked();
        if state.selected != Some(id) {
            return;
        }
        state.attachment_urls.retain(|hash, _| {
            attachments.iter().any(|attachment| {
                &attachment.hash == hash
                    && attachment.available
                    && media::is_previewable_image(&attachment.mime)
            })
        });
        for (hash, url) in fresh_urls {
            state.attachment_urls.insert(hash, url);
        }
        state.attachments_open = state.attachments_open && !attachments.is_empty();
        state.attachments = attachments;
        state.attachment_usage = usage;
        if state.gallery_open {
            if state.attachments.is_empty() {
                state.gallery_open = false;
                state.gallery_confirm_delete = false;
                state.gallery_note_count = 0;
                gallery_closed = true;
            } else {
                state.gallery_index = state.gallery_index.min(state.attachments.len() - 1);
                state.gallery_confirm_delete = false;
            }
        }
        drop(state);
        if gallery_closed {
            self.restore_gallery_focus();
        } else if self.state.peek().gallery_open {
            // The deleted item can shift the selection to another attachment.
            // Refresh the shared-file count before a Delete button is enabled.
            self.refresh_gallery_note_count();
        }
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
                state.account_open = false;
                state.note_menu = None;
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
        self.find_select_after(index, 0);
    }

    fn find_select_after(&self, index: usize, delay_ms: u32) {
        let query = self.state.peek().find_query.clone();
        if query.is_empty() {
            return;
        }
        let needle = serde_json::to_string(&query).unwrap_or_else(|_| "\"\"".to_string());
        let _ = document::eval(&format!(
            "setTimeout(() => {{\
             const el = document.getElementById('note-editor');\
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
             }}\
             }}, {delay_ms});"
        ));
    }

    /// Start a newly opened note at the top.
    ///
    /// A textarea keeps its scroll position when its value changes, so a long
    /// note opened after another one would otherwise start partway down.
    fn reset_editor_scroll(&self) {
        let _ = document::eval(
            "setTimeout(() => {\
             const el = document.getElementById('note-editor');\
             if (el) el.scrollTop = 0;\
             }, 0);",
        );
    }

    /// Put the caret on `term` and scroll it into view without focusing.
    ///
    /// A textarea does not scroll to a selection while it is unfocused, so a
    /// hidden copy of its text measures the match. Focusing the editor would
    /// open the on-screen keyboard on a phone and take the arrow keys away
    /// from the note list.
    fn reveal_editor_term(&self, term: &str) {
        let needle = serde_json::to_string(term).unwrap_or_else(|_| "\"\"".to_string());
        let _ = document::eval(&format!(
            "setTimeout(() => {{\
             const el = document.getElementById('note-editor');\
             if (!el) return;\
             const needle = {needle}.toLowerCase();\
             if (!needle) return;\
             const at = el.value.toLowerCase().indexOf(needle);\
             if (at < 0) return;\
             el.setSelectionRange(at, at);\
             const style = getComputedStyle(el);\
             const mirror = document.createElement('div');\
             mirror.style.position = 'absolute';\
             mirror.style.visibility = 'hidden';\
             mirror.style.top = '0';\
             mirror.style.left = '-10000px';\
             mirror.style.width = el.clientWidth + 'px';\
             mirror.style.boxSizing = 'border-box';\
             mirror.style.whiteSpace = 'pre-wrap';\
             mirror.style.overflowWrap = 'break-word';\
             mirror.style.fontFamily = style.fontFamily;\
             mirror.style.fontSize = style.fontSize;\
             mirror.style.fontWeight = style.fontWeight;\
             mirror.style.fontStyle = style.fontStyle;\
             mirror.style.lineHeight = style.lineHeight;\
             mirror.style.letterSpacing = style.letterSpacing;\
             mirror.style.tabSize = style.tabSize;\
             mirror.style.padding = style.padding;\
             mirror.textContent = el.value.slice(0, at);\
             const marker = document.createElement('span');\
             marker.textContent = el.value.slice(at, at + 1) || '.';\
             mirror.appendChild(marker);\
             document.body.appendChild(mirror);\
             const top = marker.offsetTop;\
             mirror.remove();\
             el.scrollTop = Math.max(0, top - el.clientHeight / 3);\
             }}, 60);"
        ));
    }

    /// Open a note chosen from its card menu.
    pub fn open_note_from_menu(&self, id: NoteId) {
        self.close_note_menu(false);
        self.select_note(id);
        if self.state.peek().selected == Some(id) {
            self.focus_note_card(id);
        }
    }

    /// Open a note and its history panel from the card menu.
    pub fn open_history_from_menu(&self, id: NoteId) {
        self.close_note_menu(false);
        self.select_note(id);
        if self.state.peek().selected == Some(id) {
            self.open_history();
        }
    }

    /// Open the card menu at a viewport point.
    pub fn open_note_menu(&self, id: NoteId, x: f64, y: f64) {
        self.end_text_selection();
        let mut state = self.state.write_unchecked();
        state.note_menu = Some(NoteMenu { note_id: id, x, y });
        state.account_open = false;
        state.color_open = false;
        state.sidebar_open = false;
    }

    /// Close the card menu, optionally returning focus to its card.
    pub fn close_note_menu(&self, return_focus: bool) {
        let menu = self.state.write_unchecked().note_menu.take();
        if return_focus {
            if let Some(menu) = menu {
                self.focus_note_card(menu.note_id);
            }
        }
    }

    /// Make one card's text selectable so it can be copied without opening it.
    pub fn begin_text_selection(&self, id: NoteId) {
        self.close_note_menu(false);
        self.state.write_unchecked().text_select_note = Some(id);
        self.focus_note_card(id);
        self.toast_info("Select the text you need. Tap outside when done.");
    }

    /// Leave select-text mode and drop the browser's selection handles.
    pub fn end_text_selection(&self) {
        let active = self
            .state
            .write_unchecked()
            .text_select_note
            .take()
            .is_some();
        if active {
            let _ = document::eval("window.getSelection()?.removeAllRanges();");
        }
    }

    fn focus_note_card(&self, id: NoteId) {
        let _ = document::eval(&format!(
            "setTimeout(() => {{\
             const target = document.getElementById('note-card-{id}')\
                 || document.getElementById('app-root');\
             target?.focus();\
             }}, 0);"
        ));
    }

    /// Toggle pin from a card menu.
    pub fn toggle_pin_from_menu(&self, id: NoteId) {
        self.close_note_menu(false);
        self.mutate_note(id, |note| {
            let pinned = note.meta().pinned;
            note.set_pinned(!pinned)?;
            Ok(true)
        });
        self.focus_note_card(id);
    }

    /// Toggle archive from a card menu.
    pub fn toggle_archive_from_menu(&self, id: NoteId) {
        self.close_note_menu(false);
        self.mutate_note(id, |note| {
            let archived = note.meta().archived;
            note.set_archived(!archived)?;
            Ok(true)
        });
        self.focus_note_card(id);
    }

    /// Set or clear a note's colour from its card menu.
    pub fn set_color_from_menu(&self, id: NoteId, color: Option<&str>) {
        self.close_note_menu(false);
        let color = color.map(str::to_string);
        self.mutate_note(id, move |note| {
            note.set_color(color.as_deref())?;
            Ok(true)
        });
        self.focus_note_card(id);
    }

    /// Move a note to a notebook from its card menu.
    pub fn set_notebook_from_menu(&self, id: NoteId, notebook_id: Option<NotebookId>) {
        self.close_note_menu(false);
        self.mutate_note(id, move |note| {
            note.set_notebook(notebook_id)?;
            Ok(true)
        });
        self.focus_note_card(id);
    }

    /// Move a note to the trash from its card menu.
    ///
    /// The existing delete preference and inline confirmation stay the only
    /// path, so the menu does not add a second confirmation UI.
    pub fn trash_from_menu(&self, id: NoteId) {
        self.close_note_menu(false);
        if self.state.peek().selected != Some(id) {
            self.select_note(id);
        }
        if self.state.peek().selected == Some(id) {
            self.trash_selected();
        }
    }

    /// Restore a trashed note from its card menu.
    pub fn restore_from_menu(&self, id: NoteId) {
        self.close_note_menu(false);
        self.restore_note(id);
        if self.state.peek().selected == Some(id) {
            self.focus_note_card(id);
        }
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

    /// Move the open note to the trash, asking first when the preference is on.
    pub fn trash_selected(&self) {
        let Some(id) = self.state.peek().selected else {
            return;
        };
        if self.state.peek().confirm_delete {
            if self.state.peek().pending_delete == Some(id) {
                return;
            }
            self.state.write_unchecked().pending_delete = Some(id);
            self.focus_delete_cancel();
            return;
        }
        self.trash_note(id);
    }

    /// Confirm the pending move to the trash.
    pub fn confirm_trash(&self) {
        let id = self
            .state
            .peek()
            .pending_delete
            .or(self.state.peek().selected);
        if let Some(id) = id {
            self.trash_note(id);
        }
    }

    /// Dismiss the delete confirmation and return focus to the trash button.
    pub fn cancel_trash(&self) {
        self.state.write_unchecked().pending_delete = None;
        self.focus_trash_button();
    }

    fn trash_note(&self, id: NoteId) {
        self.save_now();
        let was_selected = self.state.peek().selected == Some(id);
        let Some(store) = self.store() else {
            return;
        };
        match store.trash_note(&id, now()) {
            Ok(true) => {
                self.state.write_unchecked().pending_delete = None;
                self.reload();
                if was_selected {
                    let next = self.state.peek().notes.first().map(|note| note.id);
                    match next {
                        Some(next) => self.select_note(next),
                        None => self.clear_selection(),
                    }
                }
                self.toast_info("Moved to trash");
            }
            Ok(false) => {
                self.state.write_unchecked().pending_delete = None;
                self.toast_info("Note is already in the trash");
            }
            Err(error) => self.toast_danger(format!("Could not move the note: {error}")),
        }
    }

    fn focus_delete_cancel(&self) {
        let _ = document::eval(
            "setTimeout(() => document.getElementById('confirm-trash-cancel')?.focus(), 30);",
        );
    }

    fn focus_trash_button(&self) {
        let _ =
            document::eval("setTimeout(() => document.getElementById('trash-note')?.focus(), 30);");
    }

    /// Turn the delete confirmation on or off.
    pub fn set_confirm_delete(&self, enabled: bool) {
        {
            let mut state = self.state.write_unchecked();
            state.confirm_delete = enabled;
            if !enabled {
                state.pending_delete = None;
            }
        }
        self.save_preferences();
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
        {
            let mut state = self.state.write_unchecked();
            state.selected = None;
            state.draft.clear();
            state.saved_text.clear();
            state.dirty = false;
            state.undo.clear();
            state.redo.clear();
            state.mobile_list = true;
            state.attachments_open = false;
            hide_history(&mut state);
            hide_editor_overlays(&mut state);
        }
        self.save_preferences();
    }

    fn mutate_selected(&self, edit: impl FnOnce(&Note) -> Result<bool, notes_core::NoteError>) {
        let Some(id) = self.state.peek().selected else {
            return;
        };
        self.mutate_note(id, edit);
    }

    fn mutate_note(
        &self,
        id: NoteId,
        edit: impl FnOnce(&Note) -> Result<bool, notes_core::NoteError>,
    ) {
        self.save_now();
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
            self.save_preferences();
            return;
        }
        {
            let mut state = self.state.write_unchecked();
            state.screen = screen;
            state.palette_open = false;
            state.shortcuts_open = false;
            state.sidebar_open = false;
            state.account_open = false;
            hide_history(&mut state);
            hide_editor_overlays(&mut state);
            if screen == Screen::Notes {
                state.mobile_list = state.browsing || state.selected.is_none();
            }
            if screen == Screen::Login {
                state.login.error = None;
                state.login_first_run = false;
            }
            if screen == Screen::Notes {
                state.login_first_run = false;
            }
        }
        self.save_preferences();
    }

    /// Leave the editor for the notes gallery.
    ///
    /// On a phone this is the existing list pane. On a wide window the app
    /// hides the editor and lets the list use the room, so Back has a visible
    /// result instead of only setting the phone flag.
    pub fn back_to_notes(&self) {
        self.save_now();
        {
            let mut state = self.state.write_unchecked();
            state.browsing = true;
            state.mobile_list = true;
            state.attachments_open = false;
            hide_history(&mut state);
            hide_editor_overlays(&mut state);
        }
        self.reload();
        self.save_preferences();
    }

    /// Show or hide the open note's attachment strip. It starts closed so a
    /// note with an image is not permanently shorter on every screen.
    pub fn toggle_attachments(&self) {
        let mut state = self.state.write_unchecked();
        state.attachments_open = !state.attachments_open && !state.attachments.is_empty();
    }

    /// Open the per-note attachment gallery, optionally on one image.
    ///
    /// The gallery is another view of the note's own attachment rows, not a
    /// library: removing the last reference removes the item from this list.
    pub fn open_attachment_gallery(&self, hash: Option<String>) {
        self.open_gallery(hash, None);
    }

    /// Open an image token from Write mode.
    ///
    /// The hash is checked against the open note's own attachments first: a
    /// stale Markdown link must not silently open a different image. The
    /// caret is remembered so closing the viewer returns to the source token.
    pub fn open_editor_attachment(&self, hash: String, caret: Option<usize>) {
        let attached = {
            let state = self.state.peek();
            state
                .attachments
                .iter()
                .any(|attachment| attachment.hash == hash)
        };
        if !attached {
            self.toast_info("That image is no longer attached to this note");
            return;
        }
        self.open_gallery(Some(hash), caret);
    }

    fn open_gallery(&self, hash: Option<String>, editor_caret: Option<usize>) {
        let (selected, count) = {
            let state = self.state.peek();
            (state.selected, state.attachments.len())
        };
        if selected.is_none() {
            return;
        }
        if count == 0 {
            self.toast_info("This note has no attachments yet");
            return;
        }
        let return_hash = hash.clone();
        let index = {
            let state = self.state.peek();
            hash.and_then(|hash| {
                state
                    .attachments
                    .iter()
                    .position(|attachment| attachment.hash == hash)
            })
            .or_else(|| {
                if state.gallery_open && state.gallery_index < count {
                    Some(state.gallery_index)
                } else {
                    None
                }
            })
            .unwrap_or(0)
        };
        {
            let mut state = self.state.write_unchecked();
            state.gallery_open = true;
            state.gallery_index = index;
            state.gallery_confirm_delete = false;
            state.palette_open = false;
            state.shortcuts_open = false;
            state.account_open = false;
            state.color_open = false;
            state.find_open = false;
            state.gallery_from_editor = editor_caret.is_some();
            state.gallery_editor_caret = editor_caret;
            hide_history(&mut state);
        }
        self.runtime.borrow_mut().gallery_return_hash = return_hash;
        self.refresh_gallery_note_count();
        let _ = document::eval(
            "setTimeout(() => document.getElementById('gallery-close')?.focus(), 30);",
        );
    }

    /// Close the gallery and return focus to whatever opened it.
    pub fn close_attachment_gallery(&self) {
        {
            let mut state = self.state.write_unchecked();
            state.gallery_open = false;
            state.gallery_confirm_delete = false;
            state.gallery_note_count = 0;
        }
        self.restore_gallery_focus();
    }

    /// Put focus back on the image or button that opened the gallery.
    ///
    /// A deleted image no longer exists, and the footer button disappears
    /// with the last attachment, so fall back to the app root.
    fn restore_gallery_focus(&self) {
        let hash = self.runtime.borrow_mut().gallery_return_hash.take();
        let (from_editor, caret) = {
            let state = self.state.peek();
            (state.gallery_from_editor, state.gallery_editor_caret)
        };
        {
            let mut state = self.state.write_unchecked();
            state.gallery_from_editor = false;
            state.gallery_editor_caret = None;
        }
        if from_editor {
            let caret = serde_json::to_string(&caret).unwrap_or_else(|_| "null".to_string());
            let _ = document::eval(&format!(
                "setTimeout(() => {{\
                   const caret = {caret};\
                   const touch = window.matchMedia?.('(hover: none)').matches ?? false;\
                   const editor = document.getElementById('note-editor');\
                   const chip = document.getElementById('token-open');\
                   if (touch) {{\
                     (chip ?? document.getElementById('app-root'))?.focus();\
                     return;\
                   }}\
                   if (editor) {{\
                     editor.focus();\
                     if (caret !== null) editor.setSelectionRange(caret, caret);\
                     return;\
                   }}\
                   (chip ?? document.getElementById('app-root'))?.focus();\
                 }}, 30)"
            ));
            return;
        }
        let hash = serde_json::to_string(&hash).unwrap_or_else(|_| "null".to_string());
        let _ = document::eval(&format!(
            "setTimeout(() => {{\
               const hash = {hash};\
               if (hash) {{\
                 const image = Array.from(document.querySelectorAll('[data-attachment-hash]'))\
                   .find((el) => el.getAttribute('data-attachment-hash') === hash);\
                 if (image) {{ image.focus(); return; }}\
               }}\
               const fallback = document.getElementById('status-attachments-button')\
                 ?? document.getElementById('app-root');\
               fallback?.focus();\
             }}, 30)"
        ));
    }

    /// Select one image in the gallery.
    pub fn select_gallery_index(&self, index: usize) {
        {
            let mut state = self.state.write_unchecked();
            if index >= state.attachments.len() {
                return;
            }
            state.gallery_index = index;
            state.gallery_confirm_delete = false;
        }
        self.refresh_gallery_note_count();
    }

    /// Move the gallery selection, wrapping at either end.
    pub fn step_gallery(&self, delta: isize) {
        let (index, count) = {
            let state = self.state.peek();
            (state.gallery_index, state.attachments.len())
        };
        if count == 0 {
            return;
        }
        let next = (index as isize + delta).rem_euclid(count as isize) as usize;
        self.select_gallery_index(next);
    }

    /// Ask the store how many notes still link the selected attachment.
    fn refresh_gallery_note_count(&self) {
        let hash = {
            let state = self.state.peek();
            state
                .attachments
                .get(state.gallery_index)
                .map(|attachment| attachment.hash.clone())
        };
        let Some(hash) = hash else {
            return;
        };
        let count = self
            .store()
            .and_then(|store| store.attachment_note_count(&hash).ok())
            .unwrap_or(1);
        let mut state = self.state.write_unchecked();
        if state.gallery_open
            && state
                .attachments
                .get(state.gallery_index)
                .is_some_and(|attachment| attachment.hash == hash)
        {
            state.gallery_note_count = count;
        }
    }

    /// Remove every Markdown reference to an attachment, keeping its bytes.
    pub fn remove_attachment_from_note(&self, attachment: Attachment) {
        let count = self
            .store()
            .and_then(|store| store.attachment_note_count(&attachment.hash).ok())
            .unwrap_or(1);
        if count <= 1 {
            self.toast_danger(
                "This is the only note that uses the file. Use Delete to remove the last copy.",
            );
            return;
        }
        if self.state.peek().sync.status.busy() || self.runtime.borrow().busy {
            self.toast_info("Wait for the current sync to finish before removing an image.");
            return;
        }
        let draft = self.state.peek().draft.clone();
        let edited = remove_attachment_references(&draft, &attachment.hash);
        if edited == draft {
            self.toast_info("That image is no longer linked in this note");
            return;
        }
        self.note_input(edited);
        self.save_now();
        if self.state.peek().dirty {
            self.toast_danger("The reference could not be removed because the note did not save.");
        } else {
            self.toast_success(format!("Removed {} from this note", attachment.name));
        }
    }

    /// Open the delete confirmation for the selected attachment.
    pub fn request_attachment_delete(&self) {
        if self.state.peek().attachment_deleting.is_some() {
            return;
        }
        self.refresh_gallery_note_count();
        let (hash, uploaded, count, connected, status, busy) = {
            let state = self.state.peek();
            let current = state.attachments.get(state.gallery_index);
            (
                current.map(|attachment| attachment.hash.clone()),
                current.is_some_and(|attachment| attachment.uploaded),
                state.gallery_note_count,
                state.profile.sync_connected,
                state.sync.status,
                state.sync.status.busy() || self.runtime.borrow().busy,
            )
        };
        let Some(hash) = hash else {
            return;
        };
        if let Some(blocker) = attachment_delete_blocker(uploaded, count, connected, status, busy) {
            self.state.write_unchecked().gallery_confirm_delete = false;
            self.report_attachment_delete_blocker(blocker);
            return;
        }
        let mut state = self.state.write_unchecked();
        if state
            .attachments
            .get(state.gallery_index)
            .is_none_or(|attachment| attachment.hash != hash)
        {
            return;
        }
        state.gallery_confirm_delete = true;
    }

    /// Close the delete confirmation without changing anything.
    pub fn cancel_attachment_delete(&self) {
        self.state.write_unchecked().gallery_confirm_delete = false;
        let _ = document::eval(
            "requestAnimationFrame(() => document.getElementById('gallery-delete')?.focus());",
        );
    }

    /// Confirm deletion of the selected attachment.
    ///
    /// A signed-in client deletes the server copy first. If that request fails,
    /// nothing local changes. A lost note reference can be repaired by sync;
    /// orphaned bytes cannot.
    pub fn confirm_attachment_delete(&self) {
        if self.state.peek().attachment_deleting.is_some() {
            return;
        }
        let (hash, note_id, uploaded, connected, status, busy) = {
            let state = self.state.peek();
            if !state.gallery_confirm_delete {
                return;
            }
            let Some(note_id) = state.selected else {
                return;
            };
            let current = state.attachments.get(state.gallery_index);
            (
                current.map(|attachment| attachment.hash.clone()),
                note_id,
                current.is_some_and(|attachment| attachment.uploaded),
                state.profile.sync_connected,
                state.sync.status,
                state.sync.status.busy() || self.runtime.borrow().busy,
            )
        };
        let Some(hash) = hash else {
            return;
        };
        // Sync, another note or the session can change while the confirmation
        // is open. Re-check the hard guards before the cloud copy goes away.
        let count = self
            .store()
            .and_then(|store| store.attachment_note_count(&hash).ok())
            .unwrap_or(1);
        if let Some(blocker) = attachment_delete_blocker(uploaded, count, connected, status, busy) {
            self.state.write_unchecked().gallery_confirm_delete = false;
            self.report_attachment_delete_blocker(blocker);
            return;
        }
        {
            let mut state = self.state.write_unchecked();
            state.gallery_confirm_delete = false;
            state.attachment_deleting = Some(hash.clone());
        }
        let _ = document::eval(
            "requestAnimationFrame(() => document.getElementById('gallery-close')?.focus());",
        );
        {
            let mut runtime = self.runtime.borrow_mut();
            runtime.delete_hash = Some(hash.clone());
            runtime.delete_note = Some(note_id);
        }
        if connected {
            self.start_cloud_attachment_delete(hash);
        } else {
            self.finish_attachment_delete(&hash, false);
        }
    }

    fn report_attachment_delete_blocker(&self, blocker: AttachmentDeleteBlocker) {
        match blocker {
            AttachmentDeleteBlocker::Busy => {
                self.toast_info("Wait for the current sync to finish before deleting a file.")
            }
            AttachmentDeleteBlocker::Shared(count) => self.toast_danger(format!(
                "This file is used in {count} notes. Remove it from the other notes first."
            )),
            AttachmentDeleteBlocker::SignIn => {
                self.toast_danger("Sign in to remove the cloud copy. Nothing was deleted.")
            }
            AttachmentDeleteBlocker::Offline => {
                self.toast_danger("Reconnect to delete the cloud copy. Nothing was deleted.")
            }
        }
    }

    fn attachment_delete_failed(&self, message: String) {
        {
            let mut runtime = self.runtime.borrow_mut();
            runtime.delete_hash = None;
            runtime.delete_note = None;
        }
        self.state.write_unchecked().attachment_deleting = None;
        self.toast_danger(message);
    }

    fn start_cloud_attachment_delete(&self, hash: String) {
        let client = self.runtime.borrow().client.clone();
        let Some(client) = client else {
            self.attachment_delete_failed(
                "The device session is missing. Sign in again; nothing was deleted.".to_string(),
            );
            return;
        };
        let Some(store) = self.store() else {
            self.attachment_delete_failed(
                "The local store is not open. Nothing was deleted.".to_string(),
            );
            return;
        };
        // Clear the upload confirmation before asking the server. If the app
        // stops after the cloud copy is gone, the next sync uploads the local
        // bytes again rather than leaving a dead link.
        if let Err(error) = store.mark_attachment_pending(&hash) {
            self.attachment_delete_failed(format!(
                "Could not prepare the delete: {error}. Nothing was deleted."
            ));
            return;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let account = self.runtime.borrow().account.clone();
            let session = client.session();
            let (Some(account), Some(session)) = (account, session) else {
                self.attachment_delete_failed(
                    "The device session is missing. Sign in again; nothing was deleted."
                        .to_string(),
                );
                return;
            };
            let worker_hash = hash.clone();
            let (sender, receiver) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("notebox-delete-attachment".to_string())
                .spawn(move || {
                    let result = sync::delete_attachment_blocking(&account, &session, &worker_hash)
                        .map_err(|error| sync::friendly_sync_error(&error));
                    let _ = sender.send(result);
                });
            match spawned {
                Ok(_) => self.runtime.borrow_mut().delete_rx = Some(receiver),
                Err(error) => self.attachment_delete_failed(format!(
                    "Could not start the delete request: {error}. Nothing was deleted."
                )),
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let context = self.clone();
            spawn_forever(async move {
                let result = client
                    .delete_attachment(&hash)
                    .await
                    .map_err(|error| sync::friendly_sync_error(&error));
                context.finish_cloud_attachment_delete(hash, result);
            });
        }
    }

    fn finish_cloud_attachment_delete(&self, hash: String, result: Result<(), String>) {
        match result {
            Ok(()) => self.finish_attachment_delete(&hash, true),
            Err(error) => self.attachment_delete_failed(format!(
                "The cloud copy could not be deleted: {error}. Nothing was removed from this device."
            )),
        }
    }

    /// Remove the reference, local bytes and row after any cloud delete.
    fn finish_attachment_delete(&self, hash: &str, cloud_deleted: bool) {
        let note_id = {
            let mut runtime = self.runtime.borrow_mut();
            runtime.delete_hash = None;
            runtime.delete_note.take()
        };
        let Some(note_id) = note_id else {
            self.attachment_delete_failed(
                "The delete request lost track of the note. Nothing was removed.".to_string(),
            );
            return;
        };
        let (name, draft, selected) = {
            let state = self.state.peek();
            (
                state
                    .attachments
                    .iter()
                    .find(|attachment| attachment.hash == hash)
                    .map(|attachment| attachment.name.clone())
                    .or_else(|| {
                        self.store()
                            .and_then(|store| store.attachment(hash).ok().flatten())
                            .map(|attachment| attachment.name)
                    })
                    .unwrap_or_else(|| hash.chars().take(8).collect()),
                state.draft.clone(),
                state.selected,
            )
        };

        // The gallery can close and another note can open while the cloud
        // request is in flight. Edit the note the attachment belonged to, not
        // whichever note happens to be open when the request returns.
        let saved = if selected == Some(note_id) {
            let edited = remove_attachment_references(&draft, hash);
            if edited != draft {
                self.note_input(edited);
                self.save_now();
            }
            if self.state.peek().dirty {
                Err("the note did not save".to_string())
            } else {
                Ok(())
            }
        } else {
            self.save_note_without_attachment(note_id, hash)
        };
        if let Err(error) = saved {
            self.attachment_delete_failed(format!(
                "Could not remove {name} from its note: {error}.{}",
                if cloud_deleted {
                    " The cloud copy is already gone; the next sync can upload it again from this device."
                } else {
                    ""
                }
            ));
            return;
        }
        let Some(store) = self.store() else {
            self.attachment_delete_failed(
                "The local store is not open. Nothing was deleted.".into(),
            );
            return;
        };
        if store.attachment_note_count(hash).unwrap_or(1) > 0 {
            self.attachment_delete_failed(format!(
                "Another note still uses {name}. It was not deleted.{}",
                if cloud_deleted {
                    " The cloud copy is already gone; the next sync can upload it again."
                } else {
                    ""
                }
            ));
            return;
        }
        match store.delete_attachment(hash) {
            Ok(true) => {
                let selected_now = self.state.peek().selected;
                {
                    let mut state = self.state.write_unchecked();
                    state.attachment_urls.remove(hash);
                    state.card_images.remove(hash);
                    state.attachment_deleting = None;
                    state.gallery_confirm_delete = false;
                }
                if selected_now == Some(note_id) {
                    self.load_attachments(note_id);
                }
                self.reload();
                self.toast_success(format!("Deleted {name}"));
            }
            Ok(false) => {
                self.attachment_delete_failed(format!(
                    "{name} could not be deleted.{}",
                    if cloud_deleted {
                        " The cloud copy is already gone; the next sync can upload it again."
                    } else {
                        ""
                    }
                ));
            }
            Err(error) => self.attachment_delete_failed(format!(
                "Could not delete {name}: {error}.{}",
                if cloud_deleted {
                    " The cloud copy is already gone; the next sync can upload it again."
                } else {
                    ""
                }
            )),
        }
    }

    /// Strip one attachment reference from a stored note that is not open.
    fn save_note_without_attachment(&self, note_id: NoteId, hash: &str) -> Result<(), String> {
        let Some(store) = self.store() else {
            return Err("the local store is not open".to_string());
        };
        let note = store
            .open_note(&note_id)
            .map_err(|error| format!("the note could not be loaded ({error})"))?;
        let text = note.text();
        let edited = remove_attachment_references(&text, hash);
        if edited == text {
            return Ok(());
        }
        let stamp = now();
        note.set_next_commit_timestamp(stamp);
        note.replace_text(&edited)
            .map_err(|error| format!("the edit could not be applied ({error})"))?;
        store
            .save_note(&note, stamp)
            .map(|_| ())
            .map_err(|error| format!("the note could not be saved ({error})"))
    }

    /// Save one attachment to Downloads (native) or the browser's downloads.
    pub fn download_attachment(&self, attachment: Attachment) {
        if !attachment.available {
            self.toast_info("That file has not downloaded yet; it will arrive with sync.");
            return;
        }
        let Some(store) = self.store() else {
            return;
        };
        match store.attachment_bytes(&attachment.hash) {
            Ok(Some(bytes)) => {
                self.deliver_attachment(&attachment.name, &attachment.mime, bytes);
            }
            Ok(None) => self.toast_info("That file has no local bytes to download."),
            Err(error) => self.toast_danger(format!("Could not read the file: {error}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn deliver_attachment(&self, name: &str, _mime: &str, bytes: Vec<u8>) {
        let directory = storage::export_directory();
        let path = unique_download_path(&directory, name);
        let result = std::fs::create_dir_all(&directory).and_then(|_| std::fs::write(&path, bytes));
        match result {
            Ok(()) => self.toast_success(format!("Downloaded to {}", path.display())),
            Err(error) => self.toast_danger(format!("Could not save the file: {error}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn deliver_attachment(&self, name: &str, mime: &str, bytes: Vec<u8>) {
        let encoded = serde_json::to_string(&notes_proto::Base64Bytes::new(bytes))
            .unwrap_or_else(|_| "\"\"".to_string());
        let name = serde_json::to_string(name).unwrap_or_else(|_| "\"attachment\"".to_string());
        let mime = serde_json::to_string(mime)
            .unwrap_or_else(|_| "\"application/octet-stream\"".to_string());
        let _ = document::eval(&format!(
            "const data = {encoded};\
             const bytes = Uint8Array.from(atob(data), c => c.charCodeAt(0));\
             const blob = new Blob([bytes], {{type: {mime}}});\
             const link = document.createElement('a');\
             link.href = URL.createObjectURL(blob);\
             link.download = {name};\
             link.click();\
             setTimeout(() => URL.revokeObjectURL(link.href), 1000);"
        ));
        self.toast_success("Download started");
    }

    /// Collapse or restore the sidebar on a wide window.
    pub fn toggle_sidebar_collapsed(&self) {
        let mut state = self.state.write_unchecked();
        state.sidebar_collapsed = !state.sidebar_collapsed;
    }

    /// Cycle the topbar theme button.
    ///
    /// `System` must resolve the operating-system preference first. Picking
    /// `Light` unconditionally made the first click a no-op on a light system.
    pub fn toggle_theme(&self) {
        let current = self.state.peek().theme;
        if current == Theme::System {
            let context = self.clone();
            spawn(async move {
                let system_dark = document::eval(
                    "return !!(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches);",
                )
                .join::<bool>()
                .await
                .unwrap_or(false);
                context.set_theme(next_theme(Theme::System, system_dark));
            });
            return;
        }
        self.set_theme(next_theme(current, false));
    }

    /// Pick a specific theme.
    pub fn set_theme(&self, theme: Theme) {
        {
            self.state.write_unchecked().theme = theme;
        }
        self.save_preferences();
    }

    /// Switch between cards and list.
    pub fn toggle_layout(&self) {
        {
            let mut state = self.state.write_unchecked();
            state.layout = match state.layout {
                Layout::Grid => Layout::List,
                Layout::List => Layout::Grid,
            };
        }
        self.save_preferences();
    }

    /// Set the card layout from Settings.
    pub fn set_layout(&self, layout: Layout) {
        {
            self.state.write_unchecked().layout = layout;
        }
        self.save_preferences();
    }

    /// Write the local display, onboarding and last-screen choices.
    fn save_preferences(&self) {
        let (
            theme,
            layout,
            offline,
            last_note,
            last_screen,
            last_view,
            mobile_list,
            browsing,
            confirm_delete,
        ) = {
            let state = self.state.read();
            (
                state.theme,
                state.layout,
                state.offline,
                state.selected,
                SavedScreen::from(state.screen),
                SavedView::from(state.view),
                state.mobile_list,
                state.browsing,
                state.confirm_delete,
            )
        };
        let preferences = crate::prefs::Preferences {
            theme,
            layout,
            offline,
            last_note,
            last_screen,
            last_view,
            mobile_list,
            browsing,
            confirm_delete,
        };
        if let Err(error) = crate::prefs::save(&preferences) {
            self.toast_danger(format!("Could not save preferences: {error}"));
        }
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
        // Copy the id out of the guard first: `toast_info` writes the signal,
        // and an `if let`/`let else` on `peek()` would still hold the read.
        let selected = self.state.peek().selected;
        let Some(note_id) = selected else {
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
            state.account_open = false;
            state.note_menu = None;
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

    /// Show or hide the account panel under the avatar.
    ///
    /// Opening it closes the navigation drawer and the colour menu, so only
    /// one transient surface owns the next tap.
    pub fn toggle_account_menu(&self) {
        let mut state = self.state.write_unchecked();
        let open = !state.account_open;
        state.account_open = open;
        if open {
            state.sidebar_open = false;
            state.color_open = false;
            state.note_menu = None;
        }
    }

    /// Open Settings and refresh the device list when connected.
    pub fn open_settings(&self) {
        self.go(Screen::Settings);
        if let Some(store) = self.store() {
            let usage = store.attachment_usage().unwrap_or_default();
            self.state.write_unchecked().attachment_usage = usage;
        }
        if self.state.peek().profile.sync_connected {
            self.refresh_devices();
            self.refresh_cloud_usage();
        }
    }

    /// Close overlays, or leave the editor on a phone.
    pub fn escape(&self) {
        let mut return_to_account = false;
        let mut return_to_menu_card = None;
        let mut clear_text_selection = false;
        let close_gallery = self.state.peek().gallery_open;
        if close_gallery {
            self.close_attachment_gallery();
            return;
        }
        {
            let mut state = self.state.write_unchecked();
            if let Some(menu) = state.note_menu.take() {
                return_to_menu_card = Some(menu.note_id);
            } else if state.pending_delete.is_some() {
                state.pending_delete = None;
            } else if state.palette_open {
                state.palette_open = false;
            } else if state.shortcuts_open {
                state.shortcuts_open = false;
            } else if state.account_open {
                state.account_open = false;
                return_to_account = true;
            } else if state.find_open {
                state.find_open = false;
            } else if state.sidebar_open {
                state.sidebar_open = false;
            } else if state.color_open {
                state.color_open = false;
            } else if state.history_open {
                hide_history(&mut state);
            } else if state.text_select_note.take().is_some() {
                clear_text_selection = true;
            } else if state.screen == Screen::Login && state.login_first_run {
                // The first launch needs an account or an explicit offline choice.
            } else if state.screen == Screen::Settings || state.screen == Screen::Login {
                state.screen = Screen::Notes;
                state.login_first_run = false;
                state.mobile_list = state.browsing || state.selected.is_none();
            } else if !state.mobile_list {
                state.mobile_list = true;
            }
        }
        if let Some(id) = return_to_menu_card {
            self.focus_note_card(id);
        }
        if return_to_account {
            let _ = document::eval("document.getElementById('account-button')?.focus();");
        }
        if clear_text_selection {
            let _ = document::eval("window.getSelection()?.removeAllRanges();");
        }
        self.save_preferences();
    }

    /// Open the command palette.
    pub fn open_palette(&self) {
        {
            let mut state = self.state.write_unchecked();
            state.palette_open = true;
            state.palette_query.clear();
            state.palette_index = 0;
            state.account_open = false;
            state.note_menu = None;
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

    /// Put the caret in note search, switching back to Notes when needed.
    pub fn focus_search(&self) {
        {
            let mut state = self.state.write_unchecked();
            if state.screen == Screen::Login {
                return;
            }
            if state.screen != Screen::Notes {
                state.screen = Screen::Notes;
                state.mobile_list = true;
                hide_history(&mut state);
                hide_editor_overlays(&mut state);
            }
            state.palette_open = false;
            state.shortcuts_open = false;
            state.sidebar_open = false;
            state.note_menu = None;
        }
        let _ = document::eval(
            "setTimeout(() => { const el = document.getElementById('global-search'); if (el) { el.focus(); el.select(); } }, 30);",
        );
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
        state.account_open = false;
        state.note_menu = None;
    }

    /// Expand the sign-in details form with the current username.
    pub fn open_credentials(&self) {
        if !self.state.peek().profile.sync_connected {
            return;
        }
        let username = self.state.peek().profile.username.clone();
        let mut state = self.state.write_unchecked();
        state.credentials = CredentialsForm {
            open: true,
            username,
            ..CredentialsForm::default()
        };
    }

    /// Collapse the form and drop every password field.
    pub fn close_credentials(&self) {
        self.state.write_unchecked().credentials = CredentialsForm::default();
    }

    /// Update the username field without touching the live account.
    pub fn set_credentials_username(&self, value: String) {
        self.state.write_unchecked().credentials.username = value;
    }

    /// Update the current-password field.
    pub fn set_credentials_current_password(&self, value: String) {
        self.state.write_unchecked().credentials.current_password = value;
    }

    /// Update the optional new-password field.
    pub fn set_credentials_new_password(&self, value: String) {
        self.state.write_unchecked().credentials.new_password = value;
    }

    /// Update the new-password confirmation.
    pub fn set_credentials_confirm(&self, value: String) {
        self.state.write_unchecked().credentials.confirm = value;
    }

    /// Validate and submit a username and/or password change.
    pub fn submit_credentials(&self) {
        if self.runtime.borrow().busy {
            self.toast_info("Wait for the current sync to finish.");
            return;
        }
        let (current_username, username, current_password, new_password, confirm) = {
            let state = self.state.read();
            (
                state.profile.username.trim().to_string(),
                state.credentials.username.trim().to_string(),
                state.credentials.current_password.clone(),
                state.credentials.new_password.clone(),
                state.credentials.confirm.clone(),
            )
        };
        let changed_username = !username.is_empty() && username != current_username;
        let changed_password = !new_password.is_empty();
        let message = if !self.state.peek().profile.sync_connected {
            Some("Sign in before changing the account details.".to_string())
        } else if username.is_empty() {
            Some("Enter a username.".to_string())
        } else if username.chars().count() > notes_proto::MAX_USERNAME_CHARS
            || username.chars().any(char::is_control)
        {
            Some(format!(
                "Use a username of 1 to {} visible characters.",
                notes_proto::MAX_USERNAME_CHARS
            ))
        } else if current_password.is_empty() {
            Some("Enter the current password.".to_string())
        } else if !changed_username && !changed_password {
            Some("Change the username or enter a new password.".to_string())
        } else if !changed_password && !confirm.is_empty() {
            Some("Enter a new password or leave both new fields blank.".to_string())
        } else if changed_password && new_password.chars().count() < notes_sync::MIN_PASSWORD_CHARS
        {
            Some(format!(
                "Use a new password of at least {} characters.",
                notes_sync::MIN_PASSWORD_CHARS
            ))
        } else if changed_password && new_password != confirm {
            Some("The new passwords do not match.".to_string())
        } else {
            None
        };
        if let Some(message) = message {
            self.state.write_unchecked().credentials.error = Some(message);
            return;
        }
        {
            let mut state = self.state.write_unchecked();
            state.credentials.busy = true;
            state.credentials.error = None;
            // Do not keep passwords in the rendered state while the request
            // is in flight; the worker owns its own copy.
            state.credentials.current_password.clear();
            state.credentials.new_password.clear();
            state.credentials.confirm.clear();
        }
        self.runtime.borrow_mut().busy = true;
        self.start_credentials_change(username, current_password, new_password);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_credentials_change(
        &self,
        new_username: String,
        current_password: String,
        new_password: String,
    ) {
        let (account, session) = {
            let runtime = self.runtime.borrow();
            (
                runtime.account.clone(),
                runtime.client.as_ref().and_then(|client| client.session()),
            )
        };
        let (Some(account), Some(session)) = (account, session) else {
            self.finish_credentials(Err(
                "This device is no longer signed in. Sign in again.".to_string()
            ));
            return;
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("notebox-credentials".to_string())
            .spawn(move || {
                let result = sync::change_credentials_blocking(
                    &account,
                    &session,
                    &current_password,
                    &new_username,
                    &new_password,
                )
                .map_err(|error| sync::friendly_credentials_error(&error));
                let _ = sender.send(result);
            });
        match spawned {
            Ok(_) => self.runtime.borrow_mut().change_rx = Some(receiver),
            Err(error) => self.finish_credentials(Err(format!(
                "Could not start the credential thread: {error}"
            ))),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_credentials_change(
        &self,
        new_username: String,
        current_password: String,
        new_password: String,
    ) {
        let client = self.runtime.borrow().client.clone();
        let Some(client) = client else {
            self.finish_credentials(Err(
                "This device is no longer signed in. Sign in again.".to_string()
            ));
            return;
        };
        let context = self.clone();
        spawn_forever(async move {
            let result = client
                .change_credentials(&current_password, &new_username, &new_password)
                .await
                .map_err(|error| sync::friendly_credentials_error(&error));
            context.finish_credentials(result);
        });
    }

    /// Apply the server's answer to the runtime, saved account and UI.
    fn finish_credentials(&self, result: Result<notes_proto::CredentialUpdateResponse, String>) {
        {
            let mut runtime = self.runtime.borrow_mut();
            runtime.busy = false;
            #[cfg(not(target_arch = "wasm32"))]
            {
                runtime.change_rx = None;
            }
        }
        let response = match result {
            Ok(response) => response,
            Err(message) => {
                let mut state = self.state.write_unchecked();
                state.credentials.busy = false;
                state.credentials.current_password.clear();
                state.credentials.new_password.clear();
                state.credentials.confirm.clear();
                state.credentials.error = Some(message);
                return;
            }
        };

        let new_username = response.username;
        let (account, old_session) = {
            let runtime = self.runtime.borrow();
            (
                runtime.account.clone(),
                runtime.client.as_ref().and_then(|client| client.session()),
            )
        };
        let Some(account) = account else {
            let mut state = self.state.write_unchecked();
            state.credentials.busy = false;
            state.credentials.error =
                Some("The account details changed, but this device has no saved account.".into());
            return;
        };
        let new_account = sync::SavedAccount {
            username: new_username.clone(),
            ..account
        };
        let mut warning = None;
        let mut updated_session = None;
        if let Some(session) = old_session {
            match sync::remember_session_username(&session, &new_username) {
                Ok(updated) => updated_session = Some(updated),
                Err(error) => {
                    warning = Some(format!(
                        "The account changed, but this device could not remember the new username: {error}"
                    ));
                    updated_session = Some(notes_sync::SyncSession {
                        username: new_username.clone(),
                        ..session
                    });
                }
            }
        }
        if let Err(error) = sync::save_account(&new_account) {
            warning = Some(format!(
                "The account changed, but this device could not remember it: {error}"
            ));
        }
        if let Some(session) = updated_session {
            match sync::client(
                &new_account.server,
                &new_account.username,
                &new_account.device,
                Some(session),
            ) {
                Ok(client) => self.runtime.borrow_mut().client = Some(client),
                Err(error) => {
                    warning = Some(format!(
                        "The account changed, but this device could not rebuild its session: {error}"
                    ));
                }
            }
        }
        self.runtime.borrow_mut().account = Some(new_account);
        {
            let mut state = self.state.write_unchecked();
            state.profile.username = new_username.clone();
            state.profile.sync_connected = true;
            state.login.username = new_username.clone();
            state.credentials = CredentialsForm::default();
            state.sync.status = SyncStatus::Idle;
            state.sync.error = None;
            state.sync.devices.clear();
            state.sync.devices_error = None;
            state.sync.confirm_revoke = None;
        }
        match warning {
            Some(message) => self.toast_danger(message),
            None => self.toast_success("Sign-in details updated. Other devices were signed out."),
        }
        self.refresh_devices();
    }

    /// Submit the login form and open a real session.
    pub fn submit_login(&self) {
        if self.runtime.borrow().busy {
            self.toast_info("Wait for the current sync to finish.");
            return;
        }
        let (username, password, server, device) = {
            let state = self.state.read();
            (
                state.login.username.trim().to_string(),
                state.login.password.clone(),
                state.login.server.trim().trim_end_matches('/').to_string(),
                state.login.device.trim().to_string(),
            )
        };
        if username.is_empty() || password.is_empty() || server.is_empty() || device.is_empty() {
            self.state.write_unchecked().login.error =
                Some("Enter a username, password, server URL and device name.".to_string());
            return;
        }
        {
            let mut state = self.state.write_unchecked();
            state.login.password.clear();
            state.login.error = None;
            // Keep the form on screen while connecting so the button and the
            // error live in one place.
            state.sync.status = SyncStatus::Connecting;
            state.sync.error = None;
        }
        self.runtime.borrow_mut().busy = true;
        self.start_login(server, username, device, password);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn start_login(&self, server: String, username: String, device: String, password: String) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("notebox-login".to_string())
            .spawn(move || {
                let result = sync::login_blocking(&server, &username, &device, &password)
                    .map_err(|error| sync::friendly_login_error(&error));
                let _ = sender.send(result);
            });
        match spawned {
            Ok(_) => self.runtime.borrow_mut().login_rx = Some(receiver),
            Err(error) => self.login_failed(format!("Could not start the login thread: {error}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start_login(&self, server: String, username: String, device: String, password: String) {
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
            match task_client.login(&password, &sessions).await {
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
            state.login.password.clear();
            state.login.error = None;
            state.sync.status = SyncStatus::Idle;
            state.sync.error = None;
            state.sync.devices.clear();
            state.sync.devices_error = None;
            state.sync.confirm_revoke = None;
            state.screen = Screen::Notes;
            state.login_first_run = false;
            state.offline = false;
            state.mobile_list = state.browsing || state.selected.is_none();
            hide_editor_overlays(&mut state);
        }
        self.save_preferences();
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
        state.login.password.clear();
        state.login.error = Some(message.clone());
        state.sync.status = SyncStatus::Local;
        state.sync.error = Some(message);
    }

    /// Continue without an account.
    pub fn continue_local(&self) {
        {
            let mut state = self.state.write_unchecked();
            state.login.password.clear();
            state.login.error = None;
            state.profile.sync_connected = false;
            state.sync.status = SyncStatus::Local;
            state.offline = true;
            state.login_first_run = false;
            state.screen = Screen::Notes;
            state.mobile_list = state.browsing || state.selected.is_none();
            hide_editor_overlays(&mut state);
        }
        self.save_preferences();
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
        if self.runtime.borrow().delete_hash.is_some() {
            if manual {
                self.toast_info("Wait for the image delete to finish.");
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

        let change = {
            let mut runtime = self.runtime.borrow_mut();
            match runtime.change_rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(value)) => {
                    runtime.change_rx = None;
                    Some(value)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    runtime.change_rx = None;
                    Some(Err(
                        "The credential thread stopped without an answer.".to_string()
                    ))
                }
                Some(Err(TryRecvError::Empty)) | None => None,
            }
        };
        if let Some(result) = change {
            self.finish_credentials(result);
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

        let usage = {
            let mut runtime = self.runtime.borrow_mut();
            match runtime.usage_rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(value)) => {
                    runtime.usage_rx = None;
                    Some(value)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    runtime.usage_rx = None;
                    Some(Err(
                        "The storage-check thread stopped without an answer.".to_string()
                    ))
                }
                Some(Err(TryRecvError::Empty)) | None => None,
            }
        };
        if let Some(result) = usage {
            self.finish_cloud_usage(result);
        }

        let deleted = {
            let mut runtime = self.runtime.borrow_mut();
            match runtime.delete_rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(value)) => {
                    runtime.delete_rx = None;
                    Some(value)
                }
                Some(Err(TryRecvError::Disconnected)) => {
                    runtime.delete_rx = None;
                    Some(Err(
                        "The delete thread stopped without an answer. Nothing was removed."
                            .to_string(),
                    ))
                }
                Some(Err(TryRecvError::Empty)) | None => None,
            }
        };
        if let Some(result) = deleted {
            let hash = self.runtime.borrow_mut().delete_hash.clone();
            match hash {
                Some(hash) => self.finish_cloud_attachment_delete(hash, result),
                None => self.attachment_delete_failed(
                    "The delete request finished without an attachment. Nothing was removed."
                        .to_string(),
                ),
            }
        }
    }

    /// The client loop: start, three seconds after an edit, every ten seconds
    /// while visible, and on focus or returning to the foreground. Never while
    /// hidden or dirty.
    fn maybe_sync(&self) {
        let visible = sync::document_visible();
        let focused = sync::document_focused();
        let (connected, busy, last_attempt, failures, retry_after, stopped, focus) = {
            let mut runtime = self.runtime.borrow_mut();
            if visible && !runtime.was_visible {
                runtime.focus_pending = true;
            }
            runtime.was_visible = visible;
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
            // Drop the read guard before `load_attachments` takes the write
            // guard. An `if let` on `peek()` keeps its temporary alive through
            // the body, which panicked the wasm build (AlreadyBorrowed) after
            // the first attachment download.
            let selected = self.state.peek().selected;
            if let Some(id) = selected {
                self.load_attachments(id);
            }
        }
        let history_open = self.state.peek().history_open;
        if history_open && report.outcome.pulled > 0 {
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
            .map(|summary| plain_line(&summary.title))
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
            id: None,
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

    /// Read the cloud-storage totals.
    ///
    /// Settings calls this when it opens and when Refresh is pressed; no timer
    /// polls it, so an idle app adds no D1 reads.
    pub fn refresh_cloud_usage(&self) {
        if self.state.peek().sync.usage_loading {
            return;
        }
        let client = self.runtime.borrow().client.clone();
        let Some(client) = client else {
            return;
        };
        {
            let mut state = self.state.write_unchecked();
            state.sync.usage_loading = true;
            state.sync.usage_error = None;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let account = self.runtime.borrow().account.clone();
            let session = client.session();
            let (Some(account), Some(session)) = (account, session) else {
                self.state.write_unchecked().sync.usage_loading = false;
                return;
            };
            let (sender, receiver) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("notebox-usage".to_string())
                .spawn(move || {
                    let result = sync::usage_blocking(&account, &session)
                        .map_err(|error| sync::friendly_sync_error(&error));
                    let _ = sender.send(result);
                });
            match spawned {
                Ok(_) => self.runtime.borrow_mut().usage_rx = Some(receiver),
                Err(error) => self
                    .finish_cloud_usage(Err(format!("Could not start the storage check: {error}"))),
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let context = self.clone();
            spawn_forever(async move {
                let result = client
                    .storage_usage()
                    .await
                    .map_err(|error| sync::friendly_sync_error(&error));
                context.finish_cloud_usage(result);
            });
        }
    }

    fn finish_cloud_usage(&self, result: Result<StorageUsage, String>) {
        let mut state = self.state.write_unchecked();
        state.sync.usage_loading = false;
        if !state.profile.sync_connected {
            return;
        }
        match result {
            Ok(usage) => {
                state.sync.usage = Some(usage);
                state.sync.usage_error = None;
                state.sync.usage_checked_at = Some(now());
            }
            Err(error) => state.sync.usage_error = Some(error),
        }
    }

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
        {
            let runtime = self.runtime.borrow();
            if runtime.busy {
                self.toast_info("Wait for the current sync to finish.");
                return;
            }
            if runtime.delete_hash.is_some() {
                self.toast_info("Wait for the image delete to finish.");
                return;
            }
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
            state.offline = true;
            state.login_first_run = false;
            state.login.username.clear();
            state.login.password.clear();
            state.login.error = None;
            state.login.server = server;
            state.login.device = device;
        }
        self.save_preferences();
        self.toast_info("Signed out. Your notes stay on this device.");
    }

    /// Read a picked backup and show what restoring it will do.
    pub fn choose_restore(&self, files: Vec<dioxus::html::FileData>) {
        let Some(file) = files.into_iter().next() else {
            return;
        };
        let name = file.name();
        let context = self.clone();
        spawn(async move {
            match read_file_bytes(&file).await {
                Ok(bytes) => context.inspect_restore(name, bytes),
                Err(error) => context.toast_danger(format!("Could not read {name}: {error}")),
            }
        });
    }

    fn inspect_restore(&self, name: String, bytes: Vec<u8>) {
        if bytes.is_empty() {
            self.toast_danger(format!("{name} is empty"));
            return;
        }
        if self.runtime.borrow().busy {
            self.toast_info("Wait for the current sync to finish, then restore the backup.");
            return;
        }
        match notes_core::read_export_zip(Cursor::new(&bytes)) {
            Ok(bundle) => {
                let has_history = bundle.notes.iter().any(|note| note.snapshot.is_some());
                let mut state = self.state.write_unchecked();
                state.pending_restore = Some(PendingRestore {
                    name,
                    bytes: Rc::new(bytes),
                    notes: bundle.notes.len(),
                    notebooks: bundle.notebooks.len(),
                    attachments: bundle.attachments.len(),
                    has_history,
                });
            }
            Err(error) => self.toast_danger(format!(
                "Could not read {name} as a Notebox backup: {error}"
            )),
        }
    }

    /// Dismiss the restore confirmation without changing anything.
    pub fn cancel_restore(&self) {
        self.state.write_unchecked().pending_restore = None;
    }

    /// Import the chosen backup. The store applies it in one transaction, so a
    /// failure leaves the database as it was.
    pub fn confirm_restore(&self) {
        let Some(pending) = self.state.peek().pending_restore.clone() else {
            return;
        };
        if self.runtime.borrow().busy {
            self.toast_info("Wait for the current sync to finish, then restore the backup.");
            return;
        }
        self.save_now();
        if self.state.peek().dirty {
            self.toast_danger(
                "The open note still has unsaved changes. Try restoring again in a moment.",
            );
            return;
        }
        let Some(store) = self.store() else {
            return;
        };
        match store.import_zip(pending.bytes.as_slice()) {
            Ok(report) => {
                {
                    let mut state = self.state.write_unchecked();
                    state.pending_restore = None;
                    hide_history(&mut state);
                }
                self.reload();
                let reopen = {
                    let state = self.state.peek();
                    state
                        .selected
                        .or_else(|| state.notes.first().map(|note| note.id))
                };
                if let Some(id) = reopen {
                    self.select_note(id);
                }
                self.toast_success(restore_summary(&report));
                // A failure here replaces the success toast with a storage
                // warning, which is the more important message.
                self.persist_if_needed();
            }
            Err(error) => {
                self.toast_danger(format!("Restore failed: {error}. Nothing was changed."))
            }
        }
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

    /// A waiting service worker announced a new web build.
    #[cfg(target_arch = "wasm32")]
    pub fn mark_update_ready(&self) {
        let mut state = self.state.write_unchecked();
        if !state.update_ready {
            state.update_ready = true;
            state.update_applying = false;
        }
    }

    /// Keep the current build until the next launch.
    pub fn dismiss_update(&self) {
        self.state.write_unchecked().update_dismissed = true;
    }

    /// Ask a waiting worker to activate. The page reloads from the controller
    /// change, so notes must already be saved.
    pub fn apply_update(&self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.save_now();
            let still_dirty = { self.state.read().dirty };
            if still_dirty {
                self.state.write_unchecked().update_applying = false;
                self.toast_info("Notebox is still saving this note. Try Reload again in a moment.");
                return;
            }
            if let Some(handle) = self.store.get() {
                if let Some(persistence) = &handle.persistence {
                    let _ = persistence.flush_now();
                }
            }
            self.state.write_unchecked().update_applying = true;
            crate::pwa::apply_update();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.dismiss_update();
        }
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

/// One line for the toast after a restore.
fn restore_summary(report: &ImportReport) -> String {
    let notes = report.notes_created + report.notes_updated;
    let mut text = format!("Restored {}", count_label(notes, "note", "notes"));
    if report.notes_updated > 0 {
        text.push_str(&format!(" ({} updated)", report.notes_updated));
    }
    if report.notebooks > 0 {
        text.push_str(&format!(
            ", {}",
            count_label(report.notebooks, "notebook", "notebooks")
        ));
    }
    if report.attachments > 0 {
        text.push_str(&format!(
            ", {}",
            count_label(report.attachments, "attachment", "attachments")
        ));
    }
    text
}

/// `1 note` or `2 notes`, for counts shown to a person.
fn count_label(value: usize, singular: &str, plural: &str) -> String {
    format!("{value} {}", if value == 1 { singular } else { plural })
}

/// Why an attachment delete cannot start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachmentDeleteBlocker {
    Shared(u64),
    SignIn,
    Offline,
    Busy,
}

/// Check the guards shared by the confirmation request and its final click.
///
/// Another note or a sync can appear while the confirmation is open, so both
/// points call this. A shared file ranks before a session problem because the
/// user can remove one reference and keep the bytes for the other note.
fn attachment_delete_blocker(
    uploaded: bool,
    count: u64,
    connected: bool,
    status: SyncStatus,
    busy: bool,
) -> Option<AttachmentDeleteBlocker> {
    if count > 1 {
        return Some(AttachmentDeleteBlocker::Shared(count));
    }
    if !connected && uploaded {
        return Some(AttachmentDeleteBlocker::SignIn);
    }
    if connected && matches!(status, SyncStatus::Offline | SyncStatus::NeedsLogin) {
        return Some(AttachmentDeleteBlocker::Offline);
    }
    if busy {
        return Some(AttachmentDeleteBlocker::Busy);
    }
    None
}

/// Pick a free file name in the download directory.
#[cfg(not(target_arch = "wasm32"))]
fn unique_download_path(directory: &std::path::Path, name: &str) -> std::path::PathBuf {
    let name = std::path::Path::new(name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("attachment");
    let first = directory.join(name);
    if !first.exists() {
        return first;
    }
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "attachment".to_string());
    let extension = path
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned());
    for number in 1..10_000 {
        let candidate = match extension.as_deref() {
            Some(extension) => format!("{stem} ({number}).{extension}"),
            None => format!("{stem} ({number})"),
        };
        let path = directory.join(candidate);
        if !path.exists() {
            return path;
        }
    }
    first
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

/// Close dismissible menus and overlays that belong to the current screen.
/// They must not survive a screen change or a login, where the user sees no
/// menu but every tap would still land on its backdrop.
fn hide_editor_overlays(state: &mut AppState) {
    state.account_open = false;
    state.note_menu = None;
    state.color_open = false;
    state.find_open = false;
    state.find_query.clear();
    state.find_index = 0;
    state.pending_delete = None;
    state.gallery_open = false;
    state.gallery_confirm_delete = false;
    state.gallery_note_count = 0;
    state.gallery_from_editor = false;
    state.gallery_editor_caret = None;
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
///
/// Summaries are cached in the store and may have been written by a build
/// that kept an image data URL in the title, so sanitise on display too.
pub fn note_title(note: &NoteSummary) -> String {
    let title = plain_line(&note.title);
    if title.trim().is_empty() {
        "Untitled note".to_string()
    } else {
        title
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

/// What the first screen should be after storage opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartupChoice {
    offline: bool,
    login_first_run: bool,
}

/// The next theme for a single toggle click.
///
/// Explicit themes swap; `System` moves to the opposite of the theme the OS is
/// currently showing, so the first click is always visible.
fn next_theme(current: Theme, system_dark: bool) -> Theme {
    match current {
        Theme::System if system_dark => Theme::Light,
        Theme::System => Theme::Dark,
        Theme::Dark => Theme::Light,
        Theme::Light => Theme::Dark,
    }
}

fn startup_choice(account_saved: bool, offline: bool, has_notes: bool) -> StartupChoice {
    // Notes written before the offline flag existed belong to the local user.
    // Mark them offline so a later launch does not ask for an account.
    let offline = offline || (!account_saved && has_notes);
    if account_saved || offline {
        StartupChoice {
            offline,
            login_first_run: false,
        }
    } else {
        StartupChoice {
            offline: false,
            login_first_run: true,
        }
    }
}

/// Pick the term to land on: the whole phrase when the body has it, else the
/// first word of the query the body contains.
fn best_body_term(text: &str, query: &str) -> Option<String> {
    let haystack = text.to_lowercase();
    let full = query.trim();
    if full.is_empty() {
        return None;
    }
    if haystack.contains(&full.to_lowercase()) {
        return Some(full.to_string());
    }
    notes_store::search_terms(query)
        .into_iter()
        .find(|term| haystack.contains(&term.to_lowercase()))
}

fn create_tour_note(
    store: &dyn NoteStore,
    text: &str,
    tags: &[&str],
    notebook_id: Option<NotebookId>,
    color: &str,
    created_at: i64,
    pinned: bool,
) -> Result<NoteId, String> {
    let note = store
        .create_note(NewNote {
            id: None,
            text: text.to_string(),
            created_at,
            notebook_id,
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        })
        .map_err(|error| format!("Could not add the welcome tour: {error}"))?;
    let id = note.id();
    let opened = store
        .open_note(&id)
        .map_err(|error| format!("Could not open a welcome note: {error}"))?;
    if pinned {
        let _ = opened.set_pinned(true);
    }
    let _ = opened.set_color(Some(color));
    store
        .save_note(&opened, created_at)
        .map_err(|error| format!("Could not save a welcome note: {error}"))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::{
        attachment_label, attachment_set_changed, best_body_term, find_match_count, next_theme,
        note_title, restore_summary, startup_choice, tone_class, word_count, PendingRestore,
    };
    use notes_proto::NoteId;
    use notes_store::{Attachment, ImportReport, NoteSummary};
    use std::rc::Rc;

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
    fn cached_image_titles_are_sanitised_on_display() {
        let note = NoteSummary {
            id: NoteId::new(),
            title: format!("![image](data:image/png;base64,{})", "A".repeat(500)),
            preview: String::new(),
            notebook_id: None,
            tags: Vec::new(),
            pinned: false,
            archived: false,
            color: None,
            created_at: 0,
            updated_at: 0,
            trashed_at: None,
            first_image: None,
        };
        assert_eq!(note_title(&note), "image");
    }

    #[test]
    fn restore_summary_counts_notes_notebooks_and_attachments() {
        assert_eq!(
            restore_summary(&ImportReport {
                notes_created: 2,
                notes_updated: 1,
                notebooks: 1,
                attachments: 4,
            }),
            "Restored 3 notes (1 updated), 1 notebook, 4 attachments"
        );
        assert_eq!(
            restore_summary(&ImportReport {
                notes_created: 1,
                ..ImportReport::default()
            }),
            "Restored 1 note"
        );
    }

    #[test]
    fn restore_confirmation_uses_singular_counts() {
        let pending = PendingRestore {
            name: "backup.zip".to_string(),
            bytes: Rc::new(Vec::new()),
            notes: 1,
            notebooks: 1,
            attachments: 1,
            has_history: true,
        };
        assert_eq!(pending.summary(), "1 note, 1 notebook, 1 attachment");
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
    fn search_landing_term_prefers_a_phrase_then_a_body_word() {
        assert_eq!(
            best_body_term("The quick brown fox", "brown fox"),
            Some("brown fox".to_string())
        );
        assert_eq!(
            best_body_term("The quick brown fox", "brown fox!"),
            Some("brown".to_string())
        );
        assert_eq!(best_body_term("Only a book tag", "books"), None);
        assert_eq!(best_body_term("anything", "#Books"), None);
    }

    #[test]
    fn a_system_theme_click_always_changes_the_visible_theme() {
        use crate::state::Theme;

        assert_eq!(next_theme(Theme::System, true), Theme::Light);
        assert_eq!(next_theme(Theme::System, false), Theme::Dark);
        assert_eq!(next_theme(Theme::Light, false), Theme::Dark);
        assert_eq!(next_theme(Theme::Dark, false), Theme::Light);
    }

    #[test]
    fn first_launch_asks_for_an_account_until_the_owner_chooses_offline() {
        let login = startup_choice(false, false, false);
        assert!(login.login_first_run);
        assert!(!login.offline);

        let chosen = startup_choice(false, true, false);
        assert!(!chosen.login_first_run);
        assert!(chosen.offline);

        let saved_account = startup_choice(true, false, false);
        assert!(!saved_account.login_first_run);
        assert!(!saved_account.offline);

        let old_notes = startup_choice(false, false, true);
        assert!(!old_notes.login_first_run);
        assert!(old_notes.offline);
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

    #[test]
    fn attachment_delete_guards_prefer_a_shared_file_then_the_session() {
        use super::{
            attachment_delete_blocker as guard, AttachmentDeleteBlocker as Blocker, SyncStatus,
        };

        assert_eq!(guard(false, 1, false, SyncStatus::Local, false), None);
        assert_eq!(
            guard(true, 1, false, SyncStatus::Local, false),
            Some(Blocker::SignIn)
        );
        assert_eq!(
            guard(true, 2, false, SyncStatus::Local, false),
            Some(Blocker::Shared(2))
        );
        assert_eq!(
            guard(false, 1, true, SyncStatus::Offline, false),
            Some(Blocker::Offline)
        );
        assert_eq!(
            guard(false, 1, true, SyncStatus::Idle, true),
            Some(Blocker::Busy)
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn download_names_cannot_escape_the_download_directory() {
        let directory = std::path::Path::new("/tmp/notebox-download-test");
        assert_eq!(
            super::unique_download_path(directory, "../../etc/passwd"),
            directory.join("passwd")
        );
        assert_eq!(
            super::unique_download_path(directory, "folder/photo.png"),
            directory.join("photo.png")
        );
        assert_eq!(
            super::unique_download_path(directory, ".."),
            directory.join("attachment")
        );
    }
}
