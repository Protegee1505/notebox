//! S2: independent replicas converge under random edits and sync order.

use notes_core::NoteDocument;

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

    fn pick<'a>(&mut self, values: &'a [&'a str]) -> &'a str {
        values[self.below(values.len())]
    }
}

const ALPHABET: &[&str] = &["a", "b", "c", " ", "\n", "é", "😀", "中"];

fn random_edit(doc: &NoteDocument, rng: &mut Rng) {
    match rng.below(10) {
        0..=5 => {
            let pos = rng.below(doc.char_len() + 1);
            let piece = rng.pick(ALPHABET);
            doc.insert(pos, piece).expect("insert");
        }
        6..=8 => {
            let len = doc.char_len();
            if len > 0 {
                let pos = rng.below(len);
                let amount = 1 + rng.below((len - pos).min(4));
                doc.delete(pos, amount).expect("delete");
            }
        }
        _ => {
            // Whole-value edit, the path the text box uses.
            let mut buffer = doc.text();
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
            doc.apply_edit(&buffer).expect("apply_edit");
        }
    }
    doc.commit();
}

fn byte_offset(text: &str, char_pos: usize) -> usize {
    text.char_indices()
        .nth(char_pos)
        .map_or(text.len(), |(offset, _)| offset)
}

fn sync(from: &NoteDocument, to: &NoteDocument) {
    let update = from.updates_since(&to.version()).expect("export updates");
    let status = to.import(&update).expect("import updates");
    assert!(
        status.pending.is_none(),
        "a full relay must not leave missing dependencies"
    );
}

#[test]
fn three_replicas_converge_under_random_sync_order() {
    let base = NoteDocument::new();
    base.insert(0, "The quick brown fox jumps over the lazy dog.")
        .expect("seed");
    base.commit();

    let docs = [base.fork(), base.fork(), base.fork()];
    for (i, doc) in docs.iter().enumerate() {
        doc.set_peer_id(1 + i as u64).expect("peer id");
    }

    let mut rng = Rng::new(0x5EED_2026);
    for _ in 0..120 {
        let replica = rng.below(docs.len());
        random_edit(&docs[replica], &mut rng);

        // Sometimes exchange one update, sometimes drop it: the final mesh
        // below must still repair every missed update.
        if rng.below(2) == 0 {
            let from = rng.below(docs.len());
            let mut to = rng.below(docs.len());
            if to == from {
                to = (to + 1) % docs.len();
            }
            if rng.below(3) != 0 {
                sync(&docs[from], &docs[to]);
            }
        }
    }

    // Full mesh, twice, so both updates and their dependencies are relayed.
    for _ in 0..2 {
        for from in 0..docs.len() {
            for to in 0..docs.len() {
                if from != to {
                    sync(&docs[from], &docs[to]);
                }
            }
        }
    }

    let expected = docs[0].text();
    for doc in &docs {
        assert_eq!(doc.text(), expected);
        assert_eq!(doc.version(), docs[0].version());
    }

    // The converged state must survive the snapshot round trip.
    let restored =
        NoteDocument::from_snapshot(&docs[0].snapshot().expect("snapshot")).expect("restore");
    assert_eq!(restored.text(), expected);
    assert_eq!(restored.version(), docs[0].version());
}

#[test]
fn a_restored_snapshot_keeps_history() {
    let doc = NoteDocument::new();
    doc.insert(0, "first").expect("insert");
    doc.commit();
    let first = doc.frontiers();

    doc.insert(5, " second").expect("insert");
    doc.commit();

    let restored =
        NoteDocument::from_snapshot(&doc.snapshot().expect("snapshot")).expect("restore");
    assert_eq!(restored.text(), "first second");

    restored.checkout(&first).expect("checkout");
    assert_eq!(restored.text(), "first");
    restored.checkout_latest();
    assert_eq!(restored.text(), "first second");
}
