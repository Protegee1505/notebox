//! Preferences, account, notebooks and local data.

use dioxus::prelude::*;

use crate::build_info;
use crate::media::format_size;
use crate::state::{AppContext, Layout, Screen, Theme};
use crate::time::relative_time;
use crate::ui::icons::Icon;

/// One settings pane. The desktop rail and the phone index share this list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    Account,
    Appearance,
    Notebooks,
    LocalData,
    CloudStorage,
    About,
}

impl SettingsSection {
    const ALL: [Self; 6] = [
        Self::Account,
        Self::Appearance,
        Self::Notebooks,
        Self::LocalData,
        Self::CloudStorage,
        Self::About,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::Appearance => "Appearance",
            Self::Notebooks => "Notebooks",
            Self::LocalData => "Local data",
            Self::CloudStorage => "Cloud storage",
            Self::About => "About",
        }
    }

    const fn subtitle(self) -> &'static str {
        match self {
            Self::Account => "Sync, devices and sign-in.",
            Self::Appearance => "Theme, list layout and delete confirmation.",
            Self::Notebooks => "Group notes without moving them into folders.",
            Self::LocalData => "What this device stores, and how to back it up.",
            Self::CloudStorage => "Usage on your Worker's free plan.",
            Self::About => "Version and keyboard help.",
        }
    }

    const fn attr(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Appearance => "appearance",
            Self::Notebooks => "notebooks",
            Self::LocalData => "local-data",
            Self::CloudStorage => "cloud-storage",
            Self::About => "about",
        }
    }

    const fn icon(self) -> &'static str {
        match self {
            Self::Account => "user",
            Self::Appearance => "palette",
            Self::Notebooks => "book",
            Self::LocalData => "database",
            Self::CloudStorage => "cloud",
            Self::About => "info",
        }
    }
}

