//! SQLite backend: schema, migrations and the [`NoteStore`] implementation.
//!
//! The same code runs natively through `rusqlite` and in the browser through
//! `sqlite-wasm-rs` over the OPFS SAH-pool VFS. One connection per worker keeps
//! the rollback journal simple, as the S4 probe showed (ADR-0004).
//!
//! The `note` table holds the CRDT snapshot and the columns the list view
//! needs. `note_tag` and the FTS5 index are queryable mirrors of the note's own
//! metadata: the CRDT stays the source of truth, and saving a note rewrites its
//! rows in one transaction.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use notes_core::{
    content_hash, decode_version_vector, encode_version_vector, normalize_file_name,
    normalize_tags, read_export_zip, references, write_export_zip, ExportAttachment, ExportBundle,
    ExportNote, Note, NoteMeta, VersionVector, WorkspaceDocument,
};
use notes_proto::{NoteId, Notebook, NotebookId};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use crate::error::StoreError;
use crate::{
    Attachment, AttachmentUsage, ImportReport, NewNote, NoteQuery, NoteStore, NoteSummary,
    NoteView, SearchHit, SearchQuery, SyncState, TagCount, WorkspacePush,
};

/// Schema version written to `PRAGMA user_version`.
pub const SCHEMA_VERSION: i32 = 4;

/// Workspace columns read by [`NoteStore::workspace_pending`]:
/// snapshot, local version vector, last pushed version vector.
type WorkspaceRow = (Vec<u8>, Option<Vec<u8>>, Option<Vec<u8>>);

/// Database file name used by native builds and by the browser build.
pub const APP_DB_NAME: &str = "notebox.db";

/// Header in front of a serialized database image.
///
/// The body is a normal SQLite database file. The header lets the browser
/// reject a truncated or unrelated file before SQLite reads it. A future
/// worker-backed store can strip the four bytes and open the body directly.
pub const IMAGE_MAGIC: &[u8; 4] = b"NBX1";

/// OPFS directory that holds the browser database.
pub const BROWSER_DB_DIR: &str = "notebox";

/// Tables, indexes and the full-text index.
///
/// `note.seq` is the integer rowid; the FTS row for a note uses the same
/// number, so updating or deleting one note touches only its own FTS rows.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS note (
    seq         INTEGER PRIMARY KEY,
    id          TEXT NOT NULL UNIQUE,
    snapshot    BLOB NOT NULL,
    title       TEXT NOT NULL,
    preview     TEXT NOT NULL,
    notebook_id TEXT,
    pinned      INTEGER NOT NULL DEFAULT 0,
    archived    INTEGER NOT NULL DEFAULT 0,
    color       TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    trashed_at  INTEGER NOT NULL DEFAULT 0,
    last_pushed BLOB,
    local_vv    BLOB
);
CREATE INDEX IF NOT EXISTS note_list ON note (trashed_at, archived, updated_at DESC);
CREATE INDEX IF NOT EXISTS note_notebook ON note (notebook_id);

