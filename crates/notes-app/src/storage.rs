//! Where the app keeps its notes.
//!
//! Native builds open a SQLite file in the platform data directory and keep
//! the device session next to it. The web build still opens an in-memory
//! database: sync makes the notes come back from the Worker after a reload,
//! but the OPFS worker is the next persistence step, and saying so in Settings
//! is more useful than pretending the browser is persistent.

use notes_store::{NoteStore, SqliteStore};

/// A storage backend plus the text Settings shows about it.
pub struct OpenedStore {
    /// The live note store.
    pub store: Box<dyn NoteStore>,
    /// Human-readable backend description.
    pub info: StorageInfo,
}

/// What Settings says about local storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageInfo {
    /// Short backend name, such as `SQLite`.
    pub label: String,
    /// File path or browser caveat.
    pub detail: String,
    /// Whether data disappears when the app closes.
    pub temporary: bool,
}

/// Open the best store this build has, falling back to memory.
pub fn open() -> Result<OpenedStore, String> {
    open_platform()
}

#[cfg(not(target_arch = "wasm32"))]
fn open_platform() -> Result<OpenedStore, String> {
    let path = database_path();
    match SqliteStore::open(&path) {
        Ok(store) => Ok(OpenedStore {
            store: Box::new(store),
            info: StorageInfo {
                label: "SQLite".to_string(),
                detail: path.display().to_string(),
                temporary: false,
            },
        }),
        Err(error) => match SqliteStore::open_in_memory() {
            Ok(store) => Ok(OpenedStore {
                store: Box::new(store),
                info: StorageInfo {
                    label: "Memory".to_string(),
                    detail: format!(
                        "Could not open {} ({error}). Notes last for this session only.",
                        path.display()
                    ),
                    temporary: true,
                },
            }),
            Err(fallback) => Err(format!(
                "{error}; the in-memory fallback failed too: {fallback}"
            )),
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn open_platform() -> Result<OpenedStore, String> {
    match SqliteStore::open_in_memory() {
        Ok(store) => Ok(OpenedStore {
            store: Box::new(store),
            info: StorageInfo {
                label: "Browser memory".to_string(),
                detail: "Notes reset when this page reloads. Signed in, they come back from your Worker; the local OPFS database lands next."
                    .to_string(),
                temporary: true,
            },
        }),
        Err(error) => Err(format!("could not open the browser database: {error}")),
    }
}

/// Directory for a native export, or `None` in the browser.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_directory() -> std::path::PathBuf {
    if let Ok(value) = std::env::var("NOTEBOX_EXPORT_DIR") {
        return std::path::PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home).join("Downloads");
    }
    database_path()
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
}

/// Native data directory, honouring `NOTEBOX_DATA_DIR` for tests and portable
/// installs.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn data_directory() -> std::path::PathBuf {
    let directory = if let Some(value) = std::env::var_os("NOTEBOX_DATA_DIR") {
        std::path::PathBuf::from(value)
    } else if let Some(value) = std::env::var_os("XDG_DATA_HOME") {
        std::path::PathBuf::from(value).join("notebox")
    } else if let Some(home) = std::env::var_os("HOME") {
        std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("notebox")
    } else if let Ok(current) = std::env::current_dir() {
        current.join(".notebox")
    } else {
        std::env::temp_dir().join("notebox")
    };
    // The store reports a clear error if this fails; it must not stop the app
    // from opening when another directory is writable.
    let _ = std::fs::create_dir_all(&directory);
    directory
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn database_path() -> std::path::PathBuf {
    data_directory().join("notebox.db")
}

/// The device token file. Only the owner can read it.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn session_path() -> std::path::PathBuf {
    data_directory().join("session.json")
}

/// Server URL, username and device name. No secret lives here.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn account_path() -> std::path::PathBuf {
    data_directory().join("account.json")
}
