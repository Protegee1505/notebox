//! P1 store tests: CRUD, metadata, trash, search, sync plumbing and export.

use std::time::Instant;

use notes_core::{encode_version_vector, Note};
use notes_proto::{NoteId, NotebookId};
use notes_store::{
    ImportReport, NewNote, NoteQuery, NoteStore, NoteView, SearchQuery, SqliteStore, SyncState,
};

fn store() -> SqliteStore {
    SqliteStore::open_in_memory().expect("in-memory store")
}

fn create(store: &SqliteStore, text: &str, at: i64) -> Note {
    store.create_note(NewNote::new(text, at)).expect("create")
}

#[test]
fn a_note_round_trips_through_the_store() {
    let store = store();
    let note = create(&store, "# Hello\n\nbody text", 1_000);

    let opened = store.open_note(&note.id()).expect("open");
    assert_eq!(opened.text(), "# Hello\n\nbody text");
    assert_eq!(opened.meta().created_at, 1_000);

    let summary = store.note_summary(&note.id()).expect("summary");
    assert_eq!(summary.title, "Hello");
    assert_eq!(summary.preview, "body text");
    assert_eq!(summary.updated_at, 1_000);
    assert!(summary.trashed_at.is_none());
    assert_eq!(store.note_count().expect("count"), 1);
}

#[test]
fn saving_updates_the_summary_and_the_search_index() {
    let store = store();
    let note = create(&store, "first version", 1_000);

    note.replace_text("second version").expect("edit");
    let summary = store.save_note(&note, 1_100).expect("save");
    assert_eq!(summary.title, "second version");
    assert_eq!(summary.updated_at, 1_100);

    assert!(store
        .search(&SearchQuery::new("second"))
        .expect("search")
        .iter()
        .any(|hit| hit.note.id == note.id()));
    assert!(store
        .search(&SearchQuery::new("first"))
        .expect("search")
        .is_empty());
}

#[test]
fn pinned_notes_sort_first_then_recently_updated() {
    let store = store();
    let old = create(&store, "old", 1_000);
    let middle = create(&store, "middle", 2_000);
    let new = create(&store, "new", 3_000);

    old.set_pinned(true).expect("pin");
    store.save_note(&old, 4_000).expect("save");

    let listed = store
        .list_notes(&NoteQuery::page(NoteView::Active))
        .expect("list");
    let titles: Vec<&str> = listed.iter().map(|note| note.title.as_str()).collect();
    assert_eq!(titles, vec!["old", "new", "middle"]);
    assert_eq!(listed[0].id, old.id());
    assert_eq!(listed[1].id, new.id());
    assert_eq!(listed[2].id, middle.id());
}

#[test]
fn list_paging_and_notebook_filter_work() {
    let store = store();
    let notebook = store.create_notebook("Work", 1_000).expect("notebook");
    for index in 0..5 {
        let mut input = NewNote::new(format!("note {index}"), 2_000 + index);
        input.notebook_id = Some(notebook.id);
        store.create_note(input).expect("create");
    }
    create(&store, "loose note", 3_000);

    let page = store
        .list_notes(&NoteQuery {
            view: NoteView::Active,
            limit: 2,
            offset: 1,
            ..NoteQuery::default()
        })
        .expect("page");
    assert_eq!(page.len(), 2);

    let in_notebook = store
        .list_notes(&NoteQuery {
            view: NoteView::Active,
            notebook_id: Some(notebook.id),
            limit: 0,
            ..NoteQuery::default()
        })
        .expect("notebook");
    assert_eq!(in_notebook.len(), 5);
    assert!(in_notebook
        .iter()
        .all(|note| note.notebook_id == Some(notebook.id)));
}

#[test]
fn tags_round_trip_and_filter() {
    let store = store();
    let tagged = create(&store, "tagged note", 1_000);
    let plain = create(&store, "plain note", 2_000);

    tagged.add_tag("ideas").expect("tag");
    tagged.add_tag("work").expect("tag");
    store.save_note(&tagged, 1_100).expect("save");

    let summary = store.note_summary(&tagged.id()).expect("summary");
    assert_eq!(summary.tags, vec!["ideas", "work"]);

    let tagged_only = store
        .list_notes(&NoteQuery {
            view: NoteView::Active,
            tag: Some("ideas".to_string()),
            limit: 0,
            ..NoteQuery::default()
        })
        .expect("list");
    assert_eq!(tagged_only.len(), 1);
    assert_eq!(tagged_only[0].id, tagged.id());

    let counts = store.list_tags().expect("tags");
    assert_eq!(counts.len(), 2);
    assert_eq!(counts[0].tag, "ideas");
    assert_eq!(counts[0].count, 1);

    tagged.remove_tag("ideas").expect("untag");
    store.save_note(&tagged, 1_200).expect("save");
    let counts = store.list_tags().expect("tags");
    assert_eq!(counts.len(), 1);

    assert_eq!(
        store.note_summary(&plain.id()).expect("summary").tags,
        Vec::<String>::new()
    );
}

