//! D1 implementation of [`Storage`].
//!
//! Two databases are bound in `wrangler.toml`: `DB_SYNC` for accounts, devices
//! and updates, and `DB_BLOBS` for attachment chunks. Keeping them apart means
//! photos can never fill the notes database.

use async_trait::async_trait;
use base64::Engine as _;
use notes_proto::NoteId;
use serde::Deserialize;
use worker::js_sys;
use worker::wasm_bindgen::{JsCast, JsValue};
use worker::D1Database;

use crate::storage::{AccountRow, BlobMetaRow, DeviceRow, NewUpdate, Storage, UpdateRow};
use crate::StorageError;

/// A [`Storage`] backed by the two D1 databases.
#[derive(Debug)]
pub struct D1Storage {
    sync: D1Database,
    blobs: D1Database,
}

impl D1Storage {
    /// Build a store from the two bindings in `wrangler.toml`.
    pub fn new(sync: D1Database, blobs: D1Database) -> Self {
        Self { sync, blobs }
    }
}

#[derive(Debug, Deserialize)]
struct AccountDb {
    username: String,
    auth_hash: String,
    created_at: i64,
    failed_logins: i64,
    blocked_until: i64,
}

#[derive(Debug, Deserialize)]
struct DeviceDb {
    id: String,
    name: String,
    token_hash: String,
    created_at: i64,
    last_seen: i64,
}

#[derive(Debug, Deserialize)]
struct UpdateDb {
    seq: i64,
    note_id: String,
    blob: String,
    device_id: String,
    created_at: i64,
}

#[derive(Debug, Deserialize)]
struct BlobMetaDb {
    hash: String,
    size: u64,
    mime: String,
    chunks: i64,
    created_at: i64,
}

fn text(value: &str) -> JsValue {
    JsValue::from_str(value)
}

fn integer(value: i64) -> JsValue {
    JsValue::from_f64(value as f64)
}

fn changes(result: &worker::D1Result) -> usize {
    result
        .meta()
        .ok()
        .flatten()
        .and_then(|meta| meta.changes)
        .unwrap_or(0)
}

fn bound_error(error: worker::Error) -> StorageError {
    StorageError::Failed(error.to_string())
}

#[async_trait(?Send)]
impl Storage for D1Storage {
    async fn account(&self) -> Result<Option<AccountRow>, StorageError> {
        let statement = self
            .sync
            .prepare(
                "SELECT username, auth_hash, created_at, failed_logins, blocked_until \
                 FROM account WHERE id = 1",
            )
            .bind(&[])?;
        let row = statement.first::<AccountDb>(None).await?;
        Ok(row.map(|row| AccountRow {
            username: row.username,
            auth_hash: row.auth_hash,
            created_at: row.created_at,
            failed_logins: row.failed_logins.max(0) as u32,
            blocked_until: row.blocked_until,
        }))
    }

    async fn create_account(&self, account: &AccountRow) -> Result<bool, StorageError> {
        let statement = self
            .sync
            .prepare(
                "INSERT OR IGNORE INTO account \
                 (id, username, auth_hash, created_at, failed_logins, blocked_until) \
                 VALUES (1, ?, ?, ?, ?, ?)",
            )
            .bind(&[
                text(&account.username),
                text(&account.auth_hash),
                integer(account.created_at),
                integer(i64::from(account.failed_logins)),
                integer(account.blocked_until),
            ])?;
        let result = statement.run().await?;
        Ok(changes(&result) > 0)
    }

    async fn set_login_state(&self, failures: u32, blocked_until: i64) -> Result<(), StorageError> {
        let statement = self
            .sync
            .prepare("UPDATE account SET failed_logins = ?, blocked_until = ? WHERE id = 1")
            .bind(&[integer(i64::from(failures)), integer(blocked_until)])?;
        statement.run().await?;
        Ok(())
    }

