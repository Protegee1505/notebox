//! The app's state and the actions the UI can take.
//!
//! State is plain data in one `Signal`; the store sits beside it behind a
//! `Rc<dyn NoteStore>`. Components read state and call these methods, which
//! keeps the view free of storage details and gives the browser worker in P7
//! one place to plug into.

use std::rc::Rc;

use dioxus::prelude::*;
use notes_core::{Note, NoteVersion};
use notes_proto::{NoteId, Notebook, NotebookId};
use notes_store::{NewNote, NoteQuery, NoteStore, NoteSummary, NoteView, SearchQuery, TagCount};
use web_time::Instant;

use crate::markdown_ops::{self, Mark};
use crate::storage::{self, StorageInfo};
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

/// The local profile badge. Sync arrives with P7.
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
    /// Stored only long enough to be discarded; no server call yet.
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
    /// Local profile.
    pub profile: Profile,
    /// Login form.
    pub login: LoginForm,
    /// Minute of the last clock tick, so relative times refresh.
    pub clock_minute: i64,
}

impl AppState {
    fn new(storage: StorageInfo, storage_error: Option<String>) -> Self {
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
            profile: Profile::default(),
            login: LoginForm {
                server: "https://notebox.example.workers.dev".to_string(),
                device: "This device".to_string(),
                ..LoginForm::default()
            },
            clock_minute: now() / 60,
        }
    }
}

/// Shared handle passed through the component tree.
#[derive(Clone)]
pub struct AppContext {
    store: Option<Rc<dyn NoteStore>>,
    /// Reactive app state.
    pub state: Signal<AppState>,
}

impl AppContext {
    /// Open storage and load the first screen.
    pub fn new() -> Self {
        let (store, info, error) = match storage::open() {
            Ok(opened) => (Some(Rc::from(opened.store)), opened.info, None),
            Err(error) => (
                None,
                StorageInfo {
                    label: "Unavailable".to_string(),
                    detail: error.clone(),
                    temporary: true,
                },
                Some(error),
            ),
        };
        let state = Signal::new(AppState::new(info, error));
        let context = Self { store, state };
        context.bootstrap();
        context
    }

    /// The store, when it opened.
    pub fn store(&self) -> Option<&dyn NoteStore> {
        self.store.as_deref()
    }

    fn bootstrap(&self) {
        let Some(store) = self.store() else {
            return;
        };
        match store.note_count() {
            Ok(0) => self.seed(store),
            Ok(_) => {}
            Err(error) => self.toast_danger(format!("Could not read notes: {error}")),
        }
        self.reload();
        let first = self.state.peek().notes.first().map(|note| note.id);
        if let Some(id) = first {
            self.select_note(id);
        }
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
        match store.open_note(&id) {
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
            }
            Err(error) => self.toast_danger(format!("Could not open note: {error}")),
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
                state.saved_text = draft;
                state.saved_at = stamp;
                state.seen_seq = state.edit_seq;
                let view = state.view;
                replace_summary(&mut state.notes, summary, view);
            }
            Err(message) => {
                let edit_seq = self.state.peek().edit_seq;
                self.state.write_unchecked().seen_seq = edit_seq;
                self.toast_danger(message);
            }
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
        let mut eval = document::eval(
            "const el = document.getElementById('note-editor');\
             return el ? [el.selectionStart, el.selectionEnd] : [0, 0];",
        );
        let context = self.clone();
        spawn(async move {
            let selection = match eval.recv::<(usize, usize)>().await {
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

    /// Put the caret or selection back after the text box re-renders.
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

    /// Open Settings.
    pub fn open_settings(&self) {
        self.go(Screen::Settings);
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

    /// Submit the login form. Sync is intentionally not wired in P4.
    pub fn submit_login(&self) {
        let (username, passphrase, server) = {
            let state = self.state.read();
            (
                state.login.username.trim().to_string(),
                state.login.passphrase.clone(),
                state.login.server.trim().to_string(),
            )
        };
        if username.is_empty() || passphrase.is_empty() || server.is_empty() {
            self.state.write_unchecked().login.error =
                Some("Enter a username, passphrase and server URL.".to_string());
            return;
        }
        {
            let mut state = self.state.write_unchecked();
            state.profile.username = username;
            state.profile.sync_connected = false;
            state.login.passphrase.clear();
            state.login.error = None;
            state.screen = Screen::Notes;
            state.mobile_list = state.selected.is_none();
        }
        self.toast_info("Local mode. Sync through your Worker arrives with P7.");
    }

    /// Continue without an account.
    pub fn continue_local(&self) {
        {
            let mut state = self.state.write_unchecked();
            state.login.passphrase.clear();
            state.login.error = None;
            state.screen = Screen::Notes;
            state.mobile_list = state.selected.is_none();
        }
        self.toast_info("Your notes stay on this device.");
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
    use super::{find_match_count, tone_class, word_count};

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
