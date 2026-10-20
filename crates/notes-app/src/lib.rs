//! Dioxus UI for web, desktop and Android.
//!
//! The app owns presentation only. Every note operation goes through the
//! [`NoteStore`](notes_store::NoteStore) trait, Markdown is rendered by
//! `notes-core`, and the next phase can put the sync engine on the same store
//! without a UI rewrite.
//!
//! Layout follows PLAN.md: one pane under 700 px, two panes from 700 px and
//! three from 1100 px. The browser build keeps its database in memory for now;
//! P7 moves it into an OPFS worker.

#![forbid(unsafe_code)]

mod app;
mod commands;
mod images;
mod markdown_ops;
mod media;
mod state;
mod storage;
mod time;
mod ui;

pub use app::App;
