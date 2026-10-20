//! P7 store tests: a serialized database image keeps everything the browser
//! needs across a reload.

use notes_core::{attachment_url, encode_version_vector, Note};
use notes_store::{
    NewNote, NoteQuery, NoteStore, NoteView, SearchQuery, SqliteStore, SyncState, IMAGE_MAGIC,
};

fn store() -> SqliteStore {
    SqliteStore::open_in_memory().expect("in-memory store")
}

fn create(store: &SqliteStore, text: &str, at: i64) -> Note {
    store.create_note(NewNote::new(text, at)).expect("create")
}

#[test]
fn an_image_round_trips_notes_history_search_attachments_and_sync_state() {
    let store = store();
    let notebook = store.create_notebook("Work", 1_000).expect("notebook");

    let note = create(&store, "# Trip\n\nfirst version", 1_000);
    note.set_next_commit_timestamp(2_000);
    note.replace_text("# Trip\n\nsecond version").expect("edit");
    note.add_tag("travel").expect("tag");
    note.set_pinned(true).expect("pin");
    store.save_note(&note, 2_000).expect("save");

    let photo = store
        .put_attachment("photo.png", "image/webp", b"webp-photo-bytes", 2_500)
        .expect("attachment");
    let body = format!(
        "# Trip\n\nsecond version\n\n![photo]({})",
        attachment_url(&photo.hash, &photo.mime)
    );
    let edited = store.open_note(&note.id()).expect("open");
    edited.set_next_commit_timestamp(3_000);
    edited.replace_text(&body).expect("edit");
    let mut meta = edited.meta();
    meta.notebook_id = Some(notebook.id);
    meta.tags = vec!["travel".to_string()];
    edited.set_meta(&meta).expect("meta");
    store.save_note(&edited, 3_000).expect("save");

    let trashed = create(&store, "old draft", 3_100);
    store.trash_note(&trashed.id(), 3_200).expect("trash");

    let current = store.open_note(&note.id()).expect("open");
    let versions: Vec<String> = current
        .history()
        .expect("history")
        .into_iter()
        .map(|version| version.id)
        .collect();
    assert_eq!(versions.len(), 3, "create plus two later edits");

    let pushed = encode_version_vector(&current.document().version());
    store
        .set_last_pushed(&current.id(), &pushed)
        .expect("last pushed");
    let sync = SyncState {
        cursor: Some(42),
        updated_at: 3_300,
    };
    store.set_sync_state(&sync).expect("sync state");

    let image = store.to_image().expect("image");
    assert!(image.starts_with(IMAGE_MAGIC));
    assert!(image.len() > 100, "image is more than the header");

    let restored = SqliteStore::from_image(&image).expect("restore image");
    assert_eq!(restored.device_peer_id(), store.device_peer_id());

    let opened = restored.open_note(&note.id()).expect("open restored");
    assert_eq!(opened.text(), body);
    assert_eq!(opened.meta().notebook_id, Some(notebook.id));
    assert_eq!(opened.meta().tags, vec!["travel"]);
    assert!(opened.meta().pinned);
    let restored_versions: Vec<String> = opened
        .history()
        .expect("history")
        .into_iter()
        .map(|version| version.id)
        .collect();
    assert_eq!(restored_versions, versions, "history is part of the image");

    assert_eq!(
        restored.last_pushed(&note.id()).expect("last pushed"),
        Some(pushed)
    );
    assert_eq!(restored.sync_state().expect("sync state"), sync);
    assert_eq!(
        restored.list_notebooks().expect("notebooks"),
        vec![notebook]
    );
    assert_eq!(
        restored
            .search(&SearchQuery::new("second"))
            .expect("search")
            .len(),
        1,
        "the FTS index is in the image"
    );
    let links = restored.note_attachments(&note.id()).expect("links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].hash, photo.hash);
    assert_eq!(
        restored.attachment_bytes(&photo.hash).expect("bytes"),
        Some(b"webp-photo-bytes".to_vec())
    );

    assert!(restored
        .open_note(&trashed.id())
        .expect("trashed note")
        .is_trashed());
    assert_eq!(
        restored.note_count().expect("count"),
        1,
        "trash is excluded"
    );
    assert_eq!(
        restored
            .list_notes(&NoteQuery {
                view: NoteView::All,
                limit: 0,
                ..NoteQuery::default()
            })
            .expect("all")
            .len(),
        2
    );
}

#[test]
fn an_image_without_the_header_is_rejected() {
    let error = SqliteStore::from_image(b"SQLite format 3\0 but not ours")
        .err()
        .expect("foreign bytes");
    assert!(error.to_string().contains("NBX1"), "{error}");
    assert!(SqliteStore::from_image(b"NBX1").is_err());
}

#[test]
fn change_count_moves_only_for_writes_and_survives_serialization() {
    let store = store();
    let before = store.change_count();
    let note = create(&store, "one", 1_000);
    let after_create = store.change_count();
    assert!(after_create > before);

    let _ = store.to_image().expect("image");
    assert_eq!(store.change_count(), after_create, "serialize is a read");

    note.replace_text("two").expect("edit");
    store.save_note(&note, 2_000).expect("save");
    assert!(store.change_count() > after_create);
}
