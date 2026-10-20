//! P1 property test: three replicas with random edits, metadata, trash and
//! random merge order must end identical in text, metadata and history.

use notes_core::{Note, NoteMeta};
use notes_proto::{NoteId, NotebookId};

/// Small deterministic PRNG so a failure can be replayed exactly.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        // SplitMix64.
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

    fn pick<'a>(&mut self, values: &'a [&'a str]) -> &'a str {
        values[self.below(values.len())]
    }
}

const ALPHABET: &[&str] = &["a", "b", "c", " ", "\n", "é", "😀", "中"];
const TAGS: &[&str] = &["ideas", "work", "home", "later"];

fn random_edit(note: &Note, rng: &mut Rng) {
    match rng.below(10) {
        0..=5 => {
            let pos = rng.below(note.char_len() + 1);
            note.insert(pos, rng.pick(ALPHABET)).expect("insert");
        }
        6..=8 => {
            let len = note.char_len();
            if len > 0 {
                let pos = rng.below(len);
                let amount = 1 + rng.below((len - pos).min(4));
                note.delete(pos, amount).expect("delete");
            }
        }
        _ => {
            let mut buffer = note.text();
            if buffer.is_empty() {
                buffer.push('x');
            } else {
                let len = buffer.chars().count();
                let pos = rng.below(len);
                let byte = byte_offset(&buffer, pos);
                let remove = rng.below(3).min(len - pos);
                let end = byte_offset(&buffer, pos + remove);
                buffer.replace_range(byte..end, rng.pick(ALPHABET));
            }
            note.apply_edit(&buffer).expect("apply_edit");
        }
    }
}

fn random_metadata(note: &Note, rng: &mut Rng, notebook: NotebookId, now: i64) {
    match rng.below(8) {
        0 => note.set_pinned(rng.flip()).expect("pin"),
        1 => note.set_archived(rng.flip()).expect("archive"),
        2 => {
            note.add_tag(rng.pick(TAGS)).expect("add tag");
        }
        3 => {
            note.remove_tag(rng.pick(TAGS)).expect("remove tag");
        }
        4 => {
            let notebook = if rng.flip() { Some(notebook) } else { None };
            note.set_notebook(notebook).expect("notebook");
        }
        5 => note
            .set_color(if rng.flip() { Some("amber") } else { None })
            .expect("color"),
        6 => {
            note.trash(now).expect("trash");
        }
        _ => {
            note.restore().expect("restore");
        }
    }
}

fn byte_offset(text: &str, char_pos: usize) -> usize {
    text.char_indices()
        .nth(char_pos)
        .map_or(text.len(), |(offset, _)| offset)
}

fn sync(from: &Note, to: &Note) {
    let update = from
        .document()
        .updates_since(&to.document().version())
        .expect("export updates");
    let status = to.document().import(&update).expect("import updates");
    assert!(
        status.pending.is_none(),
        "a full relay must not leave missing dependencies"
    );
}

#[test]
fn three_replicas_converge_with_metadata_trash_and_history() {
    let id = NoteId::new();
    let notebook = NotebookId::new();
    let base = Note::new(id, 1_000).expect("base");
    base.insert(0, "The quick brown fox.").expect("seed");
    base.commit_at(1_000);

    let docs = [1, 2, 3].map(|peer| {
        let note =
            Note::from_snapshot(id, &base.document().snapshot().expect("snapshot")).expect("fork");
        note.document().set_peer_id(peer).expect("peer id");
        note
    });

    let mut rng = Rng::new(0x5EED_1234);
    let mut clocks = [2_000i64, 3_000, 4_000];
    for _ in 0..150 {
        let replica = rng.below(docs.len());
        let note = &docs[replica];
        if rng.below(3) == 0 {
            random_metadata(note, &mut rng, notebook, clocks[replica]);
        } else {
            random_edit(note, &mut rng);
        }
        clocks[replica] += 1 + rng.below(200) as i64;
        note.commit_at(clocks[replica]);

        // Exchange one update now and then; drops must be repaired later.
        if rng.below(2) == 0 && rng.below(3) != 0 {
            let from = rng.below(docs.len());
            let mut to = rng.below(docs.len());
            if to == from {
                to = (to + 1) % docs.len();
            }
            sync(&docs[from], &docs[to]);
        }
    }

    // Full mesh, twice, so updates and their dependencies both travel.
    for _ in 0..2 {
        for from in 0..docs.len() {
            for to in 0..docs.len() {
                if from != to {
                    sync(&docs[from], &docs[to]);
                }
            }
        }
    }

    let expected_text = docs[0].text();
    let expected_meta = docs[0].meta();
    let expected_version = docs[0].document().version();
    let expected_history: Vec<String> = docs[0]
        .history()
        .expect("history")
        .iter()
        .map(|version| version.id.clone())
        .collect();

    for note in &docs {
        assert_eq!(note.text(), expected_text, "text diverged");
        assert_eq!(note.meta(), expected_meta, "metadata diverged");
        assert_eq!(note.document().version(), expected_version);
        let history = note.history().expect("history");
        let ids: Vec<String> = history.iter().map(|version| version.id.clone()).collect();
        assert_eq!(ids, expected_history, "history grouping diverged");
        // Every version previews and diffs; with concurrent branches the newest
        // version is one device's view, not necessarily the merged current text.
        // Previewing samples the list: each preview forks the whole oplog.
        for (index, version) in history.iter().enumerate() {
            if index % 25 != 0 && index + 1 != history.len() {
                continue;
            }
            note.text_at(version).expect("preview");
            let diff = note.diff_from_current(version).expect("diff");
            assert!(
                diff.is_empty() || diff.contains('+') || diff.contains('-'),
                "a diff must show the change: {diff}"
            );
        }
    }

    // Metadata written by the property test must be readable, not just equal.
    let meta: NoteMeta = docs[0].meta();
    assert!(meta.created_at == 1_000 || meta.created_at == 0);
    assert!(meta.tags.len() <= TAGS.len());

    // A snapshot taken now must reproduce text, metadata and history.
    let restored = Note::from_snapshot(id, &docs[0].document().snapshot().expect("snapshot"))
        .expect("restore");
    assert_eq!(restored.text(), expected_text);
    assert_eq!(restored.meta(), expected_meta);
    assert_eq!(restored.document().version(), expected_version);
    let restored_history: Vec<String> = restored
        .history()
        .expect("history")
        .iter()
        .map(|version| version.id.clone())
        .collect();
    assert_eq!(restored_history, expected_history);
}

#[test]
fn a_trashed_note_survives_a_snapshot_round_trip() {
    let note = Note::new(NoteId::new(), 1_000).expect("note");
    note.insert(0, "keep me").expect("insert");
    note.trash(1_500).expect("trash");
    note.commit_at(1_500);

    let restored = Note::from_snapshot(note.id(), &note.document().snapshot().expect("snapshot"))
        .expect("restore");
    assert!(restored.is_trashed());
    assert_eq!(restored.text(), "keep me");
    assert!(restored.restore().expect("restore"));
    assert!(!restored.is_trashed());
}