#[test]
fn trash_restore_and_empty_keep_every_step_reversible_until_emptied() {
    let store = store();
    let note = create(&store, "keep me", 1_000);

    assert!(store.trash_note(&note.id(), 1_500).expect("trash"));
    assert!(!store.trash_note(&note.id(), 1_600).expect("trash again"));

    assert!(store
        .list_notes(&NoteQuery::page(NoteView::Active))
        .expect("active")
        .is_empty());
    let trash = store
        .list_notes(&NoteQuery::page(NoteView::Trash))
        .expect("trash view");
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].trashed_at, Some(1_500));

    // The note still opens while trashed: nothing is deleted yet.
    let opened = store.open_note(&note.id()).expect("open");
    assert!(opened.is_trashed());
    assert_eq!(opened.text(), "keep me");

    assert!(store.restore_note(&note.id(), 1_700).expect("restore"));
    assert_eq!(store.note_count().expect("count"), 1);
    assert!(store
        .list_notes(&NoteQuery::page(NoteView::Trash))
        .expect("trash view")
        .is_empty());

    store.trash_note(&note.id(), 1_800).expect("trash");
    assert_eq!(store.empty_trash().expect("empty"), 1);
    assert_eq!(store.empty_trash().expect("empty again"), 0);
    assert!(store.open_note(&note.id()).is_err());
    assert!(store
        .search(&SearchQuery::new("keep"))
        .expect("search")
        .is_empty());
}

#[test]
fn archived_notes_leave_the_active_list_but_stay_searchable() {
    let store = store();
    let note = create(&store, "archived gem", 1_000);
    note.set_archived(true).expect("archive");
    store.save_note(&note, 1_100).expect("save");

    assert!(store
        .list_notes(&NoteQuery::page(NoteView::Active))
        .expect("active")
        .is_empty());
    assert_eq!(
        store
            .list_notes(&NoteQuery::page(NoteView::Archived))
            .expect("archived")
            .len(),
        1
    );
    assert!(
        store
            .search(&SearchQuery {
                text: "gem".to_string(),
                view: NoteView::Archived,
                ..SearchQuery::default()
            })
            .expect("search")
            .len()
            == 1
    );
    assert!(store
        .search(&SearchQuery::new("gem"))
        .expect("search")
        .is_empty());
}

#[test]
fn search_matches_title_and_body_by_prefix() {
    let store = store();
    create(&store, "The quick brown fox", 1_000);
    create(&store, "A lazy dog sleeps", 2_000);
    create(&store, "Quick notes about quicksilver", 3_000);

    let hits = store.search(&SearchQuery::new("quick")).expect("search");
    assert_eq!(hits.len(), 2);

    let hits = store
        .search(&SearchQuery::new("brown fox!"))
        .expect("search");
    assert_eq!(hits.len(), 1);
    assert!(
        hits[0].snippet.contains("[brown]"),
        "snippet: {}",
        hits[0].snippet
    );

    // FTS5 operators typed by a person are plain words.
    let hits = store.search(&SearchQuery::new("OR")).expect("search");
    assert!(hits.iter().all(|hit| hit.rank.is_finite()));

    assert!(store
        .search(&SearchQuery::new("   "))
        .expect("search")
        .is_empty());
}

#[test]
fn search_respects_notebook_and_tag_filters() {
    let store = store();
    let notebook = store.create_notebook("Ideas", 1_000).expect("notebook");
    let mut input = NewNote::new("shared word here", 2_000);
    input.notebook_id = Some(notebook.id);
    input.tags = vec!["spark".to_string()];
    let note = store.create_note(input).expect("create");
    create(&store, "shared word there", 3_000);

    let hits = store.search(&SearchQuery::new("shared")).expect("search");
    assert_eq!(hits.len(), 2);

    let hits = store
        .search(&SearchQuery {
            text: "shared".to_string(),
            notebook_id: Some(notebook.id),
            ..SearchQuery::default()
        })
        .expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].note.id, note.id());

    let hits = store
        .search(&SearchQuery {
            text: "shared".to_string(),
            tag: Some("spark".to_string()),
            ..SearchQuery::default()
        })
        .expect("search");
    assert_eq!(hits.len(), 1);
}

