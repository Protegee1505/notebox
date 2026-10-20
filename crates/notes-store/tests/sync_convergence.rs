//! P1 store property test: three stores, random edits and metadata, random
//! update order. The CRDT must converge and every store must report the same
//! list, search and history.

use notes_core::{Note, NoteMeta};
use notes_proto::NoteId;
use notes_store::{NewNote, NoteQuery, NoteStore, NoteView, SearchQuery, SqliteStore};

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, upper: usize) -> usize {
        if upper == 0 {
            0
        } else {
            (self.next_u64() % upper as u64) as usize
        }
    }

    fn flip(&mut self) -> bool {
        self.next_u64().is_multiple_of(2)
    }
}

const WORDS: &[&str] = &["alpha", "beta", "gamma", "delta", "😀", "中"];
const TAGS: &[&str] = &["ideas", "work", "home"];

fn random_change(note: &Note, rng: &mut Rng, clock: i64) {
    match rng.below(8) {
        0..=3 => {
            let position = rng.below(note.char_len() + 1);
            note.insert(position, WORDS[rng.below(WORDS.len())])
                .expect("insert");
        }
        4 => note.set_pinned(rng.flip()).expect("pin"),
        5 => note.set_archived(rng.flip()).expect("archive"),
        6 => {
            note.add_tag(TAGS[rng.below(TAGS.len())]).expect("tag");
        }
        _ => {
            if note.is_trashed() {
                note.restore().expect("restore");
            } else {
                note.trash(clock).expect("trash");
            }
        }
    }
    note.commit_at(clock);
}

fn sync(from: &SqliteStore, to: &SqliteStore, id: &NoteId, at: i64) {
    let source = from.open_note(id).expect("source note");
    let target = to.open_note(id).expect("target note");
    let update = source
        .document()
        .updates_since(&target.document().version())
        .expect("updates");
    if !update.is_empty() {
        to.apply_update(id, &update, at).expect("apply update");
    }
}

#[test]
fn a_restored_version_travels_to_another_store() {
    let first = SqliteStore::open_in_memory().expect("first store");
    let second = SqliteStore::open_in_memory().expect("second store");

    // A note written in one sitting, then edited more than five minutes later.
    let created = first
        .create_note(NewNote::new("first draft", 1_000))
        .expect("seed");
    let id = created.id();
    let update = created.document().all_updates().expect("updates");
    second.apply_update(&id, &update, 1_000).expect("apply");
    drop(created);

    let edited = first.open_note(&id).expect("open");
    edited.set_next_commit_timestamp(1_601);
    edited.replace_text("second draft").expect("edit");
    first.save_note(&edited, 1_601).expect("save");
    sync(&first, &second, &id, 1_700);

    let versions = edited.history().expect("history");
    assert_eq!(versions.len(), 2, "two sittings make two versions");
    let older = versions.first().expect("older version").clone();
    assert_eq!(edited.text_at(&older).expect("preview"), "first draft");

    // The other device sees the edit, then restores the older version as a new
    // edit of its own. Nothing is removed from either history.
    let target = second.open_note(&id).expect("open");
    assert_eq!(target.text(), "second draft");
    target.set_next_commit_timestamp(2_001);
    assert!(target.restore_version(&older).expect("restore"));
    second.save_note(&target, 2_001).expect("save");
    sync(&second, &first, &id, 2_100);

    assert_eq!(
        first.open_note(&id).expect("open").text(),
        "first draft",
        "the restored text reaches the first device"
    );
    let first_history = first
        .open_note(&id)
        .expect("open")
        .history()
        .expect("history");
    let second_history = second
        .open_note(&id)
        .expect("open")
        .history()
        .expect("history");
    assert_eq!(first_history.len(), 3, "restore appended a version");
    assert_eq!(
        first_history
            .iter()
            .map(|version| version.id.clone())
            .collect::<Vec<_>>(),
        second_history
            .iter()
            .map(|version| version.id.clone())
            .collect::<Vec<_>>(),
        "both devices group the same history"
    );
    assert!(
        first_history.iter().any(|version| version.id == older.id),
        "the restored version still exists"
    );
}

