//! Version history: the timeline, the diff, a read-only preview and restore.

use dioxus::prelude::*;
use notes_core::render_markdown;

use crate::state::{note_title, AppContext, HistoryMode};
use crate::time::{day_label, now, relative_time};
use crate::ui::icons::Icon;

#[component]
pub fn HistoryPanel() -> Element {
    let context = use_context::<AppContext>();
    use_drop(move || {
        let _ = document::eval("document.getElementById('app-root')?.focus();");
    });
    let state = context.state.read();
    let versions = state.history_versions.clone();
    let selected_id = state.history_selected.clone();
    let current_id = state.history_current.clone();
    let local_peer = state.history_local_peer;
    let mode = state.history_mode;
    let preview = state.history_preview.clone();
    let diff = state.history_diff.clone();
    let error = state.history_error.clone();
    let title = state
        .selected
        .and_then(|id| state.notes.iter().find(|note| note.id == id))
        .map(note_title)
        .unwrap_or_else(|| "Note".to_string());
    drop(state);

    let clock = now();
    let selected = selected_id
        .as_ref()
        .and_then(|id| versions.iter().find(|version| &version.id == id))
        .cloned();
    let selected_day = selected
        .as_ref()
        .map(|version| day_label(clock, version.started_at))
        .unwrap_or_default();
    let selected_relative = selected
        .as_ref()
        .map(|version| relative_time(clock, version.ended_at))
        .unwrap_or_default();
    let selected_changes = selected
        .as_ref()
        .map(|version| change_label(version.change_count))
        .unwrap_or_default();
    let selected_peer = selected
        .as_ref()
        .map(|version| peer_label(version.peer, local_peer))
        .unwrap_or_default();
    let selected_meta = [selected_relative, selected_changes, selected_peer]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" \u{00b7} ");
    let can_restore = selected.is_some() && !diff.is_empty();
    let restore_id = selected
        .as_ref()
        .map(|version| version.id.clone())
        .unwrap_or_default();
    let preview_html = render_markdown(&preview);
    // The unified-diff header carries internal peer ids; the panel already
    // names the version and the current note, so drop those two lines.
    let diff_lines: Vec<&str> = diff
        .lines()
        .filter(|line| !line.starts_with("--- ") && !line.starts_with("+++ "))
        .collect();
    let version_count = versions.len();

    rsx! {
        section {
            class: "history-panel",
            role: "dialog",
            aria_label: "Note history",
            aria_modal: "true",
            onkeydown: move |event| {
                if event.key() == Key::Tab {
                    event.prevent_default();
                    let backwards = event.modifiers().contains(Modifiers::SHIFT);
                    crate::ui::focus::trap_tab(".history-panel", backwards);
                }
            },

            header { class: "history-header",
                div { class: "history-heading",
                    div { class: "history-mark", Icon { name: "clock" } }
                    div {
                        h2 { "History" }
                        p { "{title}" }
                    }
                }
                span { class: "history-total",
                    "{version_count} version"
                    if version_count != 1 { "s" }
                }
                button {
                    id: "history-close",
                    class: "icon-button",
                    r#type: "button",
                    aria_label: "Close history",
                    title: "Close history",
                    autofocus: true,
                    onmounted: move |_| {
                        let _ = document::eval(
                            "requestAnimationFrame(() => document.getElementById('history-close')?.focus());",
                        );
                    },
                    onclick: {
                        let context = context.clone();
                        move |_| context.close_history()
                    },
                    Icon { name: "x" }
                }
            }

            div { class: "history-body",
                aside { class: "history-timeline", role: "listbox", aria_label: "Versions",
                    if versions.is_empty() {
                        div { class: "history-empty",
                            Icon { name: "clock" }
                            p { "No saved versions yet." }
                            span { "Type something and it will appear here." }
                        }
                    } else {
                        for version in versions.iter().rev() {
                            VersionRow {
                                key: "{version.id}",
                                id: version.id.clone(),
                                day: day_label(clock, version.started_at),
                                relative: relative_time(clock, version.ended_at),
                                changes: change_label(version.change_count),
                                peer: peer_label(version.peer, local_peer),
                                selected: selected_id.as_deref() == Some(version.id.as_str()),
                                current: current_id.as_deref() == Some(version.id.as_str()),
                            }
                        }
                    }
                }

                section { class: "history-detail",
                    header { class: "history-detail-header",
                        div { class: "history-detail-title",
                            if selected.is_some() {
                                h3 { "{selected_day}" }
                                p { "{selected_meta}" }
                            } else {
                                h3 { "Nothing selected" }
                                p { "Choose a version on the left." }
                            }
                        }
                        div { class: "history-detail-tools",
                            div { class: "segmented", role: "tablist", aria_label: "History view",
                                for option in [HistoryMode::Changes, HistoryMode::Preview] {
                                    button {
                                        key: "{option.label()}",
                                        class: if mode == option { "segmented-option is-active" } else { "segmented-option" },
                                        r#type: "button",
                                        role: "tab",
                                        aria_selected: if mode == option { "true" } else { "false" },
                                        onclick: {
                                            let context = context.clone();
                                            move |_| context.set_history_mode(option)
                                        },
                                        if option == HistoryMode::Changes {
                                            Icon { name: "list" }
                                        } else {
                                            Icon { name: "eye" }
                                        }
                                        span { "{option.label()}" }
                                    }
                                }
                            }
                            button {
                                class: "primary-button history-restore",
                                r#type: "button",
                                disabled: !can_restore,
                                title: if can_restore { "Restore this version as a new edit" } else { "This version matches the current note" },
                                onclick: {
                                    let context = context.clone();
                                    let version_id = restore_id.clone();
                                    move |_| context.restore_history_version(&version_id)
                                },
                                Icon { name: "undo" }
                                span { "Restore" }
                            }
                        }
                    }

                    div { class: "history-detail-body",
                        if let Some(message) = error {
                            div { class: "history-error", role: "alert",
                                Icon { name: "clock" }
                                span { "{message}" }
                            }
                        }
                        if selected.is_none() {
                            div { class: "history-empty",
                                Icon { name: "clock" }
                                p { "Pick a version to compare it with the current note." }
                            }
                        } else if mode == HistoryMode::Changes {
                            if diff.is_empty() {
                                div { class: "history-empty",
                                    Icon { name: "check" }
                                    p { "This version matches the current note." }
                                    span { "Restore is disabled until the note changes." }
                                }
                            } else {
                                pre { class: "history-diff", aria_label: "Changes from this version to the current note",
                                    for line in diff_lines {
                                        span { class: diff_class(line), "{line}" }
                                    }
                                }
                            }
                        } else if preview.is_empty() {
                            div { class: "history-empty",
                                Icon { name: "note" }
                                p { "This version had no text." }
                            }
                        } else {
                            article {
                                class: "history-preview markdown-body",
                                aria_label: "Version preview",
                                dangerous_inner_html: "{preview_html}",
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn VersionRow(
    id: String,
    day: String,
    relative: String,
    changes: String,
    peer: String,
    selected: bool,
    current: bool,
) -> Element {
    let context = use_context::<AppContext>();
    let class = format!(
        "history-version{}{}",
        if selected { " is-selected" } else { "" },
        if current { " is-current" } else { "" }
    );
    rsx! {
        button {
            class: "{class}",
            r#type: "button",
            role: "option",
            aria_selected: if selected { "true" } else { "false" },
            onclick: move |_| context.select_history_version(&id),
            span { class: "history-dot" }
            span { class: "history-version-text",
                span { class: "history-version-line",
                    strong { "{day}" }
                    if current {
                        span { class: "history-current-pill", "Current" }
                    }
                }
                if !relative.is_empty() {
                    span { class: "history-relative", "{relative}" }
                }
                span { class: "history-version-meta", "{changes} \u{00b7} {peer}" }
            }
        }
    }
}

/// Class for one unified-diff line.
fn diff_class(line: &str) -> &'static str {
    if line.starts_with("+++") || line.starts_with("---") {
        "diff-meta"
    } else if line.starts_with('+') {
        "diff-add"
    } else if line.starts_with('-') {
        "diff-del"
    } else if line.starts_with("@@") {
        "diff-hunk"
    } else {
        "diff-context"
    }
}

fn change_label(count: usize) -> String {
    match count {
        1 => "1 change".to_string(),
        count => format!("{count} changes"),
    }
}

fn peer_label(peer: u64, local: u64) -> String {
    if peer == local {
        "This device".to_string()
    } else {
        format!("Device {:04x}", peer & 0xffff)
    }
}

#[cfg(test)]
mod tests {
    use super::{change_label, diff_class, peer_label};

    #[test]
    fn diff_lines_get_a_tone() {
        assert_eq!(diff_class("+ added"), "diff-add");
        assert_eq!(diff_class("- removed"), "diff-del");
        assert_eq!(diff_class("@@ -1 +1 @@"), "diff-hunk");
        assert_eq!(diff_class("--- a"), "diff-meta");
        assert_eq!(diff_class("+hello"), "diff-add");
    }

    #[test]
    fn history_metadata_reads_naturally() {
        assert_eq!(change_label(1), "1 change");
        assert_eq!(change_label(4), "4 changes");
        assert_eq!(peer_label(42, 42), "This device");
        assert_eq!(peer_label(0x1234_5678, 42), "Device 5678");
    }
}