CREATE TABLE IF NOT EXISTS note_tag (
    note_id  TEXT NOT NULL,
    tag      TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (note_id, tag)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS note_tag_tag ON note_tag (tag);

CREATE VIRTUAL TABLE IF NOT EXISTS note_fts USING fts5 (
    note_id UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE IF NOT EXISTS notebook (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS workspace (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    snapshot    BLOB NOT NULL,
    local_vv    BLOB,
    last_pushed BLOB
);

CREATE TABLE IF NOT EXISTS attachment (
    hash        TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    mime        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    data        BLOB,
    created_at  INTEGER NOT NULL,
    uploaded_at INTEGER
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS attachment_upload ON attachment (uploaded_at);

-- Which notes use an attachment. The note's Markdown is the source of truth;
-- this table is rebuilt from it whenever the note is saved.
CREATE TABLE IF NOT EXISTS note_attachment (
    note_id  TEXT NOT NULL,
    hash     TEXT NOT NULL,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (note_id, hash)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS note_attachment_hash ON note_attachment (hash);

CREATE TABLE IF NOT EXISTS sync_state (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    cursor     INTEGER,
    updated_at INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO sync_state (id, cursor, updated_at) VALUES (1, NULL, 0);

-- One random Loro peer id per database, so edits made after a reload still
-- belong to this device in the note history.
CREATE TABLE IF NOT EXISTS device (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    peer_id INTEGER NOT NULL
);
"#;

/// Schema v1 to v2: per-row sync bookkeeping and an integer pull cursor.
///
/// `local_vv` and `last_pushed` are encoded Loro version vectors kept as
/// bytes: the sync engine compares them, so it can find unsynced notes
/// without parsing every snapshot. A row written by v1 has no `local_vv`,
/// which reads as "not synced" — the engine then pushes it once and fills
/// the column, so an upgraded database heals itself.
const MIGRATION_V2: &str = r#"
ALTER TABLE note ADD COLUMN local_vv BLOB;
ALTER TABLE workspace ADD COLUMN local_vv BLOB;
ALTER TABLE workspace ADD COLUMN last_pushed BLOB;

CREATE TABLE sync_state_v2 (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    cursor     INTEGER,
    updated_at INTEGER NOT NULL DEFAULT 0
);
INSERT INTO sync_state_v2 (id, cursor, updated_at)
    SELECT id, NULL, updated_at FROM sync_state;
DROP TABLE sync_state;
ALTER TABLE sync_state_v2 RENAME TO sync_state;
"#;

/// Schema v2 to v3: one stable Loro peer id per device.
///
/// The row is created on the next open by [`load_or_create_device_peer`], so
/// an upgraded database starts using a stable peer without rewriting any note.
const MIGRATION_V3: &str = r#"
CREATE TABLE IF NOT EXISTS device (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    peer_id INTEGER NOT NULL
);
"#;

/// Schema v3 to v4: content-addressed attachments with their bytes.
///
/// P1 stored attachment metadata only, and the bytes never existed, so the old
/// table is renamed rather than rewritten. Nothing in a note can refer to one
/// of those rows: note links arrive with P6. Keeping the table means a
/// migration never destroys what an older build recorded.
const MIGRATION_V4: &str = r#"
ALTER TABLE attachment RENAME TO attachment_legacy;
CREATE TABLE attachment (
    hash        TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    mime        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    data        BLOB,
    created_at  INTEGER NOT NULL,
    uploaded_at INTEGER
) WITHOUT ROWID;
CREATE INDEX attachment_upload ON attachment (uploaded_at);
CREATE TABLE note_attachment (
    note_id  TEXT NOT NULL,
    hash     TEXT NOT NULL,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (note_id, hash)
) WITHOUT ROWID;
CREATE INDEX note_attachment_hash ON note_attachment (hash);
"#;

const UPSERT_NOTE: &str = r#"
INSERT INTO note (
    id, snapshot, title, preview, notebook_id, pinned, archived, color,
    created_at, updated_at, trashed_at, local_vv
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
ON CONFLICT(id) DO UPDATE SET
    snapshot    = excluded.snapshot,
    title       = excluded.title,
    preview     = excluded.preview,
    notebook_id = excluded.notebook_id,
    pinned      = excluded.pinned,
    archived    = excluded.archived,
    color       = excluded.color,
    created_at  = excluded.created_at,
    updated_at  = excluded.updated_at,
    trashed_at  = excluded.trashed_at,
    local_vv    = excluded.local_vv
"#;

const SUMMARY_COLUMNS: &str = "n.id, n.title, n.preview, n.notebook_id, n.pinned, n.archived, \
     n.color, n.created_at, n.updated_at, n.trashed_at";

/// A note store backed by one SQLite connection.
pub struct SqliteStore {
    conn: RefCell<Connection>,
    /// Stable Loro peer id for changes this database makes.
    peer_id: u64,
}

impl SqliteStore {
    /// Open (or create) a database file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::from_connection(Connection::open(path)?)
    }

    /// Open an in-memory database. Used by tests.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    /// Take ownership of an opened connection and run the migrations.
    pub fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        #[cfg(not(target_arch = "wasm32"))]
        {
            // One connection per process, but WAL survives a crash without
            // rewriting the whole journal on every debounced save.
            let _ = conn.pragma_update(None, "journal_mode", "WAL");
        }
        #[cfg(target_arch = "wasm32")]
        {
            // The SAH-pool VFS has no shared memory for WAL.
            conn.pragma_update(None, "journal_mode", "delete")?;
        }

        migrate(&conn)?;
        let peer_id = load_or_create_device_peer(&conn)?;
        Ok(Self {
            conn: RefCell::new(conn),
            peer_id,
        })
    }

    /// Stable Loro peer id this database uses for new changes.
    pub fn device_peer_id(&self) -> u64 {
        self.peer_id
    }

    /// Count of rows changed on this connection.
    ///
    /// The browser build uses this to notice saves, pulls and attachment
    /// writes without marking every store method. Reads do not move it.
    pub fn change_count(&self) -> u64 {
        self.conn.borrow().total_changes()
    }

    /// Serialize the whole database with the [`IMAGE_MAGIC`] header.
    ///
    /// This is intended for the browser's in-memory database, which uses a
    /// rollback journal and has no separate WAL file to miss. A native file
    /// store in WAL mode is not a safe image source.
    pub fn to_image(&self) -> Result<Vec<u8>, StoreError> {
        let conn = self.conn.borrow();
        let data = conn.serialize(rusqlite::MAIN_DB)?;
        let mut image = Vec::with_capacity(IMAGE_MAGIC.len() + data.len());
        image.extend_from_slice(IMAGE_MAGIC);
        image.extend_from_slice(&data);
        Ok(image)
    }

    /// Load a database written by [`SqliteStore::to_image`].
    ///
    /// The image is copied into a fresh in-memory connection, then the normal
    /// migrations and device-peer lookup run. An image from an older schema is
    /// therefore migrated on open.
    pub fn from_image(bytes: &[u8]) -> Result<Self, StoreError> {
        let body = bytes.strip_prefix(IMAGE_MAGIC).ok_or_else(|| {
            StoreError::Corrupt("database image does not start with NBX1".to_string())
        })?;
        if body.is_empty() {
            return Err(StoreError::Corrupt("database image is empty".to_string()));
        }
        let mut conn = Connection::open_in_memory()?;
        conn.deserialize_read_exact(rusqlite::MAIN_DB, body, body.len(), false)?;
        Self::from_connection(conn)
    }

    fn connection(&self) -> std::cell::RefMut<'_, Connection> {
        self.conn.borrow_mut()
    }
}

/// Install the OPFS SAH-pool VFS. Call once, before [`open_browser`].
#[cfg(target_arch = "wasm32")]
pub async fn install_browser_vfs(directory: &str) -> Result<(), StoreError> {
    use sqlite_wasm_rs::WasmOsCallback;
    use sqlite_wasm_vfs::sahpool::{install, OpfsSAHPoolCfgBuilder};

    let config = OpfsSAHPoolCfgBuilder::new()
        .directory(directory)
        .initial_capacity(6)
        .build();
    install::<WasmOsCallback>(&config, true)
        .await
        .map(|_| ())
        .map_err(|error| StoreError::Browser(error.to_string()))
}

/// Open the browser database. [`install_browser_vfs`] must have run first.
#[cfg(target_arch = "wasm32")]
pub fn open_browser(file_name: &str) -> Result<SqliteStore, StoreError> {
    SqliteStore::from_connection(Connection::open(file_name)?)
}

impl NoteStore for SqliteStore {
    fn create_note(&self, new_note: NewNote) -> Result<Note, StoreError> {
        let meta = NoteMeta {
            notebook_id: new_note.notebook_id,
            tags: normalize_tags(new_note.tags),
            created_at: new_note.created_at,
            ..NoteMeta::default()
        };
        // `NoteId::default()` is a fresh random id, not a nil UUID.
        let note = Note::empty(new_note.id.unwrap_or_default());
        note.set_peer_id(self.peer_id)?;
        note.set_meta(&meta)?;
        if !new_note.text.is_empty() {
            note.insert(0, &new_note.text)?;
        }
        if new_note.created_at != 0 {
            note.set_next_commit_timestamp(new_note.created_at);
        }
        note.commit();
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        save_note_with(&tx, &note, new_note.created_at)?;
        tx.commit()?;
        Ok(note)
    }

    fn open_note(&self, id: &NoteId) -> Result<Note, StoreError> {
        let conn = self.conn.borrow();
        load_note(&conn, id, self.peer_id)?.ok_or(StoreError::NoteNotFound(*id))
    }

    fn save_note(&self, note: &Note, updated_at: i64) -> Result<NoteSummary, StoreError> {
        // Persist the edit as one change at the time the caller saved it. A
        // pending, uncommitted insert cannot be exported as a sync update, so
        // saving without this commit would queue a change the server never
        // receives.
        note.commit_at(updated_at);
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        let summary = save_note_with(&tx, note, updated_at)?;
        tx.commit()?;
        Ok(summary)
    }

    fn note_summary(&self, id: &NoteId) -> Result<NoteSummary, StoreError> {
        let conn = self.conn.borrow();
        let mut summaries = list_with(
            &conn,
            &NoteQuery {
                view: NoteView::All,
                limit: 0,
                ..NoteQuery::default()
            },
            Some(*id),
        )?;
        summaries.pop().ok_or(StoreError::NoteNotFound(*id))
    }

    fn list_notes(&self, query: &NoteQuery) -> Result<Vec<NoteSummary>, StoreError> {
        let conn = self.conn.borrow();
        list_with(&conn, query, None)
    }

    fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>, StoreError> {
        let conn = self.conn.borrow();
        search_with(&conn, query)
    }

    fn trash_note(&self, id: &NoteId, at: i64) -> Result<bool, StoreError> {
        let note = self.open_note(id)?;
        if !note.trash(at)? {
            return Ok(false);
        }
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        save_note_with(&tx, &note, at)?;
        tx.commit()?;
        Ok(true)
    }

    fn restore_note(&self, id: &NoteId, at: i64) -> Result<bool, StoreError> {
        let note = self.open_note(id)?;
        if !note.restore()? {
            return Ok(false);
        }
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        save_note_with(&tx, &note, at)?;
        tx.commit()?;
        Ok(true)
    }

    fn empty_trash(&self) -> Result<usize, StoreError> {
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM note WHERE trashed_at <> 0",
            [],
            |row| row.get(0),
        )?;
        tx.execute(
            "DELETE FROM note_fts WHERE rowid IN (SELECT seq FROM note WHERE trashed_at <> 0)",
            [],
        )?;
        tx.execute(
            "DELETE FROM note_tag WHERE note_id IN (SELECT id FROM note WHERE trashed_at <> 0)",
            [],
        )?;
        tx.execute(
            "DELETE FROM note_attachment
             WHERE note_id IN (SELECT id FROM note WHERE trashed_at <> 0)",
            [],
        )?;
        tx.execute("DELETE FROM note WHERE trashed_at <> 0", [])?;
        tx.commit()?;
        Ok(count.max(0) as usize)
    }

    fn note_count(&self) -> Result<u64, StoreError> {
        let conn = self.conn.borrow();
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM note WHERE trashed_at = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u64)
    }

    fn put_attachment(
        &self,
        name: &str,
        mime: &str,
        bytes: &[u8],
        at: i64,
    ) -> Result<Attachment, StoreError> {
        let hash = content_hash(bytes);
        let name = normalize_file_name(name, mime);
        let mime = normalized_mime(mime);
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO attachment (hash, name, mime, size, data, created_at, uploaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(hash) DO UPDATE SET
                 data = excluded.data,
                 size = excluded.size,
                 name = CASE WHEN attachment.data IS NULL THEN excluded.name ELSE attachment.name END,
                 mime = CASE WHEN attachment.data IS NULL THEN excluded.mime ELSE attachment.mime END",
            params![hash, name, mime, bytes.len() as i64, bytes, at],
        )?;
        let attachment = read_attachment(&tx, &hash)?.ok_or_else(|| {
            StoreError::Corrupt(format!("attachment {hash} vanished after insert"))
        })?;
        tx.commit()?;
        Ok(attachment)
    }

    fn attachment(&self, hash: &str) -> Result<Option<Attachment>, StoreError> {
        let conn = self.conn.borrow();
        read_attachment(&conn, &hash.to_ascii_lowercase())
    }

    fn attachment_bytes(&self, hash: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let conn = self.conn.borrow();
        let row: Option<(i64, Option<Vec<u8>>)> = conn
            .query_row(
                "SELECT size, data FROM attachment WHERE hash = ?1",
                params![hash.to_ascii_lowercase()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(None),
            Some((_, None)) => Ok(None),
            Some((size, Some(data))) => {
                if data.len() as i64 != size {
                    return Err(StoreError::Corrupt(format!(
                        "attachment {hash} holds {} bytes but records {size}",
                        data.len()
                    )));
                }
                Ok(Some(data))
            }
        }
    }

    fn note_attachments(&self, note: &NoteId) -> Result<Vec<Attachment>, StoreError> {
        let conn = self.conn.borrow();
        let mut statement = conn.prepare(
            "SELECT a.hash, a.name, a.mime, a.size, a.data IS NOT NULL,
                    a.uploaded_at IS NOT NULL, a.created_at
             FROM attachment a
             JOIN note_attachment na ON na.hash = a.hash
             WHERE na.note_id = ?1
             ORDER BY na.added_at, a.hash",
        )?;
        let rows = statement.query_map(params![note.to_string()], attachment_row)?;
        let mut attachments = Vec::new();
        for row in rows {
            attachments.push(row?);
        }
        Ok(attachments)
    }

    fn unsynced_attachments(&self, limit: usize) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.borrow();
        let mut statement = conn.prepare(
            "SELECT hash FROM attachment
             WHERE uploaded_at IS NULL AND data IS NOT NULL
             ORDER BY created_at, hash
             LIMIT ?1",
        )?;
        let limit = if limit == 0 { -1 } else { limit as i64 };
        let rows = statement.query_map(params![limit], |row| row.get::<_, String>(0))?;
        let mut hashes = Vec::new();
        for row in rows {
            hashes.push(row?);
        }
        Ok(hashes)
    }

    fn notes_with_attachment_work(&self, limit: usize) -> Result<Vec<NoteId>, StoreError> {
        let conn = self.conn.borrow();
        let mut statement = conn.prepare(
            "SELECT na.note_id
             FROM note_attachment na
             JOIN attachment a ON a.hash = na.hash
             WHERE a.data IS NULL OR a.uploaded_at IS NULL
             GROUP BY na.note_id
             ORDER BY MIN(na.added_at), na.note_id
             LIMIT ?1",
        )?;
        let limit = if limit == 0 { -1 } else { limit as i64 };
        let rows = statement.query_map(params![limit], |row| row.get::<_, String>(0))?;
        let mut notes = Vec::new();
        for row in rows {
            let value = row?;
            notes.push(
                NoteId::parse(&value)
                    .map_err(|_| StoreError::Corrupt(format!("attachment note id {value}")))?,
            );
        }
        Ok(notes)
    }

    fn mark_attachment_uploaded(&self, hash: &str, at: i64) -> Result<(), StoreError> {
        let conn = self.conn.borrow();
        conn.execute(
            "UPDATE attachment SET uploaded_at = ?2 WHERE hash = ?1",
            params![hash.to_ascii_lowercase(), at],
        )?;
        Ok(())
    }

    fn attachment_usage(&self) -> Result<AttachmentUsage, StoreError> {
        let conn = self.conn.borrow();
        let (count, bytes): (i64, i64) = conn.query_row(
            "SELECT count(*), coalesce(sum(size), 0) FROM attachment WHERE data IS NOT NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(AttachmentUsage {
            count: count.max(0) as u64,
            bytes: bytes.max(0) as u64,
        })
    }

    fn apply_update(&self, id: &NoteId, update: &[u8], at: i64) -> Result<Note, StoreError> {
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        // Whether the server already had everything this note had. If it did,
        // the imported update comes back from the server and must not be
        // queued for a push of its own.
        let was_clean = note_was_clean(&tx, id)?;
        let note = match load_note(&tx, id, self.peer_id)? {
            Some(note) => note,
            None => {
                let note = Note::empty(*id);
                note.set_peer_id(self.peer_id)?;
                note
            }
        };
        let status = note.document().import(update)?;
        if status.pending.is_some() {
            // The server sends a note's updates in log order, so a missing
            // dependency means the log is not what this build expects.
            // Accepting it would advance the cursor past an edit that never
            // applied.
            return Err(StoreError::Corrupt(format!(
                "note {id} arrived without the history it builds on"
            )));
        }
        note.commit_at(at);
        save_note_with(&tx, &note, at)?;
        if was_clean {
            // The note now holds exactly what the server confirmed plus what
            // the server just sent, so no push is owed.
            let version = encode_version_vector(&note.document().version());
            tx.execute(
                "UPDATE note SET last_pushed = ?2 WHERE id = ?1",
                params![id.to_string(), version],
            )?;
        }
        tx.commit()?;
        Ok(note)
    }

    fn unsynced_notes(&self, limit: usize) -> Result<Vec<NoteId>, StoreError> {
        let conn = self.conn.borrow();
        let mut statement = conn.prepare(
            "SELECT id FROM note
             WHERE local_vv IS NULL OR local_vv IS NOT last_pushed
             ORDER BY updated_at DESC, id
             LIMIT ?1",
        )?;
        let limit = if limit == 0 { -1 } else { limit as i64 };
        let rows = statement.query_map(params![limit], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            let text = row?;
            ids.push(
                NoteId::parse(&text).map_err(|_| StoreError::Corrupt(format!("note id {text}")))?,
            );
        }
        Ok(ids)
    }

    fn last_pushed(&self, id: &NoteId) -> Result<Option<Vec<u8>>, StoreError> {
        let conn = self.conn.borrow();
        let value: Option<Option<Vec<u8>>> = conn
            .query_row(
                "SELECT last_pushed FROM note WHERE id = ?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        value.ok_or(StoreError::NoteNotFound(*id))
    }

    fn set_last_pushed(&self, id: &NoteId, version: &[u8]) -> Result<(), StoreError> {
        let conn = self.conn.borrow();
        let changed = conn.execute(
            "UPDATE note SET last_pushed = ?2 WHERE id = ?1",
            params![id.to_string(), version],
        )?;
        if changed == 0 {
            return Err(StoreError::NoteNotFound(*id));
        }
        Ok(())
    }

    fn list_notebooks(&self) -> Result<Vec<Notebook>, StoreError> {
        let conn = self.conn.borrow();
        notebook_rows(&conn)
    }

    fn create_notebook_with_id(
        &self,
        id: NotebookId,
        name: &str,
        at: i64,
    ) -> Result<Notebook, StoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Invalid("notebook name is empty".to_string()));
        }
        let notebook = Notebook {
            id,
            name: name.to_string(),
            created_at: at,
        };
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        let workspace = load_workspace(&tx, self.peer_id)?;
        workspace.upsert_notebook(&notebook)?;
        save_workspace(&tx, &workspace)?;
        tx.execute(
            "INSERT INTO notebook (id, name, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            params![notebook.id.to_string(), notebook.name, notebook.created_at],
        )?;
        tx.commit()?;
        Ok(notebook)
    }

    fn rename_notebook(&self, id: &NotebookId, name: &str) -> Result<bool, StoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::Invalid("notebook name is empty".to_string()));
        }
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        let workspace = load_workspace(&tx, self.peer_id)?;
        if !workspace.rename_notebook(id, name)? {
            return Ok(false);
        }
        save_workspace(&tx, &workspace)?;
        let changed = tx.execute(
            "UPDATE notebook SET name = ?2 WHERE id = ?1",
            params![id.to_string(), name],
        )?;
        tx.commit()?;
        Ok(changed > 0)
    }

    fn delete_notebook(&self, id: &NotebookId) -> Result<bool, StoreError> {
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        let workspace = load_workspace(&tx, self.peer_id)?;
        if !workspace.delete_notebook(id)? {
            return Ok(false);
        }
        save_workspace(&tx, &workspace)?;
        tx.execute(
            "DELETE FROM notebook WHERE id = ?1",
            params![id.to_string()],
        )?;
        tx.commit()?;
        Ok(true)
    }

    fn workspace_snapshot(&self) -> Result<Vec<u8>, StoreError> {
        let conn = self.conn.borrow();
        Ok(load_workspace(&conn, self.peer_id)?.snapshot()?)
    }

    fn import_workspace(&self, bytes: &[u8]) -> Result<Vec<Notebook>, StoreError> {
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        let workspace = load_workspace(&tx, self.peer_id)?;
        let was_clean = workspace_was_clean(&tx)?;
        let status = workspace.import(bytes)?;
        if status.pending.is_some() {
            return Err(StoreError::Corrupt(
                "the notebook list arrived without the history it builds on".to_string(),
            ));
        }
        save_workspace(&tx, &workspace)?;
        if was_clean {
            let version = encode_version_vector(&workspace.version());
            tx.execute(
                "UPDATE workspace SET last_pushed = ?1 WHERE id = 1",
                params![version],
            )?;
        }
        tx.execute("DELETE FROM notebook", [])?;
        let notebooks = workspace.notebooks();
        {
            let mut insert =
                tx.prepare("INSERT INTO notebook (id, name, created_at) VALUES (?1, ?2, ?3)")?;
            for notebook in &notebooks {
                insert.execute(params![
                    notebook.id.to_string(),
                    notebook.name,
                    notebook.created_at
                ])?;
            }
        }
        tx.commit()?;
        Ok(notebooks)
    }

    fn workspace_pending(&self) -> Result<Option<WorkspacePush>, StoreError> {
        let conn = self.conn.borrow();
        let row: Option<WorkspaceRow> = conn
            .query_row(
                "SELECT snapshot, local_vv, last_pushed FROM workspace WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((snapshot, local_vv, last_pushed)) = row else {
            return Ok(None);
        };
        let synced = matches!(&local_vv, Some(version) if last_pushed.as_ref() == Some(version));
        if synced {
            return Ok(None);
        }
        let workspace = WorkspaceDocument::from_snapshot(&snapshot)?;
        let since = decode_version(last_pushed.as_deref())?;
        Ok(Some(WorkspacePush {
            updates: workspace.updates_since(&since)?,
            version: encode_version_vector(&workspace.version()),
        }))
    }

    fn mark_workspace_pushed(&self, version: &[u8]) -> Result<(), StoreError> {
        let conn = self.conn.borrow();
        conn.execute(
            "UPDATE workspace SET last_pushed = ?1 WHERE id = 1",
            params![version],
        )?;
        Ok(())
    }

    fn list_tags(&self) -> Result<Vec<TagCount>, StoreError> {
        let conn = self.conn.borrow();
        let mut statement = conn.prepare(
            "SELECT t.tag, count(*) FROM note_tag t
             JOIN note n ON n.id = t.note_id
             WHERE n.trashed_at = 0
             GROUP BY t.tag
             ORDER BY count(*) DESC, t.tag",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(TagCount {
                tag: row.get(0)?,
                count: row.get::<_, i64>(1)?.max(0) as u64,
            })
        })?;
        let mut tags = Vec::new();
        for row in rows {
            tags.push(row?);
        }
        Ok(tags)
    }

    fn sync_state(&self) -> Result<SyncState, StoreError> {
        let conn = self.conn.borrow();
        let state: Option<SyncState> = conn
            .query_row(
                "SELECT cursor, updated_at FROM sync_state WHERE id = 1",
                [],
                |row| {
                    Ok(SyncState {
                        cursor: row.get(0)?,
                        updated_at: row.get(1)?,
                    })
                },
            )
            .optional()?;
        Ok(state.unwrap_or_default())
    }

    fn set_sync_state(&self, state: &SyncState) -> Result<(), StoreError> {
        let conn = self.conn.borrow();
        conn.execute(
            "INSERT INTO sync_state (id, cursor, updated_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET cursor = excluded.cursor, updated_at = excluded.updated_at",
            params![state.cursor, state.updated_at],
        )?;
        Ok(())
    }

    fn export_zip(&self, exported_at: i64) -> Result<Vec<u8>, StoreError> {
        let bundle = {
            let conn = self.conn.borrow();
            export_bundle_with(&conn, exported_at, self.peer_id)?
        };
        let mut bytes = Vec::new();
        write_export_zip(&bundle, Cursor::new(&mut bytes))?;
        Ok(bytes)
    }

    fn import_zip(&self, bytes: &[u8]) -> Result<ImportReport, StoreError> {
        let bundle = read_export_zip(Cursor::new(bytes))?;
        let mut conn = self.connection();
        let tx = conn.transaction()?;
        let mut report = ImportReport::default();

        let workspace = load_workspace(&tx, self.peer_id)?;
        for notebook in &bundle.notebooks {
            workspace.upsert_notebook(notebook)?;
            tx.execute(
                "INSERT INTO notebook (id, name, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name",
                params![notebook.id.to_string(), notebook.name, notebook.created_at],
            )?;
            report.notebooks += 1;
        }
        save_workspace(&tx, &workspace)?;

        let stamp = if bundle.exported_at > 0 {
            bundle.exported_at
        } else {
            1
        };
        for import in &bundle.notes {
            match load_note(&tx, &import.id, self.peer_id)? {
                Some(existing) => {
                    if let Some(snapshot) = &import.snapshot {
                        let status = existing.document().import(snapshot)?;
                        if status.pending.is_some() {
                            return Err(StoreError::Corrupt(format!(
                                "note {} arrived with incomplete history",
                                import.id
                            )));
                        }
                    }
                    if existing.text() != import.text {
                        existing.replace_text(&import.text)?;
                    }
                    existing.set_meta(&import.meta)?;
                    existing.commit_at(stamp);
                    save_note_with(&tx, &existing, stamp)?;
                    report.notes_updated += 1;
                }
                None => {
                    let note = match &import.snapshot {
                        Some(snapshot) => Note::from_snapshot(import.id, snapshot)?,
                        None => Note::from_text(import.id, &import.text, &import.meta)?,
                    };
                    note.set_peer_id(self.peer_id)?;
                    if note.text() != import.text {
                        note.replace_text(&import.text)?;
                    }
                    note.set_meta(&import.meta)?;
                    note.commit_at(stamp);
                    save_note_with(&tx, &note, stamp)?;
                    report.notes_created += 1;
                }
            }
        }

        for attachment in &bundle.attachments {
            import_attachment(&tx, attachment, stamp)?;
            report.attachments += 1;
        }

        tx.commit()?;
        Ok(report)
    }
}

