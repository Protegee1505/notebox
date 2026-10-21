//! Preferences, account, notebooks and local data.

use dioxus::prelude::*;

use crate::build_info;
use crate::state::{AppContext, Layout, Screen, Theme};
use crate::time::relative_time;
use crate::ui::icons::Icon;

#[component]
pub fn SettingsPane() -> Element {
    let context = use_context::<AppContext>();
    use_drop(move || {
        let _ = document::eval("document.getElementById('app-root')?.focus();");
    });
    let mut new_notebook = use_signal(String::new);
    let state = context.state.read();
    let theme = state.theme;
    let layout = state.layout;
    let notebooks = state.notebooks.clone();
    let note_count = state.note_count;
    let tag_count = state.tags.len();
    let storage = state.storage.clone();
    let confirm_empty = state.confirm_empty_trash;
    let pending_restore = state.pending_restore.clone();
    let username = state.profile.username.clone();
    let sync_connected = state.profile.sync_connected;
    let sync = state.sync.clone();
    drop(state);
    let now = crate::time::now();

    let initial = username
        .chars()
        .next()
        .unwrap_or('L')
        .to_uppercase()
        .to_string();
    let status = sync.status;
    let last_synced = sync
        .last_synced
        .map(|at| format!("Last synced {}", relative_time(now, at)))
        .unwrap_or_else(|| "Not synced yet".to_string());

    rsx! {
        main { class: "settings-pane", aria_label: "Settings",
            header { class: "settings-header",
                button {
                    id: "settings-back",
                    class: "icon-button back-button",
                    r#type: "button",
                    aria_label: "Back to notes",
                    title: "Back to notes",
                    autofocus: true,
                    onmounted: move |_| {
                        let _ = document::eval(
                            "document.getElementById('settings-back')?.focus();",
                        );
                    },
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
                        span { class: "sync-state {status.tone()}",
                            span { class: "status-dot" }
                            "{status.label()}"
                        }
                    }
                    div { class: "account-row",
                        div { class: "avatar large", "{initial}" }
                        div { class: "account-copy",
                            strong { if sync_connected { "{username}" } else { "Local only" } }
                            span {
                                if sync_connected {
                                    "{last_synced}"
                                } else {
                                    "No account connected"
                                }
                            }
                        }
                    }
                    if let Some(error) = sync.error.clone() {
                        div { class: "form-error", role: "status",
                            span { "{error}" }
                        }
                    }
                    if sync_connected {
                        p { class: "card-note",
                            "Your Worker keeps a copy and passes updates between your devices. Notes stay readable when it cannot be reached."
                        }
                        div { class: "button-row",
                            button {
                                class: "primary-button",
                                r#type: "button",
                                disabled: status.busy(),
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.sync_now()
                                },
                                Icon { name: "refresh" }
                                span { if status.busy() { "Syncing…" } else { "Sync now" } }
                            }
                            button {
                                class: "secondary-button",
                                r#type: "button",
                                disabled: status.busy(),
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.sign_out()
                                },
                                Icon { name: "logout" }
                                span { "Sign out" }
                            }
                        }
                    } else {
                        p { class: "card-note",
                            "Sign in with the account you set up on your Worker. Every note is copied to the server and merged with the other devices. You can keep working offline at any time."
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
                }

                if sync_connected {
                    section { class: "settings-card devices-card",
                        div { class: "card-heading",
                            h2 { "Devices" }
                            button {
                                class: "icon-button small",
                                r#type: "button",
                                aria_label: "Refresh devices",
                                title: "Refresh devices",
                                disabled: sync.devices_loading,
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.refresh_devices()
                                },
                                Icon { name: "refresh" }
                            }
                        }
                        if let Some(error) = sync.devices_error.clone() {
                            div { class: "form-error", role: "status", span { "{error}" } }
                        }
                        if sync.devices.is_empty() {
                            p { class: "card-note",
                                if sync.devices_loading {
                                    "Loading devices…"
                                } else {
                                    "Refresh to see the devices that can reach your account."
                                }
                            }
                        } else {
                            div { class: "device-list",
                                for device in sync.devices.iter() {
                                    div { key: "{device.id}", class: "device-row",
                                        div { class: "device-copy",
                                            strong { "{device.name}" }
                                            span { "Last active {relative_time(now, device.last_seen)}" }
                                        }
                                        if device.current {
                                            span { class: "device-pill", "This device" }
                                        } else {
                                            button {
                                                class: "text-button danger-text",
                                                r#type: "button",
                                                disabled: sync.devices_loading,
                                                onclick: {
                                                    let context = context.clone();
                                                    let id = device.id.clone();
                                                    move |_| context.revoke_device(id.clone())
                                                },
                                                if sync.confirm_revoke.as_deref() == Some(device.id.as_str()) {
                                                    "Confirm revoke"
                                                } else {
                                                    "Revoke"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        p { class: "card-note",
                            "Revoking a device asks it to sign in again. Its local notes stay where they are."
                        }
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
                            aria_label: "Restore from backup",
                            onclick: move |_| {
                                let _ = document::eval(
                                    "document.getElementById('restore-backup-input')?.click();",
                                );
                            },
                            Icon { name: "upload" }
                            span { "Restore from backup" }
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
                    input {
                        id: "restore-backup-input",
                        class: "visually-hidden",
                        r#type: "file",
                        accept: ".zip,application/zip",
                        tabindex: "-1",
                        aria_label: "Notebox backup archive",
                        onchange: {
                            let context = context.clone();
                            move |event| {
                                let files = event.files();
                                context.choose_restore(files);
                                let _ = document::eval(
                                    "const el = document.getElementById('restore-backup-input');\
                                     if (el) el.value = '';",
                                );
                            }
                        },
                    }
                    if let Some(pending) = pending_restore {
                        div { class: "restore-confirm", role: "alert",
                            div { class: "restore-copy",
                                strong { "Restore {pending.name}?" }
                                span { "{pending.summary()}" }
                                span {
                                    if pending.has_history {
                                        "This archive includes version history."
                                    } else {
                                        "This archive has no version history; the current notes are restored."
                                    }
                                }
                                span {
                                    "Notes with the same id are updated. Their previous text stays in history, and nothing is deleted."
                                }
                            }
                            div { class: "restore-actions",
                                button {
                                    class: "primary-button",
                                    r#type: "button",
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.confirm_restore()
                                    },
                                    "Restore backup"
                                }
                                button {
                                    class: "text-button",
                                    r#type: "button",
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.cancel_restore()
                                    },
                                    "Cancel"
                                }
                            }
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
                        "{build_info::label()}. Local-first notes in Rust. One codebase for web, desktop and Android; Loro history; SQLite storage; a Cloudflare Worker for sync."
                    }
                    div { class: "phase-pill", "Phase 7 \u{00b7} packaging" }
                }
            }
        }
    }
}
