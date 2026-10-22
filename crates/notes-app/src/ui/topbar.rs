//! The persistent top bar: navigation, search and account controls.

use dioxus::prelude::*;
use notes_store::NoteView;

use crate::state::{AppContext, Screen, Theme};
use crate::sync::SyncStatus;
use crate::time::relative_time;
use crate::ui::icons::Icon;

#[component]
pub fn TopBar() -> Element {
    let context = use_context::<AppContext>();
    let state = context.state.read();
    let screen = state.screen;
    let theme = state.theme;
    let search = state.search.clone();
    let hits = state.search_hits;
    let sidebar_open = state.sidebar_open;
    let sidebar_collapsed = state.sidebar_collapsed;
    let sync = state.sync.status;
    let sync_error = state.sync.error.clone();
    let last_synced = state.sync.last_synced;
    let sync_connected = state.profile.sync_connected;
    let username = state.profile.username.clone();
    let account_open = state.account_open;
    let initial = username
        .chars()
        .next()
        .unwrap_or('L')
        .to_uppercase()
        .to_string();
    drop(state);
    let sync_title = sync_error
        .clone()
        .unwrap_or_else(|| match (sync, last_synced) {
            (SyncStatus::Local, _) => "Local only. Open Settings to sync.".to_string(),
            (_, Some(at)) => format!("Last synced {}", relative_time(crate::time::now(), at)),
            _ => "Account and sync".to_string(),
        });
    let account_detail = if !sync_connected {
        "Notes stay on this device".to_string()
    } else if let Some(error) = sync_error.as_deref() {
        error.to_string()
    } else {
        match (sync, last_synced) {
            (SyncStatus::Idle, Some(at)) => {
                format!("Last synced {}", relative_time(crate::time::now(), at))
            }
            (SyncStatus::Connecting, _) => "Connecting…".to_string(),
            (SyncStatus::Syncing, _) => "Syncing…".to_string(),
            _ => sync.label().to_string(),
        }
    };
    let account_label = if sync_connected {
        format!("Account and sync: {username}, {}", sync.short())
    } else {
        "Account and sync: Local only".to_string()
    };

    rsx! {
        header { class: if account_open { "topbar has-account" } else { "topbar" },
            div { class: "topbar-start",
                button {
                    class: "icon-button menu-button menu-button-mobile",
                    r#type: "button",
                    aria_label: "Open navigation",
                    aria_expanded: if sidebar_open { "true" } else { "false" },
                    title: "Navigation",
                    onclick: {
                        let context = context.clone();
                        move |_| context.state.write_unchecked().sidebar_open = true
                    },
                    Icon { name: "menu" }
                }
                button {
                    class: "icon-button menu-button menu-button-desktop",
                    r#type: "button",
                    aria_label: if sidebar_collapsed { "Show navigation" } else { "Hide navigation" },
                    aria_expanded: if sidebar_collapsed { "false" } else { "true" },
                    title: "Navigation",
                    onclick: {
                        let context = context.clone();
                        move |_| context.toggle_sidebar_collapsed()
                    },
                    Icon { name: "menu" }
                }
                button {
                    class: "brand",
                    r#type: "button",
                    title: "All notes",
                    onclick: {
                        let context = context.clone();
                        move |_| context.set_view(NoteView::Active)
                    },
                    span { class: "brand-mark", "nb" }
                    span { class: "brand-name", "Notebox" }
                }
                if screen == Screen::Settings {
                    span { class: "crumb", "Settings" }
                }
            }

            div { class: "search-field",
                Icon { name: "search" }
                input {
                    id: "global-search",
                    r#type: "search",
                    class: "search-input",
                    placeholder: "Search notes",
                    aria_label: "Search notes",
                    autocomplete: "off",
                    value: "{search}",
                    oninput: {
                        let context = context.clone();
                        move |event| context.set_search(event.value())
                    },
                }
                if let Some(count) = hits {
                    span { class: "search-count",
                        "{count} "
                        if count == 1 { "hit" } else { "hits" }
                    }
                }
                kbd { class: "key-hint", "Ctrl K" }
            }

            div { class: "topbar-end",
                span {
                    class: "status-pill {sync.tone()}",
                    title: "{sync_title}",
                    span { class: "status-dot" }
                    span { "{sync.short()}" }
                }
                button {
                    class: "icon-button",
                    r#type: "button",
                    aria_label: "Toggle theme",
                    title: match theme {
                        Theme::Light => "Switch to dark",
                        Theme::Dark => "Switch to light",
                        Theme::System => "Switch theme",
                    },
                    onclick: {
                        let context = context.clone();
                        move |_| context.toggle_theme()
                    },
                    Icon { name: match theme {
                        Theme::Light => "moon",
                        Theme::Dark => "sun",
                        Theme::System => "monitor",
                    } }
                }
                div { class: "account-anchor",
                    button {
                        id: "account-button",
                        class: if account_open { "avatar-button is-open" } else { "avatar-button" },
                        r#type: "button",
                        aria_label: "{account_label}",
                        aria_haspopup: "dialog",
                        aria_expanded: if account_open { "true" } else { "false" },
                        title: "Account and sync",
                        onclick: {
                            let context = context.clone();
                            move |event| {
                                event.stop_propagation();
                                context.toggle_account_menu();
                            }
                        },
                        span { class: "avatar", aria_hidden: "true",
                            "{initial}"
                            span { class: "avatar-dot {sync.tone()}" }
                        }
                    }
                    if account_open {
                        div {
                            id: "account-menu",
                            class: "account-menu",
                            role: "dialog",
                            aria_label: "Account and sync",
                            tabindex: "-1",
                            onclick: move |event| event.stop_propagation(),
                            onmounted: move |_| {
                                let _ = document::eval(
                                    "setTimeout(() => document.getElementById('account-menu')?.focus(), 0);",
                                );
                            },
                            div { class: "account-identity",
                                span { class: "avatar large", aria_hidden: "true", "{initial}" }
                                div { class: "account-copy",
                                    strong { if sync_connected { "{username}" } else { "Local only" } }
                                    span { class: "account-detail sync-state {sync.tone()}",
                                        span { class: "status-dot" }
                                        span { "{account_detail}" }
                                    }
                                }
                            }
                            div { class: "account-sep" }
                            if sync_connected {
                                button {
                                    class: "account-action",
                                    r#type: "button",
                                    disabled: sync.busy(),
                                    onclick: {
                                        let context = context.clone();
                                        move |_| {
                                            context.state.write_unchecked().account_open = false;
                                            context.sync_now();
                                            let _ = document::eval(
                                                "document.getElementById('account-button')?.focus();",
                                            );
                                        }
                                    },
                                    Icon { name: "refresh" }
                                    span { if sync.busy() { "Syncing…" } else { "Sync now" } }
                                }
                            } else {
                                button {
                                    class: "account-action",
                                    r#type: "button",
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.go(Screen::Login)
                                    },
                                    Icon { name: "user" }
                                    span { "Sign in to sync" }
                                }
                            }
                            button {
                                class: "account-action",
                                r#type: "button",
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.open_settings()
                                },
                                Icon { name: "settings" }
                                span { "Account settings" }
                            }
                            if sync_connected {
                                p { class: "account-note",
                                    "Devices and sign out live in Settings."
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