#[test]
fn notebooks_round_trip_through_the_workspace_snapshot() {
    let store = store();
    let first = store.create_notebook("  Inbox ", 1_000).expect("first");
    let second = store.create_notebook("Work", 2_000).expect("second");
    assert_eq!(first.name, "Inbox");
    assert_eq!(
        store.list_notebooks().expect("list"),
        vec![first.clone(), second.clone()]
    );

    assert!(store
        .rename_notebook(&second.id, "Clients")
        .expect("rename"));
    assert!(!store
        .rename_notebook(&NotebookId::new(), "Nowhere")
        .expect("rename missing"));
    assert!(store.delete_notebook(&first.id).expect("delete"));
    assert!(!store.delete_notebook(&first.id).expect("delete again"));

    let notebooks = store.list_notebooks().expect("list");
    assert_eq!(notebooks.len(), 1);
    assert_eq!(notebooks[0].name, "Clients");

    // A second store imports the workspace snapshot and sees the same list.
    let other = SqliteStore::open_in_memory().expect("other store");
    other
        .import_workspace(&store.workspace_snapshot().expect("snapshot"))
        .expect("import workspace");
    assert_eq!(other.list_notebooks().expect("list"), notebooks);

    assert!(store
        .create_notebook("   ", 3_000)
        .expect_err("empty name")
        .to_string()
        .contains("empty"));
}

#[test]
fn remote_updates_merge_into_an_existing_note() {
    let left = store();
    let right = store();
    let note = create(&left, "one", 1_000);

    let update = note.document().all_updates().expect("updates");
    right
        .apply_update(&note.id(), &update, 1_100)
        .expect("apply");
    assert_eq!(right.open_note(&note.id()).expect("open").text(), "one");

    // The right device edits; the left device imports that edit.
    let edited = right.open_note(&note.id()).expect("open");
    edited.replace_text("one two").expect("edit");
    edited.commit_at(1_200);
    right.save_note(&edited, 1_200).expect("save");

    let update = edited
        .document()
        .updates_since(
            &left
                .open_note(&note.id())
                .expect("open")
                .document()
                .version(),
        )
        .expect("updates");
    let merged = left
        .apply_update(&note.id(), &update, 1_300)
        .expect("apply");
    assert_eq!(merged.text(), "one two");
    assert_eq!(right.open_note(&note.id()).expect("open").text(), "one two");
}

#[test]
fn an_update_for_an_unknown_note_creates_it() {
    let source = store();
    let note = create(&source, "arriving from another device", 1_000);
    let update = note.document().all_updates().expect("updates");

    let target = store();
    let id = NoteId::new();
    let created = target.apply_update(&id, &update, 2_000).expect("apply");
    assert_eq!(created.text(), "arriving from another device");
    assert_eq!(target.note_count().expect("count"), 1);
}

#[test]
fn an_imported_note_carries_no_local_history_of_its_own() {
    let source = store();
    let note = create(&source, "shared text", 1_000);
    let update = note.document().all_updates().expect("updates");

    let target = store();
    let created = target
        .apply_update(&note.id(), &update, 2_000)
        .expect("apply");
    assert_eq!(
        created.document().version(),
        note.document().version(),
        "an import must not add operations the server never sees"
    );

    // The importer edits; the creator must be able to apply that edit without
    // having received any local skeleton from the importer.
    let edited = target.open_note(&note.id()).expect("open");
    edited
        .insert(edited.char_len(), " and mine")
        .expect("insert");
    target.save_note(&edited, 2_100).expect("save");
    let diff = edited
        .document()
        .updates_since(&note.document().version())
        .expect("updates");
    let merged = source
        .apply_update(&note.id(), &diff, 2_200)
        .expect("apply");
    assert_eq!(merged.text(), "shared text and mine");
}

#[test]
fn saving_commits_a_pending_edit_so_it_can_be_pushed() {
    let store = store();
    let note = create(&store, "start", 1_000);
    let base = note.document().version();

    let edited = store.open_note(&note.id()).expect("open");
    edited.insert(edited.char_len(), " more").expect("insert");
    store.save_note(&edited, 1_100).expect("save");

    // The saved note exports the edit as a complete update: a replica at the
    // base version applies it without waiting for anything.
    let saved = store.open_note(&note.id()).expect("open");
    let diff = saved.document().updates_since(&base).expect("updates");
    let receiver = Note::from_snapshot(note.id(), &note.document().snapshot().expect("snapshot"))
        .expect("restore");
    let status = receiver.document().import(&diff).expect("import");
    assert!(status.pending.is_none(), "the edit is missing its commit");
    assert_eq!(receiver.text(), "start more");
}

