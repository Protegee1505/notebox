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
    let initial = state
        .profile
        .username
        .chars()
        .next()
        .unwrap_or('L')
        .to_uppercase()
        .to_string();
    drop(state);
    let sync_title = sync_error.unwrap_or_else(|| match (sync, last_synced) {
        (SyncStatus::Local, _) => "Local only. Open Settings to sync.".to_string(),
        (_, Some(at)) => format!("Last synced {}", relative_time(crate::time::now(), at)),
        _ => "Account and sync".to_string(),
    });

    rsx! {
        header { class: "topbar",
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
                button {
                    class: "status-pill {sync.tone()}",
                    r#type: "button",
                    title: "{sync_title}",
                    onclick: {
                        let context = context.clone();
                        move |_| context.open_settings()
                    },
                    span { class: "status-dot" }
                    span { "{sync.short()}" }
                }
                button {
                    class: "icon-button",
                    r#type: "button",
                    aria_label: "Toggle theme",
                    title: match theme {
                        Theme::Light => "Switch to dark",
                        _ => "Switch to light",
                    },
                    onclick: {
                        let context = context.clone();
                        move |_| context.toggle_theme()
                    },
                    Icon { name: match theme { Theme::Light => "moon", _ => "sun" } }
                }
                button {
                    class: "avatar",
                    r#type: "button",
                    aria_label: "Open settings",
                    title: "Settings",
                    onclick: {
                        let context = context.clone();
                        move |_| context.open_settings()
                    },
                    "{initial}"
                }
            }
        }
    }
}
