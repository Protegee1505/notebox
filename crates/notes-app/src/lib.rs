//! Dioxus UI for web, desktop and Android.
//!
//! The app owns presentation only. Every note operation goes through the
//! [`NoteStore`](notes_store::NoteStore) trait, Markdown is rendered by
//! `notes-core`, and sync runs through `notes-sync` without the UI knowing
//! which transport a target uses.
//!
//! Layout follows PLAN.md: one pane under 700 px, two panes from 700 px and
//! three from 1100 px. The browser build keeps SQLite in memory and saves a
//! database image to OPFS after changes, so local-only notes, history and
//! attachment bytes survive a reload.

#![forbid(unsafe_code)]

mod app;
#[cfg(target_arch = "wasm32")]
mod browser_storage;
mod build_info;
mod commands;
mod images;
mod markdown_ops;
mod media;
mod perf;
#[cfg(target_arch = "wasm32")]
mod pwa;
mod state;
mod storage;
mod sync;
mod time;
mod ui;

pub use app::App;