#[test]
fn the_last_pushed_version_vector_round_trips() {
    let store = store();
    let note = create(&store, "sync me", 1_000);
    assert_eq!(store.last_pushed(&note.id()).expect("last pushed"), None);

    let version = encode_version_vector(&note.document().version());
    store
        .set_last_pushed(&note.id(), &version)
        .expect("set last pushed");
    assert_eq!(
        store.last_pushed(&note.id()).expect("last pushed"),
        Some(version)
    );
    assert!(store.set_last_pushed(&NoteId::new(), &[1, 2]).is_err());
}

#[test]
fn the_offline_queue_lists_notes_until_the_server_has_them() {
    let store = store();
    assert!(store.unsynced_notes(0).expect("queue").is_empty());

    let first = create(&store, "first", 1_000);
    let second = create(&store, "second", 1_100);
    assert_eq!(
        store.unsynced_notes(0).expect("queue"),
        vec![second.id(), first.id()],
        "most recently saved first"
    );
    assert_eq!(store.unsynced_notes(1).expect("queue").len(), 1);

    let version = encode_version_vector(
        &store
            .open_note(&first.id())
            .expect("open")
            .document()
            .version(),
    );
    store
        .set_last_pushed(&first.id(), &version)
        .expect("mark pushed");
    assert_eq!(store.unsynced_notes(0).expect("queue"), vec![second.id()]);

    // An edit after a push puts the note back in the queue.
    let note = store.open_note(&first.id()).expect("open");
    note.insert(0, "edited ").expect("insert");
    store.save_note(&note, 1_200).expect("save");
    assert_eq!(
        store.unsynced_notes(0).expect("queue"),
        vec![first.id(), second.id()]
    );
}

#[test]
fn a_remote_update_does_not_add_a_clean_note_to_the_queue() {
    let store = store();
    let note = create(&store, "shared text", 1_000);
    let pushed = encode_version_vector(&note.document().version());
    store
        .set_last_pushed(&note.id(), &pushed)
        .expect("mark pushed");

    // Another device edits the same note; the update arrives from the server.
    let remote = note.document().fork();
    remote.insert(0, "hello ").expect("insert");
    let update = remote
        .updates_since(&note.document().version())
        .expect("updates");
    store
        .apply_update(&note.id(), &update, 2_000)
        .expect("apply");

    assert!(
        store.unsynced_notes(0).expect("queue").is_empty(),
        "an imported update is not a local change"
    );
    assert_eq!(
        store.open_note(&note.id()).expect("open").text(),
        "hello shared text"
    );

    // A note with local changes stays in the queue when a remote update lands.
    let note = store.open_note(&note.id()).expect("open");
    note.insert(0, "local ").expect("insert");
    store.save_note(&note, 2_100).expect("save");
    let remote = note.document().fork();
    remote.insert(0, "remote ").expect("insert");
    let update = remote
        .updates_since(&note.document().version())
        .expect("updates");
    store
        .apply_update(&note.id(), &update, 2_200)
        .expect("apply");
    assert_eq!(store.unsynced_notes(0).expect("queue"), vec![note.id()]);
}

#[test]
fn the_workspace_push_queue_follows_notebook_changes() {
    let store = store();
    assert_eq!(store.workspace_pending().expect("pending"), None);

    let notebook = store.create_notebook("Work", 1_000).expect("notebook");
    let pending = store.workspace_pending().expect("pending").expect("push");
    assert!(!pending.updates.is_empty());

    store
        .mark_workspace_pushed(&pending.version)
        .expect("mark pushed");
    assert_eq!(store.workspace_pending().expect("pending"), None);

    store
        .rename_notebook(&notebook.id, "Work notes")
        .expect("rename");
    let pending = store.workspace_pending().expect("pending").expect("push");
    store
        .mark_workspace_pushed(&pending.version)
        .expect("mark pushed");

    // A remote notebook list lands without queuing a push back.
    let other = SqliteStore::open_in_memory().expect("in-memory store");
    other.create_notebook("Home", 2_000).expect("notebook");
    let pushed = other.workspace_pending().expect("pending").expect("push");
    store
        .import_workspace(&pushed.updates)
        .expect("import workspace");
    assert_eq!(store.workspace_pending().expect("pending"), None);
    let names: Vec<String> = store
        .list_notebooks()
        .expect("notebooks")
        .into_iter()
        .map(|notebook| notebook.name)
        .collect();
    assert_eq!(
        names,
        vec!["Work notes".to_string(), "Home".to_string()],
        "the remote notebook arrives, the local one stays"
    );
}

