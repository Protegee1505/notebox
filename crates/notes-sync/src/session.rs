//! The device session: the token a device keeps between runs.
//!
//! The token is the whole account as far as this device is concerned, so it
//! lives outside the notes database and outside the export. Native builds keep
//! it in a file only the owner can read; the browser build has no token at all
//! because the Worker sets an `HttpOnly` cookie the script cannot see
//! ([`NullTokenStore`]). P4 moves the native file into the OS keystore.

use std::cell::RefCell;

use serde::{Deserialize, Serialize};

use crate::SyncError;

/// What a device keeps after a successful login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSession {
    /// Account name, needed again to derive the login key.
    pub username: String,
    /// Server-side id of this device, used by the device list.
    pub device_id: String,
    /// Bearer token sent with every later request.
    pub token: String,
}

/// Where a device keeps its bearer token.
pub trait TokenStore {
    /// The stored session, if any.
    fn load(&self) -> Result<Option<SyncSession>, SyncError>;

    /// Store a session, replacing any earlier one.
    fn save(&self, session: &SyncSession) -> Result<(), SyncError>;

    /// Forget the session.
    fn clear(&self) -> Result<(), SyncError>;
}

/// An in-memory session, for tests and headless clients.
///
/// A headless client that only needs one run can use this and never touch the
/// disk. Nothing here survives the process.
#[derive(Debug, Default)]
pub struct MemoryTokenStore {
    slot: RefCell<Option<SyncSession>>,
}

impl MemoryTokenStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }
}

impl TokenStore for MemoryTokenStore {
    fn load(&self) -> Result<Option<SyncSession>, SyncError> {
        Ok(self.slot.borrow().clone())
    }

    fn save(&self, session: &SyncSession) -> Result<(), SyncError> {
        *self.slot.borrow_mut() = Some(session.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), SyncError> {
        *self.slot.borrow_mut() = None;
        Ok(())
    }
}

/// A store that keeps nothing.
///
/// The web build uses this: the session is the Worker's `HttpOnly;
/// SameSite=Strict` cookie, which the browser sends by itself and the script
/// cannot read.
#[derive(Debug, Default)]
pub struct NullTokenStore;

impl TokenStore for NullTokenStore {
    fn load(&self) -> Result<Option<SyncSession>, SyncError> {
        Ok(None)
    }

    fn save(&self, _session: &SyncSession) -> Result<(), SyncError> {
        Ok(())
    }

    fn clear(&self) -> Result<(), SyncError> {
        Ok(())
    }
}

/// A session file that only its owner can read.
///
/// The write goes to a temporary file in the same directory and is then
/// renamed, so a crash mid-write cannot leave a half-written token behind.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct FileTokenStore {
    path: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileTokenStore {
    /// Store the session in `path`, creating its directory on first save.
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The file this store reads and writes.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn io(&self, error: std::io::Error) -> SyncError {
        SyncError::Session(format!("{}: {error}", self.path.display()))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TokenStore for FileTokenStore {
    fn load(&self) -> Result<Option<SyncSession>, SyncError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| SyncError::Session(format!("{}: {error}", self.path.display()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(self.io(error)),
        }
    }

    fn save(&self, session: &SyncSession) -> Result<(), SyncError> {
        let bytes = serde_json::to_vec(session)
            .map_err(|error| SyncError::Session(format!("{session:?}: {error}")))?;
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| self.io(error))?;
            }
        }
        let temporary = self.path.with_extension("tmp");
        write_private(&temporary, &bytes).map_err(|error| self.io(error))?;
        std::fs::rename(&temporary, &self.path).map_err(|error| self.io(error))
    }

    fn clear(&self) -> Result<(), SyncError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(self.io(error)),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::{MemoryTokenStore, SyncSession, TokenStore};

    fn session(token: &str) -> SyncSession {
        SyncSession {
            username: "alice".to_string(),
            device_id: "device-1".to_string(),
            token: token.to_string(),
        }
    }

    #[test]
    fn the_memory_store_round_trips_and_clears() {
        let store = MemoryTokenStore::new();
        assert_eq!(store.load().expect("load"), None);
        store.save(&session("token-a")).expect("save");
        assert_eq!(store.load().expect("load"), Some(session("token-a")));
        store.clear().expect("clear");
        assert_eq!(store.load().expect("load"), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_file_store_round_trips_and_keeps_the_token_private() {
        use super::FileTokenStore;

        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("nested").join("session.json");
        let store = FileTokenStore::new(&path);
        assert_eq!(store.load().expect("load"), None);

        store.save(&session("token-b")).expect("save");
        assert_eq!(store.load().expect("load"), Some(session("token-b")));
        assert!(!path.with_extension("tmp").exists(), "no temporary left");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "only the owner can read it");
        }

        store.clear().expect("clear");
        assert_eq!(store.load().expect("load"), None);
        store.clear().expect("clearing twice is fine");
    }
}
