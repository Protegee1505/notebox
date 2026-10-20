//! The version timeline: grouping the oplog into the versions a person sees.
//!
//! Every committed change stays in the document forever. For the history view
//! those changes are grouped into versions: consecutive changes by one device
//! with no gap longer than [`HISTORY_MERGE_INTERVAL_SECS`] are one version, so
//! one sitting of typing is one entry instead of one per debounced save.
//!
//! A version keeps the frontiers that reproduce the note exactly as it was
//! after the version's last change. Previewing forks the document at those
//! frontiers; restoring writes the old text as a new edit, so history only
//! ever grows.

use std::collections::BTreeMap;
use std::ops::ControlFlow;

use loro::{ChangeMeta, Frontiers, LoroDoc, ID};
use similar::TextDiff;

use crate::{Note, NoteError, HISTORY_MERGE_INTERVAL_SECS};

/// One entry in the history view.
#[derive(Debug, Clone)]
pub struct NoteVersion {
    /// Stable identity: the peer and counter of the version's last change.
    pub id: String,
    /// Device (Loro peer) that produced this version.
    pub peer: u64,
    /// Unix seconds of the first change in the version.
    pub started_at: i64,
    /// Unix seconds of the last change in the version.
    pub ended_at: i64,
    /// How many Loro changes were grouped into this version.
    pub change_count: usize,
    /// Frontiers that reproduce the note after this version.
    pub frontiers: Frontiers,
}

impl NoteVersion {
    /// Whether two versions describe the same point in the oplog.
    pub fn same_as(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Note {
    /// All versions, oldest first.
    ///
    /// Changes that share a peer and are no more than five minutes apart are
    /// one version. Versions are sorted by the time the group started.
    pub fn history(&self) -> Result<Vec<NoteVersion>, NoteError> {
        let doc = self.document().as_loro();
        let frontiers = doc.oplog_frontiers();
        let mut changes = Vec::new();
        collect_changes(doc, &frontiers, &mut changes)?;
        if changes.is_empty() {
            return Ok(Vec::new());
        }

        let mut per_peer: BTreeMap<u64, Vec<ChangeMeta>> = BTreeMap::new();
        for change in changes {
            per_peer.entry(change.id.peer).or_default().push(change);
        }

        let mut versions = Vec::new();
        for (_peer, mut group) in per_peer {
            group.sort_by_key(|change| change.id.counter);
            let mut current: Vec<ChangeMeta> = Vec::new();
            for change in group {
                let starts_new_version = current.last().is_some_and(|previous| {
                    change.timestamp.saturating_sub(previous.timestamp)
                        > HISTORY_MERGE_INTERVAL_SECS
                });
                if starts_new_version {
                    versions.push(build_version(doc, &current));
                    current.clear();
                }
                current.push(change);
            }
            if !current.is_empty() {
                versions.push(build_version(doc, &current));
            }
        }

        versions.sort_by(|a, b| {
            a.started_at
                .cmp(&b.started_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(versions)
    }

    /// The body as it was after `version`.
    pub fn text_at(&self, version: &NoteVersion) -> Result<String, NoteError> {
        Ok(self.document().fork_at(&version.frontiers)?.text())
    }

    /// Unified diff from `version` to the current body.
    ///
    /// Empty when the note is unchanged since that version.
    pub fn diff_from_current(&self, version: &NoteVersion) -> Result<String, NoteError> {
        let old = self.text_at(version)?;
        let new = self.text();
        let diff = TextDiff::from_lines(old.as_str(), new.as_str());
        Ok(diff
            .unified_diff()
            .context_radius(3)
            .header(&format!("version {}", version.id), "current")
            .to_string())
    }

    /// Restore the body of `version` as a new edit.
    ///
    /// Returns whether anything changed. The old state remains in history:
    /// restore never rewrites the oplog.
    pub fn restore_version(&self, version: &NoteVersion) -> Result<bool, NoteError> {
        let old = self.text_at(version)?;
        if old == self.text() {
            return Ok(false);
        }
        self.replace_text(&old)?;
        Ok(true)
    }
}

/// Walk the whole oplog from `frontiers` and collect every change.
fn collect_changes(
    doc: &LoroDoc,
    frontiers: &Frontiers,
    out: &mut Vec<ChangeMeta>,
) -> Result<(), NoteError> {
    if frontiers.is_empty() {
        return Ok(());
    }
    let ids: Vec<ID> = frontiers.iter().collect();
    doc.travel_change_ancestors(&ids, &mut |change| {
        out.push(change);
        ControlFlow::Continue(())
    })
    .map_err(|error| NoteError::History(error.to_string()))
}

/// Turn a run of changes from one peer into a version.
fn build_version(doc: &LoroDoc, group: &[ChangeMeta]) -> NoteVersion {
    let first = &group[0];
    let last = &group[group.len() - 1];
    let mut frontiers = last.deps.clone();
    let len = i32::try_from(last.len).unwrap_or(i32::MAX);
    let end = ID {
        peer: last.id.peer,
        counter: last.id.counter.saturating_add(len).saturating_sub(1),
    };
    frontiers.push(end);
    let frontiers = doc.minimize_frontiers(&frontiers).unwrap_or(frontiers);

    NoteVersion {
        id: format!("{}:{}", last.id.peer, last.id.counter),
        peer: last.id.peer,
        started_at: first.timestamp,
        ended_at: last.timestamp,
        change_count: group.len(),
        frontiers,
    }
}
