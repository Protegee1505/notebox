//! Display and onboarding preferences that belong to this device.
//!
//! Theme and layout are local by design: they are not part of a note, the
//! workspace document or the sync protocol. Native builds keep a small JSON
//! file next to the database; the web build uses `localStorage`, which is
//! available before the first render and is not cleared when OPFS is slow to
//! open.

use notes_proto::NoteId;
use notes_store::NoteView;
use serde::{Deserialize, Serialize};

use crate::state::{Layout, Screen, Theme};

/// Browser key. A new shape gets a new key so an older build cannot read a
/// newer file as valid preferences.
#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "notebox.preferences.v1";

/// Which app screen to reopen. Login is never restored: it starts a new
/// device session and belongs to the first-run flow, not a saved workspace.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedScreen {
    /// Note list and editor.
    #[default]
    Notes,
    /// Preferences, account and storage.
    Settings,
}

impl From<Screen> for SavedScreen {
    fn from(screen: Screen) -> Self {
        match screen {
            Screen::Settings => SavedScreen::Settings,
            Screen::Notes | Screen::Login => SavedScreen::Notes,
        }
    }
}

impl From<SavedScreen> for Screen {
    fn from(screen: SavedScreen) -> Self {
        match screen {
            SavedScreen::Notes => Screen::Notes,
            SavedScreen::Settings => Screen::Settings,
        }
    }
}

/// The list slice to reopen. `All` is an internal query and falls back to the
/// normal Active view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedView {
    /// Not trashed and not archived.
    #[default]
    Active,
    /// Archived notes.
    Archived,
    /// Trashed notes.
    Trash,
}

impl From<NoteView> for SavedView {
    fn from(view: NoteView) -> Self {
        match view {
            NoteView::Archived => SavedView::Archived,
            NoteView::Trash => SavedView::Trash,
            NoteView::Active | NoteView::All => SavedView::Active,
        }
    }
}

impl From<SavedView> for NoteView {
    fn from(view: SavedView) -> Self {
        match view {
            SavedView::Active => NoteView::Active,
            SavedView::Archived => NoteView::Archived,
            SavedView::Trash => NoteView::Trash,
        }
    }
}

/// Preferences stored outside the note database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    /// Palette choice.
    #[serde(default = "default_theme")]
    pub theme: Theme,
    /// Cards or list.
    #[serde(default = "default_layout")]
    pub layout: Layout,
    /// The owner chose the local-only path (or signed out).
    #[serde(default)]
    pub offline: bool,
    /// Note that was open when the app last closed.
    #[serde(default)]
    pub last_note: Option<NoteId>,
    /// Screen to reopen.
    #[serde(default)]
    pub last_screen: SavedScreen,
    /// List slice to reopen.
    #[serde(default)]
    pub last_view: SavedView,
    /// Whether the phone list, rather than the editor, was visible.
    #[serde(default = "default_mobile_list")]
    pub mobile_list: bool,
    /// Whether Back had opened the wide note gallery.
    #[serde(default)]
    pub browsing: bool,
    /// Ask before moving a note to the trash.
    #[serde(default = "default_confirm_delete")]
    pub confirm_delete: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            layout: Layout::Grid,
            offline: false,
            last_note: None,
            last_screen: SavedScreen::Notes,
            last_view: SavedView::Active,
            mobile_list: true,
            browsing: false,
            confirm_delete: true,
        }
    }
}

fn default_theme() -> Theme {
    Theme::System
}

fn default_layout() -> Layout {
    Layout::Grid
}

fn default_mobile_list() -> bool {
    true
}

fn default_confirm_delete() -> bool {
    true
}

/// Read preferences, falling back to defaults when nothing valid is stored.
pub fn load() -> Preferences {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(storage) =
            web_sys::window().and_then(|window| window.local_storage().ok().flatten())
        else {
            return Preferences::default();
        };
        let Some(text) = storage.get_item(STORAGE_KEY).ok().flatten() else {
            return Preferences::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        load_from(&crate::storage::preferences_path())
    }
}

/// Write preferences. A failure leaves the previous file untouched.
pub fn save(preferences: &Preferences) -> Result<(), String> {
    let bytes = serde_json::to_vec(preferences).map_err(|error| error.to_string())?;
    #[cfg(target_arch = "wasm32")]
    {
        let storage = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .ok_or_else(|| "this browser does not allow local storage".to_string())?;
        let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
        storage
            .set_item(STORAGE_KEY, &text)
            .map_err(|_| "could not save preferences in this browser".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::sync::write_atomic(&crate::storage::preferences_path(), &bytes)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn load_from(path: &std::path::Path) -> Preferences {
    let Ok(bytes) = std::fs::read(path) else {
        return Preferences::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_and_missing_fields_get_defaults() {
        let directory =
            std::env::temp_dir().join(format!("notebox-prefs-{}", uuid::Uuid::new_v4()));
        let path = directory.join("preferences.json");
        let note = NoteId::new();
        let wanted = Preferences {
            theme: Theme::Dark,
            layout: Layout::List,
            offline: true,
            last_note: Some(note),
            last_screen: SavedScreen::Settings,
            last_view: SavedView::Trash,
            mobile_list: false,
            browsing: true,
            confirm_delete: false,
        };
        save_to(&path, &wanted).expect("save");
        assert_eq!(load_from(&path), wanted);

        std::fs::write(&path, br#"{"theme":"dark"}"#).expect("write legacy");
        assert_eq!(
            load_from(&path),
            Preferences {
                theme: Theme::Dark,
                layout: Layout::Grid,
                offline: false,
                last_note: None,
                last_screen: SavedScreen::Notes,
                last_view: SavedView::Active,
                mobile_list: true,
                browsing: false,
                confirm_delete: true,
            }
        );

        std::fs::write(&path, b"not json").expect("write corrupt");
        assert_eq!(load_from(&path), Preferences::default());
        let _ = std::fs::remove_dir_all(directory);
    }

    fn save_to(path: &std::path::Path, preferences: &Preferences) -> Result<(), String> {
        let bytes = serde_json::to_vec(preferences).map_err(|error| error.to_string())?;
        crate::sync::write_atomic(path, &bytes)
    }
}