/// Run `PRAGMA user_version` migrations.
fn migrate(conn: &Connection) -> Result<(), StoreError> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    match version {
        0 => {
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            Ok(())
        }
        1 => {
            conn.execute_batch(MIGRATION_V2)?;
            conn.execute_batch(MIGRATION_V3)?;
            conn.execute_batch(MIGRATION_V4)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            Ok(())
        }
        2 => {
            conn.execute_batch(MIGRATION_V3)?;
            conn.execute_batch(MIGRATION_V4)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            Ok(())
        }
        3 => {
            conn.execute_batch(MIGRATION_V4)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            Ok(())
        }
        v if v == SCHEMA_VERSION => Ok(()),
        v if v > SCHEMA_VERSION => Err(StoreError::Invalid(format!(
            "database schema {v} is newer than this build's {SCHEMA_VERSION}"
        ))),
        v => Err(StoreError::Invalid(format!(
            "database schema {v} is older than this build's {SCHEMA_VERSION}"
        ))),
    }
}

/// Read this database's Loro peer id, or create one on first open.
///
/// The id is random per database and persisted, so it stays stable across
/// reloads but two devices do not share one. The low and high halves of a
/// UUID are folded together; zero and `u64::MAX` are reserved by Loro.
fn load_or_create_device_peer(conn: &Connection) -> Result<u64, StoreError> {
    let stored: Option<i64> = conn
        .query_row("SELECT peer_id FROM device WHERE id = 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(peer) = stored {
        return Ok(peer as u64);
    }

    let uuid = NoteId::new().as_uuid().as_u128();
    let mut peer = (uuid as u64) ^ ((uuid >> 64) as u64);
    if peer == 0 || peer == u64::MAX {
        peer = 1;
    }
    conn.execute(
        "INSERT INTO device (id, peer_id) VALUES (1, ?1)",
        params![peer as i64],
    )?;
    Ok(peer)
}