#[test]
fn three_stores_converge_under_random_updates_and_metadata_changes() {
    let stores = [
        SqliteStore::open_in_memory().expect("store 1"),
        SqliteStore::open_in_memory().expect("store 2"),
        SqliteStore::open_in_memory().expect("store 3"),
    ];

    // Seed the note on the first store, then hand the same id to the others.
    let seed = stores[0]
        .create_note(NewNote::new("the quick brown fox", 1_000))
        .expect("seed");
    let id = seed.id();
    let update = seed.document().all_updates().expect("updates");
    stores[1].apply_update(&id, &update, 1_000).expect("apply");
    stores[2].apply_update(&id, &update, 1_000).expect("apply");

    let mut rng = Rng::new(0xA11CE);
    let mut clocks = [2_000i64, 3_000, 4_000];
    for _ in 0..120 {
        let index = rng.below(stores.len());
        let store = &stores[index];
        let note = store.open_note(&id).expect("open");
        random_change(&note, &mut rng, clocks[index]);
        store.save_note(&note, clocks[index]).expect("save");
        clocks[index] += 1;

        if rng.below(2) == 0 && rng.below(3) != 0 {
            let from = rng.below(stores.len());
            let mut to = rng.below(stores.len());
            if to == from {
                to = (to + 1) % stores.len();
            }
            sync(&stores[from], &stores[to], &id, clocks[to]);
            clocks[to] += 1;
        }
    }

    // Full mesh twice: missed updates and their dependencies both travel.
    for _ in 0..2 {
        for from in 0..stores.len() {
            for to in 0..stores.len() {
                if from != to {
                    sync(&stores[from], &stores[to], &id, clocks[to]);
                    clocks[to] += 1;
                }
            }
        }
    }

    let expected_text = stores[0].open_note(&id).expect("open").text();
    let expected_meta = stores[0].open_note(&id).expect("open").meta();
    let expected_version = stores[0].open_note(&id).expect("open").document().version();
    let expected_history: Vec<String> = stores[0]
        .open_note(&id)
        .expect("open")
        .history()
        .expect("history")
        .iter()
        .map(|version| version.id.clone())
        .collect();

    for store in &stores {
        let note = store.open_note(&id).expect("open");
        assert_eq!(note.text(), expected_text, "text diverged");
        assert_eq!(note.meta(), expected_meta, "metadata diverged");
        assert_eq!(note.document().version(), expected_version);
        let ids: Vec<String> = note
            .history()
            .expect("history")
            .iter()
            .map(|version| version.id.clone())
            .collect();
        assert_eq!(ids, expected_history);

        let summaries = store
            .list_notes(&NoteQuery {
                view: NoteView::All,
                limit: 0,
                ..NoteQuery::default()
            })
            .expect("list");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, id);
        assert_eq!(summaries[0].title, note.title());
        assert_eq!(summaries[0].preview, note.preview());
        assert_eq!(summaries[0].tags, note.meta().tags);
        assert_eq!(summaries[0].trashed_at.is_some(), note.meta().is_trashed());

        let hits = store
            .search(&SearchQuery {
                text: "quick".to_string(),
                view: NoteView::All,
                ..SearchQuery::default()
            })
            .expect("search");
        assert!(
            hits.iter().any(|hit| hit.note.id == id) || !expected_text.contains("quick"),
            "search sees the converged text"
        );
    }

    // The metadata is readable, not just equal across stores.
    let meta: NoteMeta = expected_meta;
    assert!(meta.tags.len() <= TAGS.len());

    // A snapshot of the converged note rebuilds the same history.
    let restored = Note::from_snapshot(
        id,
        &stores[0]
            .open_note(&id)
            .expect("open")
            .document()
            .snapshot()
            .expect("snapshot"),
    )
    .expect("restore");
    assert_eq!(restored.text(), expected_text);
    assert_eq!(restored.meta(), meta);
}
