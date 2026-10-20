//! History: version grouping, preview, diff and restore.

use notes_core::{Note, NoteMeta, HISTORY_MERGE_INTERVAL_SECS};
use notes_proto::NoteId;

/// Build a note whose changes carry explicit timestamps.
///
/// Loro's own change-merging is switched off so this test checks the
/// five-minute version grouping in `notes-core`, not Loro's compaction.
fn note_with_edits() -> Note {
    let note = Note::new(NoteId::new(), 1_000).expect("note");
    note.document().as_loro().set_change_merge_interval(0);
    note.insert(0, "hello").expect("insert");
    note.commit_at(1_001);
    note.insert(5, " world").expect("insert");
    note.commit_at(1_100);
    note.insert(11, "!").expect("insert");
    note.commit_at(1_400);
    note.insert(12, " later").expect("insert");
    note.commit_at(2_000);
    note
}

#[test]
fn changes_within_five_minutes_are_one_version() {
    let note = note_with_edits();
    let history = note.history().expect("history");
    assert_eq!(history.len(), 2, "history: {history:?}");

    let first = &history[0];
    assert_eq!(first.started_at, 1_000);
    assert_eq!(first.ended_at, 1_400);
    assert_eq!(first.change_count, 4);

    let second = &history[1];
    assert_eq!(second.started_at, 2_000);
    assert_eq!(second.ended_at, 2_000);
    assert_eq!(second.change_count, 1);
    assert_eq!(
        second.started_at - first.ended_at,
        HISTORY_MERGE_INTERVAL_SECS + 300
    );
}

#[test]
fn a_version_previews_the_note_as_it_was() {
    let note = note_with_edits();
    let history = note.history().expect("history");
    assert_eq!(note.text_at(&history[0]).expect("text at"), "hello world!");
    assert_eq!(
        note.text_at(&history[1]).expect("text at"),
        "hello world! later"
    );
    // A preview must not disturb the live document.
    assert_eq!(note.text(), "hello world! later");
    assert!(!note.document().is_detached());
}

#[test]
fn the_diff_lists_the_lines_a_version_lost() {
    let note = note_with_edits();
    let history = note.history().expect("history");
    let diff = note.diff_from_current(&history[0]).expect("diff");
    assert!(diff.contains("hello world!"), "diff: {diff}");
    assert!(diff.contains("+hello world! later"), "diff: {diff}");

    let current = note.history().expect("history");
    let newest = current.last().expect("newest");
    assert_eq!(
        note.diff_from_current(newest).expect("diff"),
        "",
        "the newest version is the current text"
    );
}

#[test]
fn restoring_a_version_writes_a_new_edit_and_keeps_history() {
    let note = note_with_edits();
    let history = note.history().expect("history");
    let before = note.document().snapshot().expect("snapshot");

    assert!(note.restore_version(&history[0]).expect("restore"));
    assert_eq!(note.text(), "hello world!");

    // Restoring again to the same state is a no-op.
    assert!(!note.restore_version(&history[0]).expect("restore again"));

    let after = note.history().expect("history");
    assert!(
        after.len() >= history.len(),
        "history only grows: {} -> {}",
        history.len(),
        after.len()
    );
    // The old version still previews its own text, even after the restore.
    assert_eq!(
        note.text_at(&history[1]).expect("text at"),
        "hello world! later"
    );
    // Nothing was erased from the oplog by the restore.
    let restored = Note::from_snapshot(note.id(), &before).expect("restore snapshot");
    assert_eq!(restored.text(), "hello world! later");
}

#[test]
fn concurrent_peers_produce_one_version_each() {
    let base = Note::new(NoteId::new(), 1_000).expect("note");
    base.insert(0, "shared").expect("insert");
    base.commit_at(1_000);

    let left = Note::from_snapshot(base.id(), &base.document().snapshot().expect("snapshot"))
        .expect("left");
    let right = Note::from_snapshot(base.id(), &base.document().snapshot().expect("snapshot"))
        .expect("right");
    left.document().as_loro().set_change_merge_interval(0);
    right.document().as_loro().set_change_merge_interval(0);

    left.insert(6, " left").expect("insert");
    left.commit_at(1_100);
    right.insert(6, " right").expect("insert");
    right.commit_at(1_150);

    // Left holds the base change plus its own edit, one version per device.
    let history = left.history().expect("history");
    assert_eq!(history.len(), 2, "history: {history:?}");
    assert_eq!(history[0].change_count, 1);
    assert_eq!(history[0].ended_at, 1_000);
    let left_peer = left.document().peer_id();
    assert_eq!(history[1].peer, left_peer);
    assert_eq!(history[1].change_count, 1);
    assert_eq!(history[1].ended_at, 1_100);
    assert_eq!(left.text_at(&history[1]).expect("text"), "shared left");

    let update = left
        .document()
        .updates_since(&right.document().version())
        .expect("updates");
    right.document().import(&update).expect("import");
    let update = right
        .document()
        .updates_since(&left.document().version())
        .expect("updates");
    left.document().import(&update).expect("import");

    assert_eq!(left.text(), right.text());
    let history = left.history().expect("history");
    let ids: Vec<String> = history.iter().map(|version| version.id.clone()).collect();
    let right_history = right.history().expect("history");
    let peer_ids: Vec<String> = right_history
        .iter()
        .map(|version| version.id.clone())
        .collect();
    assert_eq!(ids, peer_ids, "both peers group the same oplog identically");
}

#[test]
fn a_fresh_note_has_no_versions_beyond_its_creation() {
    let note = Note::new(NoteId::new(), 0).expect("note");
    let history = note.history().expect("history");
    // Creation writes the metadata map, which is one change.
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].change_count, 1);
    assert_eq!(note.text_at(&history[0]).expect("text at"), "");
    assert_eq!(note.meta(), NoteMeta::default());
}