/// Load one note's snapshot, if the row exists.
fn load_note(conn: &Connection, id: &NoteId, peer: u64) -> Result<Option<Note>, StoreError> {
    let snapshot: Option<Vec<u8>> = conn
        .query_row(
            "SELECT snapshot FROM note WHERE id = ?1",
            params![id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    match snapshot {
        Some(bytes) => {
            let note = Note::from_snapshot(*id, &bytes)?;
            note.set_peer_id(peer)?;
            Ok(Some(note))
        }
        None => Ok(None),
    }
}

/// Read one attachment's metadata without loading its bytes.
fn read_attachment(conn: &Connection, hash: &str) -> Result<Option<Attachment>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT hash, name, mime, size, data IS NOT NULL,
                    uploaded_at IS NOT NULL, created_at
             FROM attachment WHERE hash = ?1",
            params![hash],
            attachment_row,
        )
        .optional()?)
}

fn attachment_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        hash: row.get(0)?,
        name: row.get(1)?,
        mime: row.get(2)?,
        size: row.get::<_, i64>(3)?.max(0) as u64,
        available: row.get(4)?,
        uploaded: row.get(5)?,
        created_at: row.get(6)?,
    })
}

/// Lowercase a MIME type and drop any parameters.
fn normalized_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or(mime)
        .trim()
        .to_ascii_lowercase()
}

