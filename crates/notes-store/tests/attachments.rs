//! P6 store tests: content-addressed attachments, links and export bytes.

use std::io::Cursor;

use notes_core::{attachment_url, content_hash, read_export_zip, ExportBundle};
use notes_store::{export_bundle, NewNote, NoteStore, SqliteStore, SCHEMA_VERSION};

fn store() -> SqliteStore {
    SqliteStore::open_in_memory().expect("in-memory store")
}

fn create(store: &SqliteStore, text: &str, at: i64) -> notes_core::Note {
    store.create_note(NewNote::new(text, at)).expect("create")
}

#[test]
fn bytes_round_trip_and_identical_data_is_stored_once() {
    let store = store();
    let photo = store
        .put_attachment("../Camera/IMG 1.PNG", "image/webp", b"webp-bytes", 1_000)
        .expect("put");
    assert_eq!(photo.name, "IMG 1.webp");
    assert_eq!(photo.mime, "image/webp");
    assert_eq!(photo.size, 10);
    assert!(photo.available);
    assert!(!photo.uploaded);
    assert_eq!(photo.hash, content_hash(b"webp-bytes"));

    let copy = store
        .put_attachment("copy.jpg", "image/jpeg", b"webp-bytes", 2_000)
        .expect("put again");
    assert_eq!(copy.hash, photo.hash);
    assert_eq!(copy.name, "IMG 1.webp", "the first display name wins");
    assert_eq!(copy.mime, "image/webp");

    assert_eq!(
        store.attachment_bytes(&photo.hash).expect("bytes"),
        Some(b"webp-bytes".to_vec())
    );
    assert_eq!(store.attachment(&photo.hash).expect("lookup"), Some(photo));
    assert_eq!(store.attachment("missing").expect("missing"), None);

    let usage = store.attachment_usage().expect("usage");
    assert_eq!(usage.count, 1);
    assert_eq!(usage.bytes, 10);
}

#[test]
fn saving_a_note_indexes_its_images_and_drops_removed_links() {
    let store = store();
    let photo = store
        .put_attachment("holiday.png", "image/webp", b"photo", 1_000)
        .expect("put");
    let note = create(&store, "Before", 1_000);

    let body = format!(
        "Before\n\n![holiday]({})",
        attachment_url(&photo.hash, &photo.mime)
    );
    note.replace_text(&body).expect("edit");
    store.save_note(&note, 1_100).expect("save");

    let linked = store.note_attachments(&note.id()).expect("links");
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].hash, photo.hash);
    assert_eq!(linked[0].name, "holiday.webp");

    // Removing the Markdown reference removes the note's use, not the bytes:
    // another note may still point at the same content hash.
    note.replace_text("Before").expect("edit");
    store.save_note(&note, 1_200).expect("save");
    assert!(store
        .note_attachments(&note.id())
        .expect("links")
        .is_empty());
    assert_eq!(
        store.attachment_bytes(&photo.hash).expect("bytes"),
        Some(b"photo".to_vec())
    );
}

#[test]
fn a_link_without_bytes_becomes_a_placeholder_until_the_data_arrives() {
    let store = store();
    let bytes = b"not here yet";
    let hash = content_hash(bytes);
    let note = create(&store, "Waiting", 1_000);
    let body = format!("Waiting\n\n![remote](attachments/{hash}.webp)");
    note.replace_text(&body).expect("edit");
    store.save_note(&note, 1_100).expect("save");

    let linked = store.note_attachments(&note.id()).expect("links");
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].hash, hash);
    assert!(!linked[0].available);
    assert_eq!(linked[0].mime, "image/webp");
    assert_eq!(linked[0].size, 0);
    assert_eq!(store.attachment_bytes(&hash).expect("bytes"), None);

    // Downloading the bytes fills the row and keeps the note's link.
    let filled = store
        .put_attachment("remote.webp", "image/webp", bytes, 1_200)
        .expect("fill");
    assert!(filled.available);
    assert_eq!(filled.size, bytes.len() as u64);
    assert_eq!(
        store.attachment_bytes(&hash).expect("bytes"),
        Some(bytes.to_vec())
    );
    assert!(store.note_attachments(&note.id()).expect("links")[0].available);
}

