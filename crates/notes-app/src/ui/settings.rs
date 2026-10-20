//! Preferences, account, notebooks and local data.

use dioxus::prelude::*;

use crate::state::{AppContext, Layout, Screen, Theme};
use crate::ui::icons::Icon;

#[component]
pub fn SettingsPane() -> Element {
    let context = use_context::<AppContext>();
    let mut new_notebook = use_signal(String::new);
    let state = context.state.read();
    let theme = state.theme;
    let layout = state.layout;
    let notebooks = state.notebooks.clone();
    let note_count = state.note_count;
    let tag_count = state.tags.len();
    let storage = state.storage.clone();
    let confirm_empty = state.confirm_empty_trash;
    let username = state.profile.username.clone();
    let sync_connected = state.profile.sync_connected;
    drop(state);

    let initial = username
        .chars()
        .next()
        .unwrap_or('L')
        .to_uppercase()
        .to_string();

    rsx! {
        main { class: "settings-pane", aria_label: "Settings",
            header { class: "settings-header",
                button {
                    class: "icon-button back-button",
                    r#type: "button",
                    aria_label: "Back to notes",
                    title: "Back to notes",
                    onclick: {
                        let context = context.clone();
                        move |_| context.go(Screen::Notes)
                    },
                    Icon { name: "arrow-left" }
                }
                div {
                    h1 { "Settings" }
                    p { "Preferences stay on this device." }
                }
            }

            div { class: "settings-grid",
                section { class: "settings-card account-card",
                    div { class: "card-heading",
                        h2 { "Account" }
                        span { class: "muted-label", if sync_connected { "Synced" } else { "Local only" } }
                    }
                    div { class: "account-row",
                        div { class: "avatar large", "{initial}" }
                        div { class: "account-copy",
                            strong { "{username}" }
                            span { if sync_connected { "Connected to your Worker" } else { "No account connected" } }
                        }
                    }
                    p { class: "card-note",
                        "Sync through your own free Cloudflare Worker is wired in the next phase. Until then every note stays in the local database."
                    }
                    button {
                        class: "primary-button",
                        r#type: "button",
                        onclick: {
                            let context = context.clone();
                            move |_| context.go(Screen::Login)
                        },
                        Icon { name: "user" }
                        span { "Sign in to sync" }
                    }
                }

                section { class: "settings-card",
                    div { class: "card-heading",
                        h2 { "Appearance" }
                    }
                    div { class: "setting-row",
                        span { class: "setting-label", "Theme" }
                        div { class: "segmented", role: "group", aria_label: "Theme",
                            for option in [Theme::System, Theme::Light, Theme::Dark] {
                                button {
                                    key: "{option.attr()}",
                                    class: if theme == option { "segmented-option is-active" } else { "segmented-option" },
                                    r#type: "button",
                                    aria_pressed: if theme == option { "true" } else { "false" },
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.set_theme(option)
                                    },
                                    if option == Theme::System { Icon { name: "monitor" } }
                                    if option == Theme::Light { Icon { name: "sun" } }
                                    if option == Theme::Dark { Icon { name: "moon" } }
                                    span { "{option.label()}" }
                                }
                            }
                        }
                    }
                    div { class: "setting-row",
                        span { class: "setting-label", "Note list" }
                        div { class: "segmented", role: "group", aria_label: "Note list layout",
                            for option in [Layout::Grid, Layout::List] {
                                button {
                                    key: "{option.label()}",
                                    class: if layout == option { "segmented-option is-active" } else { "segmented-option" },
                                    r#type: "button",
                                    aria_pressed: if layout == option { "true" } else { "false" },
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.set_layout(option)
                                    },
                                    if option == Layout::Grid { Icon { name: "grid" } }
                                    if option == Layout::List { Icon { name: "list" } }
                                    span { "{option.label()}" }
                                }
                            }
                        }
                    }
                }

                section { class: "settings-card notebooks-card",
                    div { class: "card-heading",
                        h2 { "Notebooks" }
                        span { class: "muted-label", "{notebooks.len()}" }
                    }
                    for notebook in notebooks.iter() {
                        div { key: "{notebook.id}", class: "notebook-row",
                            span { class: "notebook-dot" }
                            input {
                                class: "rename-input",
                                r#type: "text",
                                aria_label: "Notebook name",
                                value: "{notebook.name}",
                                onchange: {
                                    let context = context.clone();
                                    let id = notebook.id;
                                    move |event| context.rename_notebook(id, event.value())
                                },
                            }
                            button {
                                class: "icon-button small danger-hover",
                                r#type: "button",
                                aria_label: "Delete notebook {notebook.name}",
                                title: "Delete notebook. Its notes are kept.",
                                onclick: {
                                    let context = context.clone();
                                    let id = notebook.id;
                                    move |_| context.delete_notebook(id)
                                },
                                Icon { name: "trash" }
                            }
                        }
                    }
                    if notebooks.is_empty() {
                        p { class: "card-note", "Create a notebook to group notes. Deleting one never deletes its notes." }
                    }
                    form {
                        class: "notebook-add",
                        onsubmit: {
                            let context = context.clone();
                            move |event| {
                                event.prevent_default();
                                let name = new_notebook.read().clone();
                                if !name.trim().is_empty() {
                                    context.create_notebook(name);
                                    new_notebook.set(String::new());
                                }
                            }
                        },
                        input {
                            class: "inline-input",
                            r#type: "text",
                            placeholder: "New notebook",
                            aria_label: "New notebook name",
                            value: "{new_notebook}",
                            oninput: move |event| new_notebook.set(event.value()),
                        }
                        button { class: "secondary-button", r#type: "submit",
                            Icon { name: "plus" }
                            span { "Add" }
                        }
                    }
                }

                section { class: "settings-card storage-card",
                    div { class: "card-heading",
                        h2 { "Local data" }
                        span { class: "muted-label", "{storage.label}" }
                    }
                    dl { class: "facts",
                        div { dt { "Notes" } dd { "{note_count}" } }
                        div { dt { "Notebooks" } dd { "{notebooks.len()}" } }
                        div { dt { "Tags" } dd { "{tag_count}" } }
                    }
                    p { class: if storage.temporary { "storage-path is-temporary" } else { "storage-path" },
                        "{storage.detail}"
                    }
                    div { class: "button-row",
                        button {
                            class: "secondary-button",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.export_notes()
                            },
                            Icon { name: "download" }
                            span { "Export everything" }
                        }
                        button {
                            class: "secondary-button",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.show_shortcuts()
                            },
                            Icon { name: "command" }
                            span { "Shortcuts" }
                        }
                    }
                }

                section { class: "settings-card danger-card",
                    div { class: "card-heading",
                        h2 { "Trash" }
                    }
                    p { class: "card-note",
                        "Deleted notes stay in the trash until you empty it. Nothing is erased automatically."
                    }
                    if confirm_empty {
                        div { class: "confirm-row",
                            p { "Erase every trashed note? This cannot be undone." }
                            div { class: "button-row",
                                button {
                                    class: "danger-button",
                                    r#type: "button",
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.empty_trash()
                                    },
                                    "Empty trash"
                                }
                                button {
                                    class: "text-button",
                                    r#type: "button",
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.state.write_unchecked().confirm_empty_trash = false
                                    },
                                    "Cancel"
                                }
                            }
                        }
                    } else {
                        button {
                            class: "secondary-button",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.state.write_unchecked().confirm_empty_trash = true
                            },
                            Icon { name: "trash" }
                            span { "Empty trash" }
                        }
                    }
                }

                section { class: "settings-card about-card",
                    div { class: "card-heading",
                        h2 { "About" }
                    }
                    p { class: "card-note",
                        "Notebox 0.1.0 \u{00b7} local-first notes in Rust. One codebase for web, desktop and Android; Loro history; SQLite storage; a Cloudflare Worker for sync."
                    }
                    div { class: "phase-pill", "Phase 4 \u{00b7} app shell" }
                }
            }
        }
    }
}