/// Rebuild the note-to-attachment index from the note's Markdown.
///
/// A note may link an attachment whose bytes are not here yet, so a missing
/// hash becomes a placeholder row. Nothing here deletes attachment bytes: a
/// removed link only removes the note's use of it.
fn update_note_attachment_links(
    conn: &Connection,
    note_id: &str,
    text: &str,
    at: i64,
) -> Result<(), StoreError> {
    let wanted = references(text);
    let wanted_hashes: std::collections::HashSet<&str> = wanted
        .iter()
        .map(|reference| reference.hash.as_str())
        .collect();

    let existing: Vec<String> = {
        let mut statement = conn.prepare("SELECT hash FROM note_attachment WHERE note_id = ?1")?;
        let rows = statement.query_map(params![note_id], |row| row.get::<_, String>(0))?;
        let mut hashes = Vec::new();
        for row in rows {
            hashes.push(row?);
        }
        hashes
    };
    for hash in existing {
        if !wanted_hashes.contains(hash.as_str()) {
            conn.execute(
                "DELETE FROM note_attachment WHERE note_id = ?1 AND hash = ?2",
                params![note_id, hash],
            )?;
        }
    }

    for reference in wanted {
        let mime = reference.mime().unwrap_or("application/octet-stream");
        let name = format!("attachment.{}", reference.extension);
        conn.execute(
            "INSERT OR IGNORE INTO attachment
                 (hash, name, mime, size, data, created_at, uploaded_at)
             VALUES (?1, ?2, ?3, 0, NULL, ?4, NULL)",
            params![reference.hash, name, mime, at],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO note_attachment (note_id, hash, added_at)
             VALUES (?1, ?2, ?3)",
            params![note_id, reference.hash, at],
        )?;
    }
    Ok(())
}

