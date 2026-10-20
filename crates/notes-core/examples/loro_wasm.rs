//! Size probe: a minimal binary that pulls in the note document and Loro.
//!
//! Build for wasm and compress:
//! `cargo build -p notes-core --release --target wasm32-unknown-unknown \
//!  --example loro_wasm`

fn main() {
    let doc = notes_core::NoteDocument::new();
    doc.insert(0, "Notebox").expect("insert");
    doc.commit();
    let snapshot = doc.snapshot().expect("snapshot");
    std::hint::black_box(&snapshot);
}