#[component]
pub fn SettingsPane() -> Element {
    let context = use_context::<AppContext>();
    use_drop(move || {
        let _ = document::eval("document.getElementById('app-root')?.focus();");
    });
    let mut new_notebook = use_signal(String::new);
    let mut section = use_signal(|| SettingsSection::Account);
    let mut mobile_index = use_signal(|| true);
    let state = context.state.read();
    let theme = state.theme;
    let layout = state.layout;
    let notebooks = state.notebooks.clone();
    let note_count = state.note_count;
    let tag_count = state.tags.len();
    let storage = state.storage.clone();
    let attachment_usage = state.attachment_usage;
    let confirm_empty = state.confirm_empty_trash;
    let confirm_delete = state.confirm_delete;
    let pending_restore = state.pending_restore.clone();
    let username = state.profile.username.clone();
    let sync_connected = state.profile.sync_connected;
    let sync = state.sync.clone();
    let credentials = state.credentials.clone();
    drop(state);
    let now = crate::time::now();
    let active = section();
    let show_index = mobile_index();

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
    let cloud_usage = sync.usage;
    let usage_loading = sync.usage_loading;
    let usage_error = sync.usage_error.clone();
    let usage_checked = sync
        .usage_checked_at
        .map(|at| format!("Checked {}", relative_time(now, at)))
        .unwrap_or_else(|| "Not checked yet".to_string());

    rsx! {
        main {
            class: if show_index { "settings-pane is-index" } else { "settings-pane is-detail" },
            aria_label: "Settings",
            onmounted: move |_| {
                let _ = document::eval(
                    "setTimeout(() => {\
                     const button = [...document.querySelectorAll('[data-settings-back]')]\
                         .find((item) => item.offsetParent !== null);\
                     button?.focus();\
                     }, 0);",
                );
            },

            aside { class: "settings-rail",
                button {
                    class: "settings-back",
                    r#type: "button",
                    aria_label: "Back to notes",
                    title: "Back to notes",
                    "data-settings-back": "true",
                    onclick: {
                        let context = context.clone();
                        move |_| context.go(Screen::Notes)
                    },
                    Icon { name: "arrow-left" }
                    span { "Back to notes" }
                }

                nav { class: "settings-nav", aria_label: "Settings",
                    for item in SettingsSection::ALL.iter().copied() {
                        button {
                            key: "{item.attr()}",
                            class: if active == item { "settings-nav-item is-active" } else { "settings-nav-item" },
                            r#type: "button",
                            aria_current: if active == item { "page" } else { "false" },
                            "data-settings-section": item.attr(),
                            onclick: move |_| {
                                section.set(item);
                                mobile_index.set(false);
                            },
                            Icon { name: item.icon() }
                            span { "{item.label()}" }
                        }
                    }
                }

                div { class: "settings-rail-foot",
                    span { class: "settings-rail-dot" }
                    span { "Local-first" }
                    span { class: "settings-rail-version", "v{build_info::VERSION}" }
                }
            }

            section { class: "settings-index", aria_label: "Settings",
                header { class: "settings-index-head",
                    button {
                        class: "settings-index-back",
                        r#type: "button",
                        aria_label: "Back to notes",
                        title: "Back to notes",
                        "data-settings-back": "true",
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

                div { class: "settings-index-profile",
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

                nav { class: "settings-index-nav", aria_label: "Settings",
                    for item in SettingsSection::ALL.iter().copied() {
                        button {
                            key: "{item.attr()}",
                            class: "settings-index-row",
                            r#type: "button",
                            "data-settings-section": item.attr(),
                            onclick: move |_| {
                                section.set(item);
                                mobile_index.set(false);
                                let _ = document::eval(
                                    "setTimeout(() => document.querySelector('[data-settings-detail-back]')?.focus(), 0);",
                                );
                            },
                            span { class: "settings-index-icon", Icon { name: item.icon() } }
                            span { class: "settings-index-copy",
                                strong { "{item.label()}" }
                            }
                            Icon { name: "chevron-right" }
                        }
                    }
                }
            }

            div { class: "settings-content",
                h1 { class: "visually-hidden", "Settings" }
                button {
                    class: "settings-detail-back",
                    r#type: "button",
                    aria_label: "Back to settings",
                    title: "Back to settings",
                    "data-settings-detail-back": "true",
                    onclick: move |_| {
                        mobile_index.set(true);
                        let attr = active.attr();
                        let _ = document::eval(&format!(
                            "setTimeout(() => document.querySelector('.settings-index-row[data-settings-section=\"{attr}\"]')?.focus(), 0);"
                        ));
                    },
                    Icon { name: "arrow-left" }
                    span { "Settings" }
                }

                section {
                    id: "settings-account",
                    class: if active == SettingsSection::Account { "settings-section is-active" } else { "settings-section" },
                    aria_labelledby: "settings-account-title",
                    header { class: "settings-section-head",
                        div { class: "settings-section-title",
                            span { class: "settings-section-icon", Icon { name: SettingsSection::Account.icon() } }
                            div {
                                h2 { id: "settings-account-title", "Account" }
                                p { "{SettingsSection::Account.subtitle()}" }
                            }
                        }
                        span { class: "sync-state {status.tone()}",
                            span { class: "status-dot" }
                            "{status.label()}"
                        }
                    }

                    div { class: "settings-account",
                        div { class: "settings-account-identity",
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
                        if sync_connected {
                            button {
                                class: "primary-button",
                                r#type: "button",
                                disabled: status.busy() || credentials.busy,
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.sync_now()
                                },
                                Icon { name: "refresh" }
                                span { if status.busy() { "Syncing…" } else { "Sync now" } }
                            }
                        } else {
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

                    if let Some(error) = sync.error.clone() {
                        div { class: "form-error", role: "status",
                            span { "{error}" }
                        }
                    }

                    if sync_connected {
                        p { class: "card-note",
                            "Your Worker keeps a copy and passes updates between your devices. Notes stay readable when it cannot be reached."
                        }
                        div { class: "settings-subhead",
                            h3 { "Security" }
                        }
                        if credentials.open {
                            form {
                                class: "credentials-form",
                                aria_busy: credentials.busy,
                                onsubmit: {
                                    let context = context.clone();
                                    move |event| {
                                        event.prevent_default();
                                        if !credentials.busy {
                                            context.submit_credentials();
                                        }
                                    }
                                },
                                div { class: "credentials-heading",
                                    strong { "Change username or password" }
                                    span { "Your current device stays signed in. Other devices are signed out." }
                                }
                                if let Some(message) = credentials.error.clone() {
                                    div { class: "form-error", role: "alert", span { "{message}" } }
                                }
                                div { class: "credentials-grid",
                                    label { class: "field",
                                        span { "Current password" }
                                        input {
                                            class: "input",
                                            r#type: "password",
                                            autocomplete: "current-password",
                                            value: "{credentials.current_password}",
                                            disabled: credentials.busy,
                                            oninput: {
                                                let context = context.clone();
                                                move |event| context.set_credentials_current_password(event.value())
                                            },
                                        }
                                    }
                                    label { class: "field",
                                        span { "Username" }
                                        input {
                                            class: "input",
                                            r#type: "text",
                                            autocomplete: "username",
                                            value: "{credentials.username}",
                                            disabled: credentials.busy,
                                            oninput: {
                                                let context = context.clone();
                                                move |event| context.set_credentials_username(event.value())
                                            },
                                        }
                                    }
                                    label { class: "field",
                                        span { "New password (optional)" }
                                        input {
                                            class: "input",
                                            r#type: "password",
                                            autocomplete: "new-password",
                                            placeholder: "Leave blank to keep it",
                                            value: "{credentials.new_password}",
                                            disabled: credentials.busy,
                                            oninput: {
                                                let context = context.clone();
                                                move |event| context.set_credentials_new_password(event.value())
                                            },
                                        }
                                    }
                                    label { class: "field",
                                        span { "Confirm new password" }
                                        input {
                                            class: "input",
                                            r#type: "password",
                                            autocomplete: "new-password",
                                            value: "{credentials.confirm}",
                                            disabled: credentials.busy,
                                            oninput: {
                                                let context = context.clone();
                                                move |event| context.set_credentials_confirm(event.value())
                                            },
                                        }
                                    }
                                }
                                p { class: "card-note",
                                    "Changing either detail signs out every other device. Use at least 12 characters for a new password."
                                }
                                div { class: "button-row",
                                    button {
                                        class: "primary-button",
                                        r#type: "submit",
                                        disabled: credentials.busy,
                                        if credentials.busy { "Updating…" } else { "Update username or password" }
                                    }
                                    button {
                                        class: "text-button",
                                        r#type: "button",
                                        disabled: credentials.busy,
                                        onclick: {
                                            let context = context.clone();
                                            move |_| context.close_credentials()
                                        },
                                        "Cancel"
                                    }
                                }
                            }
                        } else {
                            div { class: "settings-action-list",
                                button {
                                    class: "settings-action-row",
                                    r#type: "button",
                                    disabled: status.busy(),
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.open_credentials()
                                    },
                                    span { class: "settings-action-icon", Icon { name: "key" } }
                                    span { class: "settings-action-copy",
                                        strong { "Change username or password" }
                                        span { "Signs out other devices" }
                                    }
                                    Icon { name: "chevron-right" }
                                }
                                button {
                                    class: "settings-action-row is-danger",
                                    r#type: "button",
                                    disabled: status.busy() || credentials.busy,
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.sign_out()
                                    },
                                    span { class: "settings-action-icon", Icon { name: "logout" } }
                                    span { class: "settings-action-copy",
                                        strong { "Sign out" }
                                        span { "Keep local notes on this device" }
                                    }
                                    Icon { name: "chevron-right" }
                                }
                            }
                        }

                        div { class: "settings-subhead",
                            h3 { "Devices" }
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
                                        div { class: "settings-device-main",
                                            span { class: "settings-device-icon", Icon { name: "monitor" } }
                                            div { class: "device-copy",
                                                strong { "{device.name}" }
                                                span { "Last active {relative_time(now, device.last_seen)}" }
                                            }
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

                section {
                    id: "settings-appearance",
                    class: if active == SettingsSection::Appearance { "settings-section is-active" } else { "settings-section" },
                    aria_labelledby: "settings-appearance-title",
                    header { class: "settings-section-head",
                        div { class: "settings-section-title",
                            span { class: "settings-section-icon", Icon { name: SettingsSection::Appearance.icon() } }
                            div {
                                h2 { id: "settings-appearance-title", "Appearance" }
                                p { "{SettingsSection::Appearance.subtitle()}" }
                            }
                        }
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
                    div { class: "setting-row",
                        span { class: "setting-label", "Confirm before deleting" }
                        div { class: "segmented", role: "group", aria_label: "Confirm before moving notes to trash",
                            for option in [true, false] {
                                button {
                                    key: "{option}",
                                    class: if confirm_delete == option { "segmented-option is-active" } else { "segmented-option" },
                                    r#type: "button",
                                    aria_pressed: if confirm_delete == option { "true" } else { "false" },
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.set_confirm_delete(option)
                                    },
                                    span { if option { "On" } else { "Off" } }
                                }
                            }
                        }
                    }
                    p { class: "card-note", "Changes apply immediately and stay on this device." }
                }

                section {
                    id: "settings-notebooks",
                    class: if active == SettingsSection::Notebooks { "settings-section is-active" } else { "settings-section" },
                    aria_labelledby: "settings-notebooks-title",
                    header { class: "settings-section-head",
                        div { class: "settings-section-title",
                            span { class: "settings-section-icon", Icon { name: SettingsSection::Notebooks.icon() } }
                            div {
                                h2 { id: "settings-notebooks-title", "Notebooks" }
                                p { "{SettingsSection::Notebooks.subtitle()}" }
                            }
                        }
                        span { class: "muted-label",
                            "{plural_count(notebooks.len() as u64, \"notebook\", \"notebooks\")}"
                        }
                    }
                    for notebook in notebooks.iter() {
                        div { key: "{notebook.id}", class: "notebook-row",
                            span { class: "settings-notebook-icon", Icon { name: "book" } }
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
                        p { class: "card-note", "No notebooks yet. Create one below." }
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
                    p { class: "card-note", "Deleting a notebook never deletes its notes." }
                }

                section {
                    id: "settings-local-data",
                    class: if active == SettingsSection::LocalData { "settings-section is-active storage-card" } else { "settings-section storage-card" },
                    aria_labelledby: "settings-local-data-title",
                    header { class: "settings-section-head",
                        div { class: "settings-section-title",
                            span { class: "settings-section-icon", Icon { name: SettingsSection::LocalData.icon() } }
                            div {
                                h2 { id: "settings-local-data-title", "Local data" }
                                p { "{SettingsSection::LocalData.subtitle()}" }
                            }
                        }
                    }
                    dl { class: "facts",
                        div { dt { "Notes" } dd { "{note_count}" } }
                        div { dt { "Notebooks" } dd { "{notebooks.len()}" } }
                        div { dt { "Tags" } dd { "{tag_count}" } }
                        div { dt { "Images & files" } dd { "{attachment_usage.count}" } }
                        div { dt { "Attachment size" } dd { "{format_size(attachment_usage.bytes)}" } }
                    }
                    div { class: "settings-value-row",
                        span { class: "settings-value-icon", Icon { name: "database" } }
                        div { class: "settings-value-copy",
                            span { class: "setting-label", "Storage" }
                            span { class: "card-note", "{storage.detail}" }
                        }
                        span { class: "settings-value", "{storage.label}" }
                    }
                    if storage.temporary {
                        p { class: "settings-warning", role: "status",
                            "Notes reset when this page reloads. Export a backup to keep them."
                        }
                    }

                    div { class: "settings-subhead", h3 { "Backup" } }
                    div { class: "settings-inline-actions",
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
                                    Icon { name: "upload" }
                                    span { "Restore backup" }
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

                    div { class: "settings-subhead", h3 { "Trash" } }
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
                                    Icon { name: "trash" }
                                    span { "Empty trash" }
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
                            class: "text-button danger-text",
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

                section {
                    id: "settings-cloud-storage",
                    class: if active == SettingsSection::CloudStorage { "settings-section is-active" } else { "settings-section" },
                    aria_labelledby: "settings-cloud-storage-title",
                    header { class: "settings-section-head",
                        div { class: "settings-section-title",
                            span { class: "settings-section-icon", Icon { name: SettingsSection::CloudStorage.icon() } }
                            div {
                                h2 { id: "settings-cloud-storage-title", "Cloud storage" }
                                p { "{SettingsSection::CloudStorage.subtitle()}" }
                            }
                        }
                        span { class: "muted-label",
                            if sync_connected { "500 MB per database" } else { "Local only" }
                        }
                    }
                    if !sync_connected {
                        p { class: "card-note",
                            "Sign in to see how much of your Worker's free storage the account uses. Images, notes and history all stay readable on this device when you are offline."
                        }
                    } else if let Some(usage) = cloud_usage {
                        UsageMeter {
                            title: "Images & files",
                            bytes: usage.attachments.bytes,
                            count_label: plural_count(usage.attachments.count, "file", "files"),
                            limit: usage.limit_bytes,
                            approximate: false,
                        }
                        UsageMeter {
                            title: "Notes & history",
                            bytes: usage.notes.bytes,
                            count_label: plural_count(usage.notes.count, "update", "updates"),
                            limit: usage.limit_bytes,
                            approximate: true,
                        }
                        p { class: "card-note",
                            "Each database has its own 500 MB on the free plan. The notes figure is approximate because the update log stores base64-encoded history."
                        }
                    } else if usage_loading {
                        div { class: "cloud-checking", role: "status",
                            span { class: "save-dot is-dirty" }
                            "Checking cloud storage..."
                        }
                    } else {
                        p { class: "card-note",
                            "Cloud storage has not been checked yet."
                        }
                    }
                    if let Some(error) = usage_error {
                        div { class: "form-error", role: "status",
                            span { "{error}" }
                        }
                    }
                    if sync_connected {
                        div { class: "settings-inline-actions cloud-actions",
                            button {
                                class: "text-button",
                                r#type: "button",
                                disabled: usage_loading,
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.refresh_cloud_usage()
                                },
                                Icon { name: "refresh" }
                                span { if usage_loading { "Checking..." } else { "Refresh usage" } }
                            }
                            span { class: "muted-label", "{usage_checked}" }
                        }
                    }
                }

                section {
                    id: "settings-about",
                    class: if active == SettingsSection::About { "settings-section is-active" } else { "settings-section" },
                    aria_labelledby: "settings-about-title",
                    header { class: "settings-section-head",
                        div { class: "settings-section-title",
                            span { class: "settings-section-icon", Icon { name: SettingsSection::About.icon() } }
                            div {
                                h2 { id: "settings-about-title", "About" }
                                p { "{SettingsSection::About.subtitle()}" }
                            }
                        }
                    }
                    div { class: "settings-about-brand",
                        span { class: "brand-mark", "nb" }
                        div { class: "about-copy",
                            strong { "Notebox" }
                            span { "Local-first notes, made in Rust." }
                        }
                    }
                    dl { class: "settings-kv-list",
                        div { dt { "Version" } dd { "{build_info::label()}" } }
                        div { dt { "Phase" } dd { "Phase 8 · polish" } }
                        div { dt { "Help" } dd {
                            button {
                                class: "text-button",
                                r#type: "button",
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.show_shortcuts()
                                },
                                Icon { name: "command" }
                                span { "Keyboard shortcuts" }
                            }
                        } }
                    }
                    p { class: "card-note",
                        "Local-first notes in Rust. One codebase for web, desktop and Android; Loro history; SQLite storage; a Cloudflare Worker for sync."
                    }
                }
            }
        }
    }
}

#[component]
fn UsageMeter(
    title: &'static str,
    bytes: u64,
    count_label: String,
    limit: u64,
    #[props(default = false)] approximate: bool,
) -> Element {
    let remaining = limit.saturating_sub(bytes);
    rsx! {
        div { class: "usage-meter",
            div { class: "usage-meter-head",
                strong { "{title}" }
                span { "{format_size(bytes)} of {format_size(limit)}" }
            }
            progress {
                class: "usage-progress",
                value: "{bytes}",
                max: "{limit}",
                aria_label: "{title} storage used",
            }
            div { class: "usage-meter-foot",
                span { "{format_size(remaining)} remaining" }
                span {
                    "{count_label}"
                    if approximate {
                        " · approximate"
                    }
                }
            }
        }
    }
}

fn plural_count(count: u64, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}
