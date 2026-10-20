//! S2: a 10,000-edit note must stay compact enough for SQLite rows and D1.

use notes_core::NoteDocument;

#[test]
fn ten_thousand_edits_stay_compact() {
    let doc = NoteDocument::new();
    // Real prose, not one repeated character: Loro's columnar encoding must
    // handle varied text, and the compressed size must stay honest.
    const PIECES: &[&str] = &[
        "the ", "quick ", "brown ", "fox ", "jumps ", "over ", "lazy ", "dog. ", "0123", "\n",
    ];
    for edit in 0..10_000 {
        let pos = doc.char_len();
        doc.insert(pos, PIECES[edit % PIECES.len()])
            .expect("insert");
        // Simulate a debounced autosave: ten changes per hundred keystrokes.
        if edit % 100 == 99 {
            doc.commit();
        }
    }
    doc.commit();

    let text = doc.text();
    let snapshot = doc.snapshot().expect("snapshot");
    let updates = doc.all_updates().expect("updates");

    println!(
        "10k edits: chars={} changes={} snapshot={} bytes updates={} bytes",
        text.chars().count(),
        doc.as_loro().len_changes(),
        snapshot.len(),
        updates.len()
    );

    assert_eq!(
        text.chars().count(),
        (0..10_000)
            .map(|edit| PIECES[edit % PIECES.len()].chars().count())
            .sum::<usize>()
    );
    // A D1 row is capped at 2 MB. Leave room for the note metadata and the
    // SQLite row header; a note this size must not approach the cap.
    assert!(
        snapshot.len() < 1_500_000,
        "snapshot grew to {} bytes",
        snapshot.len()
    );
    assert!(
        updates.len() < 1_500_000,
        "updates grew to {} bytes",
        updates.len()
    );
}
