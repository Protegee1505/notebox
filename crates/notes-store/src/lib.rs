//! Backend-agnostic local storage.
//!
//! Native builds use SQLite through `rusqlite`; browser builds use
//! `sqlite-wasm-rs`. Both implement [`NoteStore`].

#![forbid(unsafe_code)]

/// Local note storage contract.
///
/// Methods are added with the Phase 1 note model so the UI can be written
/// against this trait while the native and browser backends land separately.
pub trait NoteStore {}
