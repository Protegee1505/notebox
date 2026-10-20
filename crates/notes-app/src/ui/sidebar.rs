//! Notebooks, tags and the view switcher.

use dioxus::prelude::*;
use notes_store::NoteView;

use crate::state::{AppContext, Screen, Theme};
use crate::ui::icons::Icon;

#[component]
pub fn Sidebar() -> Element {
    let context = use_context::<AppContext>();
    let state = context.state.read();
    let view = state.view;
    let notebook_filter = state.notebook;
    let tag_filter = state.tag.clone();
    let notebooks = state.notebooks.clone();
    let tags = state.tags.clone();
    let note_count = state.note_count;
    let storage = state.storage.clone();
    let screen = state.screen;
    let theme = state.theme;
    let sidebar_open = state.sidebar_open;
    let new_notebook = state.new_notebook.clone();
    drop(state);

    rsx! {
        if sidebar_open {
            div {
                class: "scrim",
                onclick: {
                    let context = context.clone();
                    move |_| context.state.write_unchecked().sidebar_open = false
                }
            }
        }
        aside {
            class: if sidebar_open { "sidebar is-open" } else { "sidebar" },
            aria_label: "Notebooks and tags",
            div { class: "sidebar-inner",
                button {
                    class: "new-note-button",
                    r#type: "button",
                    onclick: {
                        let context = context.clone();
                        move |_| {
                            context.new_note();
                            context.state.write_unchecked().sidebar_open = false;
                        }
                    },
                    Icon { name: "plus" }
                    span { "New note" }
                    kbd { class: "key-hint on-primary", "N" }
                }

                nav { class: "sidebar-nav", aria_label: "Views",
                    button {
                        class: if view == NoteView::Active { "nav-row is-active" } else { "nav-row" },
                        r#type: "button",
                        aria_current: if view == NoteView::Active { "page" } else { "false" },
                        onclick: {
                            let context = context.clone();
                            move |_| context.set_view(NoteView::Active)
                        },
                        Icon { name: "note" }
                        span { class: "nav-label", "Notes" }
                        span { class: "nav-count", "{note_count}" }
                    }
                    button {
                        class: if view == NoteView::Archived { "nav-row is-active" } else { "nav-row" },
                        r#type: "button",
                        onclick: {
                            let context = context.clone();
                            move |_| context.set_view(NoteView::Archived)
                        },
                        Icon { name: "archive" }
                        span { class: "nav-label", "Archive" }
                    }
                    button {
                        class: if view == NoteView::Trash { "nav-row is-active" } else { "nav-row" },
                        r#type: "button",
                        onclick: {
                            let context = context.clone();
                            move |_| context.set_view(NoteView::Trash)
                        },
                        Icon { name: "trash" }
                        span { class: "nav-label", "Trash" }
                    }
                }

                div { class: "sidebar-section",
                    div { class: "section-heading",
                        span { "Notebooks" }
                        button {
                            class: "mini-button",
                            r#type: "button",
                            aria_label: "New notebook",
                            title: "New notebook",
                            onclick: {
                                let context = context.clone();
                                move |_| context.state.write_unchecked().new_notebook = Some(String::new())
                            },
                            Icon { name: "plus" }
                        }
                    }
                    if let Some(value) = new_notebook.as_deref() {
                        form {
                            class: "inline-form",
                            onsubmit: {
                                let context = context.clone();
                                move |event| {
                                    event.prevent_default();
                                    let name = context.state.peek().new_notebook.clone().unwrap_or_default();
                                    if name.trim().is_empty() {
                                        context.state.write_unchecked().new_notebook = None;
                                    } else {
                                        context.create_notebook(name);
                                    }
                                }
                            },
                            input {
                                class: "inline-input",
                                r#type: "text",
                                placeholder: "Notebook name",
                                aria_label: "New notebook name",
                                autofocus: true,
                                value: "{value}",
                                oninput: {
                                    let context = context.clone();
                                    move |event| {
                                        context.state.write_unchecked().new_notebook = Some(event.value());
                                    }
                                },
                                onkeydown: {
                                    let context = context.clone();
                                    move |event| {
                                        if event.key() == Key::Escape {
                                            context.state.write_unchecked().new_notebook = None;
                                        }
                                    }
                                },
                            }
                        }
                    }
                    for notebook in notebooks.iter() {
                        button {
                            key: "{notebook.id}",
                            class: if notebook_filter == Some(notebook.id) { "nav-row is-active" } else { "nav-row" },
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                let id = notebook.id;
                                let active = notebook_filter == Some(notebook.id);
                                move |_| context.filter_notebook(if active { None } else { Some(id) })
                            },
                            span { class: "notebook-dot" }
                            span { class: "nav-label", "{notebook.name}" }
                        }
                    }
                    if notebooks.is_empty() && new_notebook.is_none() {
                        p { class: "sidebar-hint", "No notebooks yet." }
                    }
                }

                div { class: "sidebar-section tags-section",
                    div { class: "section-heading",
                        span { "Tags" }
                    }
                    div { class: "tag-cloud",
                        for tag in tags.iter() {
                            button {
                                key: "{tag.tag}",
                                class: if tag_filter.as_deref() == Some(tag.tag.as_str()) { "tag-chip is-active" } else { "tag-chip" },
                                r#type: "button",
                                onclick: {
                                    let context = context.clone();
                                    let name = tag.tag.clone();
                                    let active = tag_filter.as_deref() == Some(tag.tag.as_str());
                                    move |_| context.filter_tag(if active { None } else { Some(name.clone()) })
                                },
                                span { "#{tag.tag}" }
                                span { class: "tag-count", "{tag.count}" }
                            }
                        }
                        if tags.is_empty() {
                            p { class: "sidebar-hint", "Tags you add appear here." }
                        }
                    }
                }

                div { class: "sidebar-footer",
                    if storage.temporary {
                        div { class: "storage-note",
                            Icon { name: "database" }
                            span { "{storage.label} \u{00b7} not saved after reload" }
                        }
                    }
                    button {
                        class: "nav-row",
                        r#type: "button",
                        onclick: {
                            let context = context.clone();
                            move |_| {
                                context.open_palette();
                                context.state.write_unchecked().sidebar_open = false;
                            }
                        },
                        Icon { name: "command" }
                        span { class: "nav-label", "Commands" }
                        kbd { class: "key-hint", "Ctrl K" }
                    }
                    button {
                        class: if screen == Screen::Settings { "nav-row is-active" } else { "nav-row" },
                        r#type: "button",
                        onclick: {
                            let context = context.clone();
                            move |_| context.open_settings()
                        },
                        Icon { name: "settings" }
                        span { class: "nav-label", "Settings" }
                    }
                    button {
                        class: "nav-row",
                        r#type: "button",
                        onclick: {
                            let context = context.clone();
                            move |_| context.toggle_theme()
                        },
                        Icon { name: match theme { Theme::Light => "moon", _ => "sun" } }
                        span { class: "nav-label", "Switch theme" }
                    }
                }
            }
        }
    }
}
