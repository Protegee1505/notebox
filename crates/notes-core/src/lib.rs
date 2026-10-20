//! Note model, CRDT documents, history, search and Markdown rendering.
//!
//! UI-agnostic by design: the Dioxus app calls this crate through traits and
//! plain data types only.

#![forbid(unsafe_code)]