/// Write one attachment from an export archive. The caller owns the
/// transaction; the archive has already validated entry paths and sizes.
fn import_attachment(
    conn: &Connection,
    attachment: &ExportAttachment,
    at: i64,
) -> Result<(), StoreError> {
    let hash = content_hash(&attachment.data);
    let name = normalize_file_name(&attachment.name, &attachment.mime);
    let mime = normalized_mime(&attachment.mime);
    conn.execute(
        "INSERT INTO attachment (hash, name, mime, size, data, created_at, uploaded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
         ON CONFLICT(hash) DO UPDATE SET
             data = excluded.data,
             size = excluded.size,
             name = CASE WHEN attachment.data IS NULL THEN excluded.name ELSE attachment.name END,
             mime = CASE WHEN attachment.data IS NULL THEN excluded.mime ELSE attachment.mime END",
        params![
            hash,
            name,
            mime,
            attachment.data.len() as i64,
            attachment.data,
            at
        ],
    )?;
    if let Some(note_id) = attachment.note_id {
        conn.execute(
            "INSERT OR IGNORE INTO note_attachment (note_id, hash, added_at)
             VALUES (?1, ?2, ?3)",
            params![note_id.to_string(), hash, at],
        )?;
    }
    Ok(())
}

/// Write one note's row, tags and search index. The caller owns the transaction.
fn save_note_with(
    conn: &Connection,
    note: &Note,
    updated_at: i64,
) -> Result<NoteSummary, StoreError> {
    let id = note.id();
    let id_text = id.to_string();
    let meta = note.meta();
    let title = note.title();
    let preview = note.preview();
    let text = note.text();
    let snapshot = note.document().snapshot()?;
    let created_at = if meta.created_at != 0 {
        meta.created_at
    } else {
        updated_at
    };
    let trashed_at = meta.trashed_at.unwrap_or(0);
    let notebook_id = meta.notebook_id.map(|id| id.to_string());
    let local_vv = encode_version_vector(&note.document().version());

    conn.execute(
        UPSERT_NOTE,
        params![
            id_text,
            snapshot,
            title,
            preview,
            notebook_id,
            meta.pinned,
            meta.archived,
            meta.color,
            created_at,
            updated_at,
            trashed_at,
            local_vv
        ],
    )?;

    let tags = normalize_tags(meta.tags);
    conn.execute("DELETE FROM note_tag WHERE note_id = ?1", params![id_text])?;
    {
        let mut insert = conn.prepare(
            "INSERT OR REPLACE INTO note_tag (note_id, tag, position) VALUES (?1, ?2, ?3)",
        )?;
        for (position, tag) in tags.iter().enumerate() {
            insert.execute(params![id_text, tag, position as i64])?;
        }
    }

    // The FTS row shares the note's integer rowid, so this touches one row.
    conn.execute(
        "DELETE FROM note_fts WHERE rowid = (SELECT seq FROM note WHERE id = ?1)",
        params![id_text],
    )?;
    conn.execute(
        "INSERT INTO note_fts (rowid, note_id, title, body)
         VALUES ((SELECT seq FROM note WHERE id = ?1), ?1, ?2, ?3)",
        params![id_text, title, text],
    )?;

    update_note_attachment_links(conn, &id_text, &text, updated_at)?;

    Ok(NoteSummary {
        id,
        title,
        preview,
        notebook_id: meta.notebook_id,
        tags,
        pinned: meta.pinned,
        archived: meta.archived,
        color: meta.color,
        created_at,
        updated_at,
        trashed_at: meta.trashed_at,
    })
}