#[test]
fn the_upload_queue_skips_data_the_server_already_has() {
    let store = store();
    let first = store
        .put_attachment("one.png", "image/png", b"one", 1_000)
        .expect("put");
    let second = store
        .put_attachment("two.png", "image/png", b"two", 2_000)
        .expect("put");

    let queue = store.unsynced_attachments(0).expect("queue");
    assert_eq!(queue, vec![first.hash.clone(), second.hash.clone()]);

    store
        .mark_attachment_uploaded(&first.hash, 3_000)
        .expect("mark");
    assert_eq!(
        store.unsynced_attachments(0).expect("queue"),
        vec![second.hash.clone()]
    );
    assert!(
        store
            .attachment(&first.hash)
            .expect("lookup")
            .unwrap()
            .uploaded
    );
    assert_eq!(
        store.unsynced_attachments(1).expect("limit"),
        vec![second.hash]
    );
}

#[test]
fn export_and_import_carry_attachment_bytes_and_links() {
    let source = store();
    let photo = source
        .put_attachment("holiday.png", "image/webp", b"image-bytes", 1_000)
        .expect("put");
    let note = create(&source, "Trip", 1_000);
    let body = format!(
        "Trip\n\n![holiday]({})",
        attachment_url(&photo.hash, &photo.mime)
    );
    note.replace_text(&body).expect("edit");
    source.save_note(&note, 1_100).expect("save");

    let bundle = export_bundle(&source, 2_000).expect("bundle");
    assert_eq!(bundle.attachments.len(), 1);
    assert_eq!(bundle.attachments[0].data, b"image-bytes");
    assert_eq!(bundle.attachments[0].note_id, Some(note.id()));

    let zip = source.export_zip(2_100).expect("export");
    let restored = read_export_zip(Cursor::new(&zip)).expect("read export");
    assert_eq!(restored.attachments[0].data, b"image-bytes");
    assert!(restored.notes[0]
        .text
        .contains(&attachment_url(&photo.hash, "image/webp")));

    let target = store();
    let report = target.import_zip(&zip).expect("import");
    assert_eq!(report.notes_created, 1);
    assert_eq!(report.attachments, 1);
    assert_eq!(
        target.attachment_bytes(&photo.hash).expect("bytes"),
        Some(b"image-bytes".to_vec())
    );
    let imported = target.open_note(&note.id()).expect("open");
    assert_eq!(imported.text(), body);
    let linked = target.note_attachments(&note.id()).expect("links");
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].hash, photo.hash);
    assert_eq!(linked[0].name, "holiday.webp");

    // Exporting again after the import produces the same attachment path.
    let second = target.export_zip(3_000).expect("export again");
    let second_bundle = read_export_zip(Cursor::new(&second)).expect("read again");
    assert_eq!(second_bundle.attachments[0].data, b"image-bytes");
}

#[test]
fn a_v3_database_keeps_its_legacy_attachment_metadata() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("v3.db");
    {
        let conn = rusqlite::Connection::open(&path).expect("open v3");
        conn.execute_batch(
            "CREATE TABLE attachment (
                 id         TEXT PRIMARY KEY,
                 note_id    TEXT,
                 name       TEXT NOT NULL,
                 mime       TEXT NOT NULL,
                 size       INTEGER NOT NULL,
                 created_at INTEGER NOT NULL
             );
             CREATE INDEX attachment_note ON attachment (note_id);
             INSERT INTO attachment (id, note_id, name, mime, size, created_at)
                 VALUES ('old', NULL, 'old.webp', 'image/webp', 0, 1);
             CREATE TABLE device (
                 id      INTEGER PRIMARY KEY CHECK (id = 1),
                 peer_id INTEGER NOT NULL
             );
             INSERT INTO device (id, peer_id) VALUES (1, 42);
             PRAGMA user_version = 3;",
        )
        .expect("v3 schema");
    }

    let store = SqliteStore::open(&path).expect("migrate");
    assert_eq!(SCHEMA_VERSION, 4);
    assert!(store
        .note_attachments(&notes_proto::NoteId::new())
        .expect("links")
        .is_empty());
    drop(store);

    let conn = rusqlite::Connection::open(&path).expect("reopen");
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(version, 4);
    let legacy: (String, String) = conn
        .query_row("SELECT name, mime FROM attachment_legacy", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .expect("legacy row survives");
    assert_eq!(legacy, ("old.webp".to_string(), "image/webp".to_string()));
    let new_rows: i64 = conn
        .query_row("SELECT count(*) FROM attachment", [], |row| row.get(0))
        .expect("new table");
    assert_eq!(new_rows, 0);
}

#[test]
fn an_empty_export_still_writes_a_manifest() {
    let store = store();
    let zip = store.export_zip(1_000).expect("export");
    let bundle: ExportBundle = read_export_zip(Cursor::new(&zip)).expect("read");
    assert!(bundle.notes.is_empty());
    assert!(bundle.attachments.is_empty());
}
