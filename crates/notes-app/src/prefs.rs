//! Display and onboarding preferences that belong to this device.
//!
//! Theme and layout are local by design: they are not part of a note, the
//! workspace document or the sync protocol. Native builds keep a small JSON
//! file next to the database; the web build uses `localStorage`, which is
//! available before the first render and is not cleared when OPFS is slow to
//! open.

use serde::{Deserialize, Serialize};

use crate::state::{Layout, Theme};

/// Browser key. A new shape gets a new key so an older build cannot read a
/// newer file as valid preferences.
#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "notebox.preferences.v1";

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
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            layout: Layout::Grid,
            offline: false,
        }
    }
}

fn default_theme() -> Theme {
    Theme::System
}

fn default_layout() -> Layout {
    Layout::Grid
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
        let wanted = Preferences {
            theme: Theme::Dark,
            layout: Layout::List,
            offline: true,
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
