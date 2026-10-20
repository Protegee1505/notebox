//! Dioxus UI for web, desktop and Android.
//!
//! The app owns presentation only. Every note operation goes through the
//! [`NoteStore`](notes_store::NoteStore) trait, Markdown is rendered by
//! `notes-core`, and sync runs through `notes-sync` without the UI knowing
//! which transport a target uses.
//!
//! Layout follows PLAN.md: one pane under 700 px, two panes from 700 px and
//! three from 1100 px. The browser build still keeps its database in memory;
//! sync restores it from the Worker after a reload, and the OPFS worker is the
//! next persistence step.

#![forbid(unsafe_code)]

mod app;
mod commands;
mod images;
mod markdown_ops;
mod media;
mod state;
mod storage;
mod sync;
mod time;
mod ui;

pub use app::App;