/// A database row before ids are parsed.
struct RawSummary {
    id: String,
    title: String,
    preview: String,
    notebook_id: Option<String>,
    pinned: bool,
    archived: bool,
    color: Option<String>,
    created_at: i64,
    updated_at: i64,
    trashed_at: i64,
}

fn raw_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawSummary> {
    Ok(RawSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        preview: row.get(2)?,
        notebook_id: row.get(3)?,
        pinned: row.get::<_, i64>(4)? != 0,
        archived: row.get::<_, i64>(5)? != 0,
        color: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        trashed_at: row.get(9)?,
    })
}

fn finish_summary(raw: RawSummary) -> Result<NoteSummary, StoreError> {
    let id = NoteId::parse(&raw.id).map_err(|_| StoreError::Corrupt(format!("id {}", raw.id)))?;
    let notebook_id = match raw.notebook_id {
        Some(value) if !value.is_empty() => Some(
            NotebookId::parse(&value)
                .map_err(|_| StoreError::Corrupt(format!("notebook {value}")))?,
        ),
        _ => None,
    };
    Ok(NoteSummary {
        id,
        title: raw.title,
        preview: raw.preview,
        notebook_id,
        tags: Vec::new(),
        pinned: raw.pinned,
        archived: raw.archived,
        color: raw.color.filter(|value| !value.is_empty()),
        created_at: raw.created_at,
        updated_at: raw.updated_at,
        trashed_at: if raw.trashed_at == 0 {
            None
        } else {
            Some(raw.trashed_at)
        },
    })
}

/// List notes, optionally for a single id.
fn list_with(
    conn: &Connection,
    query: &NoteQuery,
    only: Option<NoteId>,
) -> Result<Vec<NoteSummary>, StoreError> {
    let mut sql = format!("SELECT {SUMMARY_COLUMNS} FROM note n WHERE ");
    let mut params: Vec<Value> = Vec::new();

    let mut clauses = vec![view_clause(query.view).to_string()];
    if let Some(id) = only {
        clauses.push("n.id = ?".to_string());
        params.push(Value::Text(id.to_string()));
    }
    if let Some(notebook_id) = query.notebook_id {
        clauses.push("n.notebook_id = ?".to_string());
        params.push(Value::Text(notebook_id.to_string()));
    }
    if let Some(tag) = query
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        clauses.push(
            "EXISTS (SELECT 1 FROM note_tag t WHERE t.note_id = n.id AND t.tag = ?)".to_string(),
        );
        params.push(Value::Text(tag.to_string()));
    }
    sql.push_str(&clauses.join(" AND "));
    sql.push_str(match query.view {
        NoteView::Trash => " ORDER BY n.trashed_at DESC, n.updated_at DESC, n.id",
        _ => " ORDER BY n.pinned DESC, n.updated_at DESC, n.id",
    });
    if query.limit > 0 {
        sql.push_str(" LIMIT ? OFFSET ?");
        params.push(Value::Integer(query.limit as i64));
        params.push(Value::Integer(query.offset as i64));
    } else if query.offset > 0 {
        sql.push_str(" LIMIT -1 OFFSET ?");
        params.push(Value::Integer(query.offset as i64));
    }

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params.iter()), raw_summary)?;
    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(finish_summary(row?)?);
    }
    drop(statement);
    attach_tags(conn, &mut summaries)?;
    Ok(summaries)
}

/// Full-text search over title and body.
fn search_with(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>, StoreError> {
    let Some(expression) = fts_expression(&query.text) else {
        return Ok(Vec::new());
    };
    let limit = if query.limit == 0 { 50 } else { query.limit };

    let mut sql = format!(
        "SELECT {SUMMARY_COLUMNS}, bm25(note_fts) AS rank, \
         snippet(note_fts, -1, '[', ']', '…', 12) \
         FROM note_fts JOIN note n ON n.id = note_fts.note_id WHERE note_fts MATCH ?"
    );
    let mut params: Vec<Value> = vec![Value::Text(expression)];
    let mut clauses = vec![view_clause(query.view).to_string()];
    if let Some(notebook_id) = query.notebook_id {
        clauses.push("n.notebook_id = ?".to_string());
        params.push(Value::Text(notebook_id.to_string()));
    }
    if let Some(tag) = query
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        clauses.push(
            "EXISTS (SELECT 1 FROM note_tag t WHERE t.note_id = n.id AND t.tag = ?)".to_string(),
        );
        params.push(Value::Text(tag.to_string()));
    }
    sql.push_str(" AND ");
    sql.push_str(&clauses.join(" AND "));
    sql.push_str(" ORDER BY rank LIMIT ?");
    params.push(Value::Integer(limit as i64));

    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params.iter()), |row| {
        Ok((
            raw_summary(row)?,
            row.get::<_, f64>(10)?,
            row.get::<_, String>(11)?,
        ))
    })?;
    let mut hits = Vec::new();
    for row in rows {
        let (raw, rank, snippet) = row?;
        hits.push(SearchHit {
            note: finish_summary(raw)?,
            snippet,
            rank,
        });
    }
    drop(statement);

    let mut summaries: Vec<NoteSummary> = hits.iter().map(|hit| hit.note.clone()).collect();
    attach_tags(conn, &mut summaries)?;
    for (hit, summary) in hits.iter_mut().zip(summaries) {
        hit.note = summary;
    }
    Ok(hits)
}

