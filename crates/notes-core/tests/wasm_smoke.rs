//! S2 on wasm32: create, edit, export, import and time travel in Node.
//!
//! Run with:
//! `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
//!  cargo test -p notes-core --target wasm32-unknown-unknown`

#![cfg(target_arch = "wasm32")]

use notes_core::NoteDocument;
use wasm_bindgen_test::wasm_bindgen_test;

// wasm-bindgen-test runs in Node by default.

#[wasm_bindgen_test]
fn create_edit_export_import_and_time_travel() {
    let doc = NoteDocument::new();
    doc.insert(0, "wasm 😀").expect("insert");
    doc.commit();
    let first = doc.frontiers();

    doc.insert(6, " works").expect("insert");
    doc.commit();
    assert_eq!(doc.text(), "wasm 😀 works");

    doc.checkout(&first).expect("checkout");
    assert_eq!(doc.text(), "wasm 😀");
    doc.checkout_latest();
    assert_eq!(doc.text(), "wasm 😀 works");

    let snapshot = doc.snapshot().expect("snapshot");
    let restored = NoteDocument::from_snapshot(&snapshot).expect("restore");
    assert_eq!(restored.text(), doc.text());
    assert_eq!(restored.version(), doc.version());
}
