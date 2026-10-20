//! The workspace document: the notebook list, as one small Loro document.
//!
//! Notebooks live in a CRDT for the same reason notes do: creating a notebook
//! on the phone while renaming one on the desktop must merge without a
//! conflict dialog. The SQLite table is a queryable mirror, not the source of
//! truth.

use loro::LoroDoc;
use notes_proto::{Notebook, NotebookId};
use serde::{Deserialize, Serialize};

use crate::{NoteDocument, NoteError};

/// Root container that holds notebooks, keyed by notebook id.
pub const NOTEBOOKS_CONTAINER: &str = "notebooks";

/// One notebook's value inside the workspace document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NotebookValue {
    name: String,
    created_at: i64,
}

/// The workspace document.
#[derive(Debug)]
pub struct WorkspaceDocument {
    doc: NoteDocument,
}

impl Default for WorkspaceDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceDocument {
    /// An empty workspace.
    pub fn new() -> Self {
        Self {
            doc: NoteDocument::new(),
        }
    }

    /// Restore from a snapshot.
    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, NoteError> {
        Ok(Self {
            doc: NoteDocument::from_snapshot(bytes)?,
        })
    }

    /// The underlying Loro document.
    pub fn as_loro(&self) -> &LoroDoc {
        self.doc.as_loro()
    }

    /// Full snapshot, for storage and sync.
    pub fn snapshot(&self) -> Result<Vec<u8>, NoteError> {
        self.doc.snapshot()
    }

    /// Updates this document has that the peer at `since` does not.
    pub fn updates_since(&self, since: &crate::VersionVector) -> Result<Vec<u8>, NoteError> {
        self.doc.updates_since(since)
    }

    /// Import a snapshot or updates; importing twice is harmless.
    pub fn import(&self, bytes: &[u8]) -> Result<crate::ImportStatus, NoteError> {
        self.doc.import(bytes)
    }

    /// Current oplog version, used as the sync cursor.
    pub fn version(&self) -> crate::VersionVector {
        self.doc.version()
    }

    /// Commit pending operations as one change.
    pub fn commit(&self) {
        self.doc.commit();
    }

    /// Create a notebook, or update its name when the id already exists.
    pub fn upsert_notebook(&self, notebook: &Notebook) -> Result<(), NoteError> {
        let value = NotebookValue {
            name: notebook.name.clone(),
            created_at: notebook.created_at,
        };
        let json = serde_json::to_string(&value)?;
        self.notebooks_map()
            .insert(&notebook.id.to_string(), json)?;
        self.doc.commit();
        Ok(())
    }

    /// Rename a notebook. Returns whether the notebook exists.
    pub fn rename_notebook(&self, id: &NotebookId, name: &str) -> Result<bool, NoteError> {
        let Some(mut value) = self.notebook_value(id) else {
            return Ok(false);
        };
        value.name = name.to_string();
        let json = serde_json::to_string(&value)?;
        self.notebooks_map().insert(&id.to_string(), json)?;
        self.doc.commit();
        Ok(true)
    }

    /// Remove a notebook from the workspace. Returns whether it existed.
    ///
    /// Notes that pointed at it keep their `notebook_id`; moving them is a
    /// deliberate UI action, not a side effect of deleting a notebook.
    pub fn delete_notebook(&self, id: &NotebookId) -> Result<bool, NoteError> {
        if self.notebook_value(id).is_none() {
            return Ok(false);
        }
        self.notebooks_map().delete(&id.to_string())?;
        self.doc.commit();
        Ok(true)
    }

    /// Every notebook, sorted by creation time then name.
    pub fn notebooks(&self) -> Vec<Notebook> {
        let mut out = Vec::new();
        self.notebooks_map().for_each(|key, value| {
            let Ok(id) = NotebookId::parse(key) else {
                return;
            };
            let Some(value) = value.get_deep_value().into_string().ok() else {
                return;
            };
            let Ok(value) = serde_json::from_str::<NotebookValue>(value.as_ref()) else {
                return;
            };
            out.push(Notebook {
                id,
                name: value.name,
                created_at: value.created_at,
            });
        });
        out.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        out
    }

    fn notebooks_map(&self) -> loro::LoroMap {
        self.as_loro().get_map(NOTEBOOKS_CONTAINER)
    }

    fn notebook_value(&self, id: &NotebookId) -> Option<NotebookValue> {
        let value = self.notebooks_map().get(&id.to_string())?;
        let value = value.get_deep_value().into_string().ok()?;
        serde_json::from_str(value.as_ref()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceDocument;
    use notes_proto::{Notebook, NotebookId};

    #[test]
    fn notebooks_round_trip_through_a_snapshot() {
        let workspace = WorkspaceDocument::new();
        let first = Notebook {
            id: NotebookId::new(),
            name: "Inbox".to_string(),
            created_at: 100,
        };
        let second = Notebook {
            id: NotebookId::new(),
            name: "Work".to_string(),
            created_at: 200,
        };
        workspace.upsert_notebook(&first).expect("first");
        workspace.upsert_notebook(&second).expect("second");

        let restored = WorkspaceDocument::from_snapshot(&workspace.snapshot().expect("snapshot"))
            .expect("restore");
        assert_eq!(restored.notebooks(), vec![first, second]);
    }

    #[test]
    fn rename_and_delete_change_only_that_notebook() {
        let workspace = WorkspaceDocument::new();
        let notebook = Notebook {
            id: NotebookId::new(),
            name: "Old".to_string(),
            created_at: 100,
        };
        workspace
            .upsert_notebook(&notebook)
            .expect("upsert notebook");
        assert!(workspace
            .rename_notebook(&notebook.id, "New")
            .expect("rename"));
        assert_eq!(workspace.notebooks()[0].name, "New");
        assert!(workspace.delete_notebook(&notebook.id).expect("delete"));
        assert!(!workspace.delete_notebook(&notebook.id).expect("delete"));
        assert!(workspace.notebooks().is_empty());
    }

    #[test]
    fn concurrent_notebooks_from_two_replicas_both_survive() {
        let base = WorkspaceDocument::new();
        let left =
            WorkspaceDocument::from_snapshot(&base.snapshot().expect("snapshot")).expect("left");
        let right =
            WorkspaceDocument::from_snapshot(&base.snapshot().expect("snapshot")).expect("right");

        let a = Notebook {
            id: NotebookId::new(),
            name: "A".to_string(),
            created_at: 1,
        };
        let b = Notebook {
            id: NotebookId::new(),
            name: "B".to_string(),
            created_at: 2,
        };
        left.upsert_notebook(&a).expect("a");
        right.upsert_notebook(&b).expect("b");

        let from_left = left.updates_since(&right.version()).expect("updates");
        right.import(&from_left).expect("import");
        let from_right = right.updates_since(&left.version()).expect("updates");
        left.import(&from_right).expect("import");

        assert_eq!(left.notebooks(), right.notebooks());
        assert_eq!(left.notebooks().len(), 2);
    }
}
