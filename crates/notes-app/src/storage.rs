//! Where the app keeps its notes.
//!
//! Native builds open a SQLite file in the platform data directory and keep
//! the device session next to it. The web build opens a versioned SQLite image
//! from OPFS and writes it back after changes. If OPFS is unavailable, web
//! falls back to memory and says so in Settings; if the saved image is corrupt,
//! the app stops instead of replacing data it could not read.

use std::rc::Rc;

use notes_store::SqliteStore;

/// A storage backend plus the text Settings shows about it.
pub struct OpenedStore {
    /// The live note store.
    pub store: Rc<SqliteStore>,
    /// Browser image writer, when this build has one.
    pub persistence: Option<Rc<Persistence>>,
    /// Human-readable backend description.
    pub info: StorageInfo,
}

/// Why storage could not open.
#[allow(dead_code)] // Locked and Corrupt are produced by the browser backend only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenFailure {
    /// Another browser tab owns the saved database.
    Locked,
    /// A saved image exists but could not be read. It must not be overwritten.
    Corrupt(String),
    /// No backend opened at all.
    Failed(String),
}

impl OpenFailure {
    /// Message shown in the fatal screen.
    pub fn message(&self) -> String {
        match self {
            Self::Locked => {
                "Notebox is already open in another tab. Close that tab, then reload this one."
                    .to_string()
            }
            Self::Corrupt(message) | Self::Failed(message) => message.clone(),
        }
    }
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

/// Saves the browser database image after changes.
pub struct Persistence {
    #[cfg(target_arch = "wasm32")]
    inner: Rc<crate::browser_storage::BrowserPersistence>,
}

impl Persistence {
    #[cfg(target_arch = "wasm32")]
    fn new(inner: Rc<crate::browser_storage::BrowserPersistence>) -> Self {
        Self { inner }
    }

    /// Send a new image if the store changed and no save is in flight.
    pub fn flush_if_needed(&self) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.flush_if_needed(false)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(())
        }
    }

    /// Send the latest image even if an older save is in flight.
    ///
    /// Used from the page-hide handler, where the tab will not get another
    /// tick to retry.
    #[cfg(target_arch = "wasm32")]
    pub fn flush_now(&self) -> Result<(), String> {
        self.inner.flush_if_needed(true)
    }
}

/// Open the best store this build has, falling back to memory.
pub async fn open() -> Result<OpenedStore, OpenFailure> {
    open_platform().await
}

#[cfg(not(target_arch = "wasm32"))]
async fn open_platform() -> Result<OpenedStore, OpenFailure> {
    let path = database_path();
    match SqliteStore::open(&path) {
        Ok(store) => Ok(OpenedStore {
            store: Rc::new(store),
            persistence: None,
            info: StorageInfo {
                label: "SQLite".to_string(),
                detail: path.display().to_string(),
                temporary: false,
            },
        }),
        Err(error) => match SqliteStore::open_in_memory() {
            Ok(store) => Ok(OpenedStore {
                store: Rc::new(store),
                persistence: None,
                info: StorageInfo {
                    label: "Memory".to_string(),
                    detail: format!(
                        "Could not open {} ({error}). Notes last for this session only.",
                        path.display()
                    ),
                    temporary: true,
                },
            }),
            Err(fallback) => Err(OpenFailure::Failed(format!(
                "Could not open {} ({error}); the in-memory fallback failed too: {fallback}. Set NOTEBOX_DATA_DIR to a writable folder, then restart Notebox.",
                path.display()
            ))),
        },
    }
}

#[cfg(target_arch = "wasm32")]
async fn open_platform() -> Result<OpenedStore, OpenFailure> {
    use crate::browser_storage::{self, LoadError};

    match browser_storage::load().await {
        Ok((worker, bytes, persisted)) => {
            let opened = if bytes.is_empty() {
                SqliteStore::open_in_memory()
            } else {
                SqliteStore::from_image(&bytes)
            };
            match opened {
                Ok(store) => {
                    let store = Rc::new(store);
                    let persistence = worker.attach(store.clone());
                    let detail = if persisted {
                        "SQLite in this browser's OPFS. Notes, history and images survive a reload. Export a zip for backup."
                            .to_string()
                    } else {
                        "SQLite in this browser's OPFS. The browser may clear it when storage runs low; export a zip for backup and restore it in Settings."
                            .to_string()
                    };
                    Ok(OpenedStore {
                        store,
                        persistence: Some(Rc::new(Persistence::new(persistence))),
                        info: StorageInfo {
                            label: "Browser storage".to_string(),
                            detail,
                            temporary: false,
                        },
                    })
                }
                Err(error) => Err(OpenFailure::Corrupt(format!(
                    "The saved browser database could not be read ({error}). It has not been changed. Clear this site's data only if you have a zip export to restore."
                ))),
            }
        }
        Err(LoadError::Locked) => Err(OpenFailure::Locked),
        Err(LoadError::Unavailable(message)) => match SqliteStore::open_in_memory() {
            Ok(store) => Ok(OpenedStore {
                store: Rc::new(store),
                persistence: None,
                info: StorageInfo {
                    label: "Browser memory".to_string(),
                    detail: format!(
                        "Browser storage is unavailable ({message}). Notes reset when this page reloads; export a zip to keep them."
                    ),
                    temporary: true,
                },
            }),
            Err(error) => Err(OpenFailure::Failed(format!(
                "Browser storage is unavailable ({message}); the in-memory fallback failed too: {error}"
            ))),
        },
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
