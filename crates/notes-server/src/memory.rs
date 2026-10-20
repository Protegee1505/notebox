use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::storage::{AccountRow, BlobMetaRow, DeviceRow, NewUpdate, Storage, UpdateRow};
use crate::StorageError;

/// In-memory [`Storage`] used by the handler tests.
///
/// It keeps the same ordering and idempotency rules as D1: updates get
/// increasing sequence numbers, and re-inserting a blob chunk replaces it.
#[derive(Debug, Default)]
pub struct MemoryStorage {
    state: Mutex<MemoryState>,
}

#[derive(Debug, Default)]
struct MemoryState {
    account: Option<AccountRow>,
    devices: Vec<DeviceRow>,
    updates: Vec<UpdateRow>,
    next_seq: i64,
    blob_meta: HashMap<String, BlobMetaRow>,
    blob_chunks: HashMap<(String, u32), Vec<u8>>,
}

impl MemoryStorage {
    /// An empty server.
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MemoryState {
                next_seq: 1,
                ..MemoryState::default()
            }),
        }
    }

    /// Number of stored updates, for tests.
    pub fn update_count(&self) -> usize {
        self.lock().updates.len()
    }

    /// Number of stored devices, for tests.
    pub fn device_count(&self) -> usize {
        self.lock().devices.len()
    }

    /// Number of stored attachment chunks, for tests.
    pub fn chunk_count(&self) -> usize {
        self.lock().blob_chunks.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryState> {
        self.state.lock().expect("memory storage lock")
    }
}

#[async_trait(?Send)]
impl Storage for MemoryStorage {
    async fn account(&self) -> Result<Option<AccountRow>, StorageError> {
        Ok(self.lock().account.clone())
    }

    async fn create_account(&self, account: &AccountRow) -> Result<bool, StorageError> {
        let mut state = self.lock();
        if state.account.is_some() {
            return Ok(false);
        }
        state.account = Some(account.clone());
        Ok(true)
    }

    async fn set_login_state(&self, failures: u32, blocked_until: i64) -> Result<(), StorageError> {
        let mut state = self.lock();
        let account = state
            .account
            .as_mut()
            .ok_or_else(|| StorageError::Failed("no account".into()))?;
        account.failed_logins = failures;
        account.blocked_until = blocked_until;
        Ok(())
    }

    async fn create_device(&self, device: &DeviceRow) -> Result<(), StorageError> {
        let mut state = self.lock();
        if state
            .devices
            .iter()
            .any(|existing| existing.token_hash == device.token_hash)
        {
            return Err(StorageError::Failed("duplicate token hash".into()));
        }
        state.devices.push(device.clone());
        Ok(())
    }

    async fn device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<DeviceRow>, StorageError> {
        Ok(self
            .lock()
            .devices
            .iter()
            .find(|device| device.token_hash == token_hash)
            .cloned())
    }

    async fn touch_device(&self, id: &str, at: i64) -> Result<(), StorageError> {
        if let Some(device) = self
            .lock()
            .devices
            .iter_mut()
            .find(|device| device.id == id)
        {
            device.last_seen = at;
        }
        Ok(())
    }

    async fn devices(&self) -> Result<Vec<DeviceRow>, StorageError> {
        Ok(self.lock().devices.clone())
    }

    async fn delete_device(&self, id: &str) -> Result<bool, StorageError> {
        let mut state = self.lock();
        let before = state.devices.len();
        state.devices.retain(|device| device.id != id);
        Ok(state.devices.len() != before)
    }

    async fn append_updates(
        &self,
        updates: &[NewUpdate],
        device_id: &str,
        at: i64,
    ) -> Result<(), StorageError> {
        let mut state = self.lock();
        for update in updates {
            let seq = state.next_seq;
            state.updates.push(UpdateRow {
                seq,
                note_id: update.note_id,
                blob: update.blob.clone(),
                device_id: device_id.to_string(),
                created_at: at,
            });
            state.next_seq += 1;
        }
        Ok(())
    }

    async fn updates_after(
        &self,
        cursor: i64,
        limit: usize,
    ) -> Result<Vec<UpdateRow>, StorageError> {
        Ok(self
            .lock()
            .updates
            .iter()
            .filter(|row| row.seq > cursor)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn blob_meta(&self, hash: &str) -> Result<Option<BlobMetaRow>, StorageError> {
        Ok(self.lock().blob_meta.get(hash).cloned())
    }

    async fn insert_blob_meta(&self, meta: &BlobMetaRow) -> Result<bool, StorageError> {
        let mut state = self.lock();
        if state.blob_meta.contains_key(&meta.hash) {
            return Ok(false);
        }
        state.blob_meta.insert(meta.hash.clone(), meta.clone());
        Ok(true)
    }

    async fn put_blob_chunk(
        &self,
        hash: &str,
        index: u32,
        data: &[u8],
    ) -> Result<(), StorageError> {
        self.lock()
            .blob_chunks
            .insert((hash.to_string(), index), data.to_vec());
        Ok(())
    }

    async fn blob_chunk(&self, hash: &str, index: u32) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self
            .lock()
            .blob_chunks
            .get(&(hash.to_string(), index))
            .cloned())
    }
}
