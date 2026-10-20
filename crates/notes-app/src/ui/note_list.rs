//! The note list: a Keep-style card grid or a compact list.

use dioxus::prelude::*;
use notes_store::{NoteSummary, NoteView};

use crate::state::{edited_label, note_title, tone_class, AppContext, Layout};
use crate::ui::icons::Icon;

#[component]
pub fn NoteList() -> Element {
    let context = use_context::<AppContext>();
    let state = context.state.read();
    let view = state.view;
    let search = state.search.clone();
    let hits = state.search_hits;
    let layout = state.layout;
    let notes = state.notes.clone();
    let snippets = state.snippets.clone();
    let selected = state.selected;
    let confirm_empty = state.confirm_empty_trash;
    let tag = state.tag.clone();
    let notebook = state.notebook.and_then(|id| {
        state
            .notebooks
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.clone())
    });
    drop(state);

    let heading = if !search.trim().is_empty() {
        "Search".to_string()
    } else {
        match view {
            NoteView::Active => notebook.unwrap_or_else(|| {
                if let Some(tag) = tag {
                    format!("#{tag}")
                } else {
                    "All notes".to_string()
                }
            }),
            NoteView::Archived => "Archive".to_string(),
            NoteView::Trash => "Trash".to_string(),
            NoteView::All => "All notes".to_string(),
        }
    };

    let empty_search = !search.trim().is_empty();
    let empty_view = view == NoteView::Trash && notes.is_empty();
    let empty_icon = if empty_search {
        "search"
    } else if empty_view {
        "trash"
    } else {
        "sparkle"
    };
    let list_class = match layout {
        Layout::Grid => "note-list note-grid",
        Layout::List => "note-list note-rows",
    };

    rsx! {
        section { class: "list-pane", aria_label: "Notes",
            header { class: "list-header",
                div { class: "list-heading",
                    h2 { "{heading}" }
                    span { class: "list-count",
                        if let Some(count) = hits {
                            "{count} found"
                        } else {
                            "{notes.len()} notes"
                        }
                    }
                }
                div { class: "list-actions",
                    if view == NoteView::Trash && !notes.is_empty() {
                        button {
                            class: "trash-empty-button",
                            r#type: "button",
                            aria_label: "Empty trash",
                            aria_expanded: if confirm_empty { "true" } else { "false" },
                            title: "Empty the trash permanently",
                            onclick: {
                                let context = context.clone();
                                move |_| {
                                    let open = context.state.peek().confirm_empty_trash;
                                    context.state.write_unchecked().confirm_empty_trash = !open;
                                }
                            },
                            Icon { name: "trash" }
                            span { "Empty trash" }
                        }
                    }
                    div { class: "list-tools",
                        button {
                            class: if layout == Layout::Grid { "segmented-button is-active" } else { "segmented-button" },
                            r#type: "button",
                            aria_label: "Card grid",
                            aria_pressed: if layout == Layout::Grid { "true" } else { "false" },
                            title: "Cards",
                            onclick: {
                                let context = context.clone();
                                move |_| context.set_layout(Layout::Grid)
                            },
                            Icon { name: "grid" }
                        }
                        button {
                            class: if layout == Layout::List { "segmented-button is-active" } else { "segmented-button" },
                            r#type: "button",
                            aria_label: "List view",
                            aria_pressed: if layout == Layout::List { "true" } else { "false" },
                            title: "List",
                            onclick: {
                                let context = context.clone();
                                move |_| context.set_layout(Layout::List)
                            },
                            Icon { name: "list" }
                        }
                    }
                }
            }

            if view == NoteView::Trash && confirm_empty && !notes.is_empty() {
                div { class: "trash-confirm", role: "alert",
                    div { class: "trash-confirm-copy",
                        strong { "Erase the trash?" }
                        span {
                            "This removes {notes.len()} note"
                            if notes.len() != 1 { "s" }
                            " permanently. It cannot be undone."
                        }
                    }
                    div { class: "trash-confirm-actions",
                        button {
                            class: "secondary-button",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.state.write_unchecked().confirm_empty_trash = false
                            },
                            "Cancel"
                        }
                        button {
                            class: "danger-button trash-confirm-erase",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.empty_trash()
                            },
                            Icon { name: "trash" }
                            span { "Erase" }
                        }
                    }
                }
            }

            if notes.is_empty() {
                div { class: "empty-list",
                    div { class: "empty-illustration",
                        Icon { name: empty_icon }
                    }
                    h3 {
                        if empty_search {
                            "Nothing matches"
                        } else if empty_view {
                            "The trash is empty"
                        } else if view == NoteView::Archived {
                            "Nothing archived"
                        } else {
                            "A quiet page"
                        }
                    }
                    p {
                        if empty_search {
                            "Try a shorter word, or clear the search."
                        } else if empty_view {
                            "Notes you delete wait here until you erase them."
                        } else {
                            "Start with a blank note. The first line becomes its title."
                        }
                    }
                    if empty_search {
                        button {
                            class: "text-button",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.set_search(String::new())
                            },
                            "Clear search"
                        }
                    } else if !empty_view {
                        button {
                            class: "primary-button",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.new_note()
                            },
                            Icon { name: "plus" }
                            span { "New note" }
                        }
                    }
                }
            } else {
                div { class: "{list_class}",
                    for note in notes {
                        NoteCard {
                            key: "{note.id}",
                            note: note.clone(),
                            snippet: snippets.iter().find(|(id, _)| *id == note.id).map(|(_, text)| text.clone()),
                            selected: selected == Some(note.id),
                            trash_view: view == NoteView::Trash,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn NoteCard(
    note: NoteSummary,
    snippet: Option<String>,
    selected: bool,
    trash_view: bool,
) -> Element {
    let context = use_context::<AppContext>();
    let id = note.id;
    let title = note_title(&note);
    let preview = snippet
        .as_deref()
        .map(|text| text.replace(['[', ']'], ""))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| note.preview.clone());
    let tags = note.tags.clone();
    let pinned = note.pinned;
    let color = tone_class(note.color.as_deref());
    let stamp = if trash_view {
        note.trashed_at.unwrap_or(note.updated_at)
    } else {
        note.updated_at
    };
    let edited = edited_label(stamp);
    let class = format!(
        "note-card {color}{}{}",
        if selected { " is-selected" } else { "" },
        if trash_view { " is-trashed" } else { "" }
    );

    rsx! {
        article {
            class: "{class}",
            role: "option",
            tabindex: "0",
            aria_selected: if selected { "true" } else { "false" },
            onclick: {
                let context = context.clone();
                move |_| context.select_note(id)
            },
            onkeydown: {
                let context = context.clone();
                move |event| {
                    if event.key() == Key::Enter || event.key() == Key::Character(" ".to_string()) {
                        event.prevent_default();
                        context.select_note(id);
                    }
                }
            },
            div { class: "note-card-top",
                if pinned {
                    span { class: "pin-mark", title: "Pinned", Icon { name: "pin" } }
                }
                div { class: "note-tags",
                    for tag in tags.iter().take(2) {
                        span { class: "note-tag", "#{tag}" }
                    }
                    if tags.len() > 2 {
                        span { class: "note-tag", "+{tags.len() - 2}" }
                    }
                }
            }
            h3 { class: "note-title", "{title}" }
            if !preview.is_empty() {
                p { class: "note-preview", "{preview}" }
            }
            footer { class: "note-meta",
                span { "{edited}" }
                if trash_view {
                    button {
                        class: "restore-button",
                        r#type: "button",
                        title: "Restore note",
                        onclick: {
                            let context = context.clone();
                            move |event| {
                                event.stop_propagation();
                                context.restore_note(id);
                            }
                        },
                        Icon { name: "undo" }
                        span { "Restore" }
                    }
                }
            }
        }
    }
}