/// Turn user text into an FTS5 expression: every word, matched by prefix.
///
/// The whole query is quoted, so `AND`, `OR` and `*` typed by the user cannot
/// be read as FTS5 syntax.
fn fts_expression(text: &str) -> Option<String> {
    let mut terms = Vec::new();
    for word in text.split(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '\'')) {
        let word = word.trim_matches('\'');
        if word.is_empty() {
            continue;
        }
        terms.push(format!("\"{word}\"*"));
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

fn view_clause(view: NoteView) -> &'static str {
    match view {
        NoteView::Active => "n.trashed_at = 0 AND n.archived = 0",
        NoteView::Archived => "n.trashed_at = 0 AND n.archived = 1",
        NoteView::Trash => "n.trashed_at <> 0",
        NoteView::All => "1 = 1",
    }
}

/// Read the tags of the listed notes in one query, keeping their order.
fn attach_tags(conn: &Connection, summaries: &mut [NoteSummary]) -> Result<(), StoreError> {
    if summaries.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = summaries.iter().map(|note| note.id.to_string()).collect();
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT note_id, tag FROM note_tag WHERE note_id IN ({placeholders}) \
         ORDER BY note_id, position"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(ids.iter().map(String::as_str)), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut by_note: HashMap<String, Vec<String>> = HashMap::new();
    for row in rows {
        let (id, tag) = row?;
        by_note.entry(id).or_default().push(tag);
    }
    for summary in summaries.iter_mut() {
        if let Some(tags) = by_note.remove(&summary.id.to_string()) {
            summary.tags = tags;
        }
    }
    Ok(())
}

fn notebook_rows(conn: &Connection) -> Result<Vec<Notebook>, StoreError> {
    let mut statement =
        conn.prepare("SELECT id, name, created_at FROM notebook ORDER BY created_at, name, id")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut notebooks = Vec::new();
    for row in rows {
        let (id, name, created_at) = row?;
        let id =
            NotebookId::parse(&id).map_err(|_| StoreError::Corrupt(format!("notebook id {id}")))?;
        notebooks.push(Notebook {
            id,
            name,
            created_at,
        });
    }
    Ok(notebooks)
}

fn load_workspace(conn: &Connection, peer: u64) -> Result<WorkspaceDocument, StoreError> {
    let snapshot: Option<Vec<u8>> = conn
        .query_row("SELECT snapshot FROM workspace WHERE id = 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    let workspace = match snapshot {
        Some(bytes) => WorkspaceDocument::from_snapshot(&bytes)?,
        None => WorkspaceDocument::new(),
    };
    workspace.set_peer_id(peer)?;
    Ok(workspace)
}

fn save_workspace(conn: &Connection, workspace: &WorkspaceDocument) -> Result<(), StoreError> {
    let snapshot = workspace.snapshot()?;
    let local_vv = encode_version_vector(&workspace.version());
    conn.execute(
        "INSERT INTO workspace (id, snapshot, local_vv) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET snapshot = excluded.snapshot, local_vv = excluded.local_vv",
        params![snapshot, local_vv],
    )?;
    Ok(())
}

/// Whether the server already had everything this note had.
///
/// A note written by schema v1 has no `local_vv`; that counts as not clean,
/// so the first sync after an upgrade pushes it once and fills the column.
fn note_was_clean(conn: &Connection, id: &NoteId) -> Result<bool, StoreError> {
    let clean: Option<bool> = conn
        .query_row(
            "SELECT (local_vv IS NOT NULL AND local_vv IS last_pushed)
             FROM note WHERE id = ?1",
            params![id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    // No row yet: the note is new, so nothing local is owed to the server.
    Ok(clean.unwrap_or(true))
}

/// Whether the server already had everything the workspace document had.
fn workspace_was_clean(conn: &Connection) -> Result<bool, StoreError> {
    let clean: Option<bool> = conn
        .query_row(
            "SELECT (local_vv IS NOT NULL AND local_vv IS last_pushed)
             FROM workspace WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(clean.unwrap_or(true))
}

/// Decode an encoded `VersionVector`, treating a missing one as "nothing yet".
fn decode_version(bytes: Option<&[u8]>) -> Result<VersionVector, StoreError> {
    match bytes {
        Some(bytes) => decode_version_vector(bytes).map_err(StoreError::Note),
        None => Ok(VersionVector::default()),
    }
}

/// Gather every note, notebook and attachment for an export.
fn export_bundle_with(
    conn: &Connection,
    exported_at: i64,
    peer: u64,
) -> Result<ExportBundle, StoreError> {
    let mut statement = conn.prepare("SELECT id FROM note ORDER BY created_at, id")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<String>, _>>()?;
    drop(statement);

    let mut notes = Vec::with_capacity(ids.len());
    for id in ids {
        let id = NoteId::parse(&id).map_err(|_| StoreError::Corrupt(format!("note id {id}")))?;
        let note = load_note(conn, &id, peer)?.ok_or(StoreError::NoteNotFound(id))?;
        notes.push(ExportNote {
            id,
            meta: note.meta(),
            text: note.text(),
            snapshot: Some(note.document().snapshot()?),
        });
    }

    let mut statement = conn.prepare(
        "SELECT a.name, a.mime, a.data,
                (SELECT na.note_id FROM note_attachment na
                 WHERE na.hash = a.hash ORDER BY na.added_at LIMIT 1)
         FROM attachment a
         WHERE a.data IS NOT NULL
         ORDER BY a.created_at, a.hash",
    )?;
    let attachments = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(name, mime, data, note_id)| ExportAttachment {
            note_id: note_id.and_then(|value| NoteId::parse(&value).ok()),
            name,
            mime,
            data,
        })
        .collect();

    Ok(ExportBundle {
        exported_at,
        notebooks: notebook_rows(conn)?,
        notes,
        attachments,
    })
}

#[cfg(test)]
mod tests {
    use super::fts_expression;

    #[test]
    fn search_words_become_quoted_prefix_terms() {
        assert_eq!(
            fts_expression("quick fox"),
            Some("\"quick\"* AND \"fox\"*".to_string())
        );
        assert_eq!(
            fts_expression("don't stop"),
            Some("\"don't\"* AND \"stop\"*".to_string())
        );
        assert_eq!(fts_expression("  ...  "), None);
    }

    #[test]
    fn fts_syntax_characters_are_not_operators() {
        let expression = fts_expression("rock OR roll*").expect("expression");
        assert!(!expression.contains(" OR "));
        assert_eq!(
            expression,
            "\"rock\"* AND \"OR\"* AND \"roll\"*".to_string()
        );
    }
}