    async fn update_credentials(
        &self,
        username: &str,
        auth_hash: &str,
        keep_device: &str,
    ) -> Result<(), StorageError> {
        let account = self
            .sync
            .prepare(
                "UPDATE account \
                 SET username = ?, auth_hash = ?, failed_logins = 0, blocked_until = 0 \
                 WHERE id = 1",
            )
            .bind(&[text(username), text(auth_hash)])?;
        let devices = self
            .sync
            .prepare("DELETE FROM device WHERE id <> ?")
            .bind(&[text(keep_device)])?;
        // D1 runs a batch atomically; either the account and device list both
        // change or neither does.
        let results = self.sync.batch(vec![account, devices]).await?;
        for result in &results {
            if !result.success() {
                return Err(StorageError::Failed(
                    result
                        .error()
                        .unwrap_or_else(|| "credential update failed".into()),
                ));
            }
        }
        // The caller already read the account; a successful batch means both
        // statements ran. Do not use row-count changes: setting the same
        // username or hash again can report zero changed rows.
        Ok(())
    }

    async fn create_device(&self, device: &DeviceRow) -> Result<(), StorageError> {
        let statement = self
            .sync
            .prepare(
                "INSERT INTO device (id, name, token_hash, created_at, last_seen) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&[
                text(&device.id),
                text(&device.name),
                text(&device.token_hash),
                integer(device.created_at),
                integer(device.last_seen),
            ])?;
        statement.run().await.map_err(bound_error)?;
        Ok(())
    }

    async fn device_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<DeviceRow>, StorageError> {
        let statement = self
            .sync
            .prepare(
                "SELECT id, name, token_hash, created_at, last_seen \
                 FROM device WHERE token_hash = ?",
            )
            .bind(&[text(token_hash)])?;
        let row = statement.first::<DeviceDb>(None).await?;
        Ok(row.map(|row| DeviceRow {
            id: row.id,
            name: row.name,
            token_hash: row.token_hash,
            created_at: row.created_at,
            last_seen: row.last_seen,
        }))
    }

    async fn touch_device(&self, id: &str, at: i64) -> Result<(), StorageError> {
        let statement = self
            .sync
            .prepare("UPDATE device SET last_seen = ? WHERE id = ?")
            .bind(&[integer(at), text(id)])?;
        statement.run().await?;
        Ok(())
    }

    async fn devices(&self) -> Result<Vec<DeviceRow>, StorageError> {
        let statement = self
            .sync
            .prepare(
                "SELECT id, name, token_hash, created_at, last_seen \
                 FROM device ORDER BY created_at, id",
            )
            .bind(&[])?;
        let rows = statement.all().await?.results::<DeviceDb>()?;
        Ok(rows
            .into_iter()
            .map(|row| DeviceRow {
                id: row.id,
                name: row.name,
                token_hash: row.token_hash,
                created_at: row.created_at,
                last_seen: row.last_seen,
            })
            .collect())
    }

    async fn delete_device(&self, id: &str) -> Result<bool, StorageError> {
        let statement = self
            .sync
            .prepare("DELETE FROM device WHERE id = ?")
            .bind(&[text(id)])?;
        let result = statement.run().await?;
        Ok(changes(&result) > 0)
    }

    async fn append_updates(
        &self,
        updates: &[NewUpdate],
        device_id: &str,
        at: i64,
    ) -> Result<(), StorageError> {
        if updates.is_empty() {
            return Ok(());
        }
        let mut statements = Vec::with_capacity(updates.len());
        for update in updates {
            let blob = base64::engine::general_purpose::STANDARD.encode(&update.blob);
            let statement = self
                .sync
                .prepare(
                    "INSERT INTO update_log (note_id, blob, device_id, created_at) \
                     VALUES (?, ?, ?, ?)",
                )
                .bind(&[
                    text(&update.note_id.to_string()),
                    text(&blob),
                    text(device_id),
                    integer(at),
                ])?;
            statements.push(statement);
        }
        let results = self.sync.batch(statements).await?;
        for result in results {
            if !result.success() {
                return Err(StorageError::Failed(
                    result
                        .error()
                        .unwrap_or_else(|| "batch insert failed".into()),
                ));
            }
        }
        Ok(())
    }

    async fn updates_after(
        &self,
        cursor: i64,
        limit: usize,
    ) -> Result<Vec<UpdateRow>, StorageError> {
        let statement = self
            .sync
            .prepare(
                "SELECT seq, note_id, blob, device_id, created_at \
                 FROM update_log WHERE seq > ? ORDER BY seq LIMIT ?",
            )
            .bind(&[integer(cursor), integer(limit as i64)])?;
        let rows = statement.all().await?.results::<UpdateDb>()?;
        rows.into_iter()
            .map(|row| {
                let note_id = NoteId::parse(&row.note_id)
                    .map_err(|error| StorageError::Failed(format!("stored note id: {error}")))?;
                let blob = base64::engine::general_purpose::STANDARD
                    .decode(row.blob.as_bytes())
                    .map_err(|error| {
                        StorageError::Failed(format!("stored update is not base64: {error}"))
                    })?;
                Ok(UpdateRow {
                    seq: row.seq,
                    note_id,
                    blob,
                    device_id: row.device_id,
                    created_at: row.created_at,
                })
            })
            .collect()
    }

    async fn blob_meta(&self, hash: &str) -> Result<Option<BlobMetaRow>, StorageError> {
        let statement = self
            .blobs
            .prepare(
                "SELECT hash, size, mime, chunks, created_at \
                 FROM blob_meta WHERE hash = ?",
            )
            .bind(&[text(hash)])?;
        let row = statement.first::<BlobMetaDb>(None).await?;
        Ok(row.map(|row| BlobMetaRow {
            hash: row.hash,
            size: row.size,
            mime: row.mime,
            chunks: row.chunks.max(0) as u32,
            created_at: row.created_at,
        }))
    }

    async fn insert_blob_meta(&self, meta: &BlobMetaRow) -> Result<bool, StorageError> {
        let statement = self
            .blobs
            .prepare(
                "INSERT OR IGNORE INTO blob_meta (hash, size, mime, chunks, created_at) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&[
                text(&meta.hash),
                integer(meta.size as i64),
                text(&meta.mime),
                integer(i64::from(meta.chunks)),
                integer(meta.created_at),
            ])?;
        let result = statement.run().await?;
        Ok(changes(&result) > 0)
    }

    async fn put_blob_chunk(
        &self,
        hash: &str,
        index: u32,
        data: &[u8],
    ) -> Result<(), StorageError> {
        // D1 expects an ArrayBuffer for a BLOB column. Copying into a
        // Uint8Array gives it exactly that.
        let buffer = js_sys::Uint8Array::from(data).buffer();
        let statement = self
            .blobs
            .prepare("INSERT OR REPLACE INTO blob_chunk (hash, idx, data) VALUES (?, ?, ?)")
            .bind(&[text(hash), integer(i64::from(index)), buffer.into()])?;
        statement.run().await.map_err(bound_error)?;
        Ok(())
    }

    async fn blob_chunk(&self, hash: &str, index: u32) -> Result<Option<Vec<u8>>, StorageError> {
        let statement = self
            .blobs
            .prepare("SELECT data FROM blob_chunk WHERE hash = ? AND idx = ?")
            .bind(&[text(hash), integer(i64::from(index))])?;
        let rows = statement.raw_js_value().await?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let array = row
            .dyn_ref::<js_sys::Array>()
            .ok_or_else(|| StorageError::Failed("D1 did not return a row array".into()))?;
        let value = array.get(0);
        if value.is_null() || value.is_undefined() {
            return Ok(None);
        }
        let bytes = if value.is_instance_of::<js_sys::Uint8Array>() {
            js_sys::Uint8Array::from(value).to_vec()
        } else if value.is_instance_of::<js_sys::ArrayBuffer>() || js_sys::Array::is_array(&value) {
            js_sys::Uint8Array::new(&value).to_vec()
        } else {
            let kind = value
                .js_typeof()
                .as_string()
                .unwrap_or_else(|| "unknown".into());
            let constructor = js_sys::Reflect::get(&value, &JsValue::from_str("constructor"))
                .ok()
                .and_then(|value| {
                    js_sys::Reflect::get(&value, &JsValue::from_str("name"))
                        .ok()
                        .and_then(|name| name.as_string())
                })
                .unwrap_or_else(|| "unknown".into());
            return Err(StorageError::Failed(format!(
                "D1 returned an attachment chunk that is not binary (typeof {kind}, constructor {constructor})"
            )));
        };
        Ok(Some(bytes))
    }
}