#[test]
fn the_sync_cursor_round_trips() {
    let store = store();
    assert_eq!(store.sync_state().expect("state"), SyncState::default());

    let state = SyncState {
        cursor: Some(9_008_007),
        updated_at: 4_200,
    };
    store.set_sync_state(&state).expect("set");
    assert_eq!(store.sync_state().expect("state"), state);
}

#[test]
fn export_and_import_round_trip_notes_notebooks_and_tags() {
    let source = store();
    let notebook = source.create_notebook("Work", 1_000).expect("notebook");
    let mut input = NewNote::new("# Report\n\nnumbers and words", 2_000);
    input.notebook_id = Some(notebook.id);
    input.tags = vec!["work".to_string(), "2026".to_string()];
    let note = source.create_note(input).expect("create");
    note.set_pinned(true).expect("pin");
    source.save_note(&note, 2_100).expect("save");

    let trashed = create(&source, "old draft", 1_500);
    source.trash_note(&trashed.id(), 1_600).expect("trash");

    let bytes = source.export_zip(5_000).expect("export");
    assert!(bytes.len() > 100);

    let target = store();
    let report = target.import_zip(&bytes).expect("import");
    assert_eq!(
        report,
        ImportReport {
            notes_created: 2,
            notes_updated: 0,
            notebooks: 1,
            attachments: 0,
        }
    );

    assert_eq!(
        target.list_notebooks().expect("notebooks"),
        vec![notebook.clone()]
    );
    let all = target
        .list_notes(&NoteQuery {
            view: NoteView::All,
            limit: 0,
            ..NoteQuery::default()
        })
        .expect("list");
    assert_eq!(all.len(), 2);

    let imported = target.open_note(&note.id()).expect("open");
    assert_eq!(imported.text(), "# Report\n\nnumbers and words");
    let meta = imported.meta();
    assert_eq!(meta.notebook_id, Some(notebook.id));
    assert_eq!(meta.tags, vec!["work", "2026"]);
    assert!(meta.pinned);
    assert_eq!(meta.created_at, 2_000);

    let imported_trash = target.open_note(&trashed.id()).expect("open");
    assert!(imported_trash.is_trashed());
    assert_eq!(imported_trash.text(), "old draft");

    assert_eq!(
        target
            .search(&SearchQuery::new("numbers"))
            .expect("search")
            .len(),
        1
    );
}

#[test]
fn importing_the_same_archive_twice_does_not_duplicate_or_lose_notes() {
    let source = store();
    let note = create(&source, "stable text", 1_000);
    let bytes = source.export_zip(2_000).expect("export");

    let target = store();
    let first = target.import_zip(&bytes).expect("first import");
    assert_eq!(first.notes_created, 1);

    let second = target.import_zip(&bytes).expect("second import");
    assert_eq!(second.notes_updated, 1);
    assert_eq!(second.notes_created, 0);
    assert_eq!(target.note_count().expect("count"), 1);
    assert_eq!(
        target.open_note(&note.id()).expect("open").text(),
        "stable text"
    );
}

#[test]
fn a_file_database_survives_reopening() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("notebox.db");
    let id;
    {
        let store = SqliteStore::open(&path).expect("open");
        let note = create(&store, "persisted", 1_000);
        id = note.id();
    }
    let reopened = SqliteStore::open(&path).expect("reopen");
    assert_eq!(reopened.open_note(&id).expect("open").text(), "persisted");
    assert_eq!(reopened.note_count().expect("count"), 1);
}

#[test]
fn ten_thousand_notes_search_under_100_ms() {
    let store = store();
    let words = ["alpha", "brown", "cobalt", "delta", "ember", "frost"];
    let started = Instant::now();
    for index in 0..10_000 {
        let body = format!(
            "note {index}: {} {} fox jumps over the lazy dog",
            words[index % words.len()],
            words[(index / words.len()) % words.len()]
        );
        store
            .create_note(NewNote::new(body, 1_000 + index as i64))
            .expect("create");
    }
    let seeded = started.elapsed();

    let started = Instant::now();
    let hits = store
        .search(&SearchQuery {
            text: "note 9999".to_string(),
            limit: 20,
            ..SearchQuery::default()
        })
        .expect("search");
    let elapsed = started.elapsed();

    println!(
        "10,000 notes: seeded in {} ms, search in {} ms, {} hits",
        seeded.as_millis(),
        elapsed.as_millis(),
        hits.len()
    );
    assert!(!hits.is_empty(), "the seeded note must be found");
    assert!(
        elapsed.as_millis() < 100,
        "search took {} ms",
        elapsed.as_millis()
    );
}
