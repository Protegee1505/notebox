//! S5: a text box drives Loro through prefix/suffix replacements.

use notes_core::{diff_replacement, NoteDocument};

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
}

const PIECES: &[&str] = &[
    "a", "b", " ", "\n", "- [ ] ", "é", "😀", "中文", "word ", "**bold**",
];

fn byte_offset(text: &str, char_pos: usize) -> usize {
    text.char_indices()
        .nth(char_pos)
        .map_or(text.len(), |(offset, _)| offset)
}

fn random_buffer_edit(buffer: &mut String, rng: &mut Rng) {
    let len = buffer.chars().count();
    match rng.below(10) {
        0..=5 => {
            let pos = rng.below(len + 1);
            let byte = byte_offset(buffer, pos);
            let piece = PIECES[rng.below(PIECES.len())];
            buffer.insert_str(byte, piece);
        }
        6..=8 if len > 0 => {
            let pos = rng.below(len);
            let remove = 1 + rng.below((len - pos).min(5));
            let start = byte_offset(buffer, pos);
            let end = byte_offset(buffer, pos + remove);
            buffer.replace_range(start..end, "");
        }
        _ if len > 0 => {
            let pos = rng.below(len);
            let remove = 1 + rng.below((len - pos).min(4));
            let start = byte_offset(buffer, pos);
            let end = byte_offset(buffer, pos + remove);
            let piece = PIECES[rng.below(PIECES.len())];
            buffer.replace_range(start..end, piece);
        }
        _ => {
            let piece = PIECES[rng.below(PIECES.len())];
            buffer.push_str(piece);
        }
    }
}

#[test]
fn one_thousand_random_textbox_edits_converge() {
    let doc = NoteDocument::new();
    let mut buffer = String::new();
    let mut rng = Rng::new(0xB0A7_2026);

    for step in 0..1_000 {
        random_buffer_edit(&mut buffer, &mut rng);
        let edit = doc.apply_edit(&buffer).expect("apply edit");
        assert!(
            !edit.is_noop() || doc.text() == buffer,
            "step {step}: unexpected no-op"
        );
        assert_eq!(doc.text(), buffer, "step {step} diverged");
        if step % 25 == 0 {
            doc.commit();
        }
    }
    doc.commit();

    // A second replica must be able to import the converged text and continue
    // editing without a full snapshot.
    let peer = doc.fork();
    let update = doc.updates_since(&peer.version()).expect("updates");
    peer.import(&update).expect("import");
    assert_eq!(peer.text(), buffer);

    let restored =
        NoteDocument::from_snapshot(&doc.snapshot().expect("snapshot")).expect("restore");
    assert_eq!(restored.text(), buffer);
}

#[test]
fn typing_at_the_end_produces_one_insert() {
    let doc = NoteDocument::new();
    doc.insert(0, "hello").expect("insert");
    doc.commit();

    let edit = doc.apply_edit("hello!").expect("edit");
    assert_eq!(edit.char_pos, 5);
    assert_eq!(edit.removed_chars, 0);
    assert_eq!(edit.inserted, "!");
    assert_eq!(doc.text(), "hello!");
}

#[test]
fn replacing_a_selection_keeps_the_surrounding_text() {
    let doc = NoteDocument::new();
    doc.insert(0, "hello world").expect("insert");
    doc.commit();

    let edit = doc.apply_edit("hello brave world").expect("edit");
    assert_eq!(edit.char_pos, 6);
    assert_eq!(edit.removed_chars, 0);
    assert_eq!(edit.inserted, "brave ");
    assert_eq!(doc.text(), "hello brave world");
}

#[test]
fn an_unchanged_value_is_a_noop() {
    let doc = NoteDocument::new();
    doc.insert(0, "same").expect("insert");
    let edit = doc.apply_edit("same").expect("edit");
    assert!(edit.is_noop());
    assert_eq!(doc.char_len(), 4);
}

#[test]
fn a_remote_insert_does_not_move_the_logical_cursor() {
    let doc = NoteDocument::new();
    doc.insert(0, "hello").expect("insert");
    doc.commit();

    // The caret sits at the end of "hello".
    let text = doc.text_container();
    let cursor = text.get_cursor(5, Default::default()).expect("cursor");

    let remote = doc.fork();
    remote.insert(0, "oh ").expect("remote insert");
    remote.commit();

    let status = doc
        .import(&remote.updates_since(&doc.version()).expect("updates"))
        .expect("import");
    assert!(status.pending.is_none());
    assert_eq!(doc.text(), "oh hello");

    // The cursor still points after "hello", now at scalar position 8.
    let resolved = doc.as_loro().get_cursor_pos(&cursor).expect("cursor pos");
    assert_eq!(resolved.current.pos, 8);
}

#[test]
fn diff_positions_are_unicode_scalars() {
    let edit = diff_replacement("😀a", "😀b");
    assert_eq!(edit.char_pos, 1);
    assert_eq!(edit.removed_chars, 1);
    assert_eq!(edit.inserted, "b");
}
