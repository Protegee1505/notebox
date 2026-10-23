//! Note actions opened from a card with the secondary mouse button.

use dioxus::prelude::*;

use crate::state::{note_title, tone_class, AppContext, NoteMenu, COLORS};
use crate::ui::icons::Icon;

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuPage {
    Main,
    Notebook,
}

#[component]
pub fn NoteContextMenu(menu: NoteMenu) -> Element {
    let context = use_context::<AppContext>();
    let mut page = use_signal(|| MenuPage::Main);
    let state = context.state.read();
    let summary = state
        .notes
        .iter()
        .find(|note| note.id == menu.note_id)
        .cloned();
    let notebooks = state.notebooks.clone();
    drop(state);

    let Some(summary) = summary else {
        return rsx! {};
    };

    let id = summary.id;
    let title = note_title(&summary);
    let trashed = summary.trashed_at.is_some();
    let pinned = summary.pinned;
    let archived = summary.archived;
    let color = summary.color.clone();
    let current_notebook = summary.notebook_id.and_then(|id| {
        notebooks
            .iter()
            .find(|notebook| notebook.id == id)
            .map(|notebook| notebook.name.clone())
    });

    let x = menu.x;
    let y = menu.y;
    use_effect(move || {
        // Reading the page makes the effect rerun when the drill-in changes and
        // the menu needs a new height and a new initial focus.
        let _page = page();
        let _ = document::eval(&format!(
            r#"(function () {{
  const menu = document.getElementById('note-context-menu');
  if (!menu) return;
  menu.style.visibility = 'hidden';
  const pad = 8;
  const view = window.visualViewport;
  const left = view?.offsetLeft ?? 0;
  const top = view?.offsetTop ?? 0;
  const width = view?.width ?? window.innerWidth;
  const height = view?.height ?? window.innerHeight;
  const rect = menu.getBoundingClientRect();
  const maxX = Math.max(left + pad, left + width - rect.width - pad);
  const maxY = Math.max(top + pad, top + height - rect.height - pad);
  const px = Math.min(Math.max(left + pad, {x}), maxX);
  const py = Math.min(Math.max(top + pad, {y}), maxY);
  menu.style.left = px + 'px';
  menu.style.top = py + 'px';
  menu.style.visibility = 'visible';
  const first = menu.querySelector('.context-menu-initial')
    || menu.querySelector('[role="menuitem"], [role="menuitemradio"]');
  first?.focus();
}})();"#
        ));
    });

    rsx! {
        div {
            id: "note-context-menu",
            class: "note-context-menu",
            role: "menu",
            aria_label: "Note actions for {title}",
            style: "left: {x}px; top: {y}px;",
            onclick: move |event| event.stop_propagation(),
            oncontextmenu: move |event| {
                event.prevent_default();
                event.stop_propagation();
            },
            onkeydown: {
                let context = context.clone();
                move |event| match event.key() {
                    Key::ArrowDown => {
                        event.prevent_default();
                        move_focus(1);
                    }
                    Key::ArrowUp => {
                        event.prevent_default();
                        move_focus(-1);
                    }
                    Key::Home => {
                        event.prevent_default();
                        focus_edge(false);
                    }
                    Key::End => {
                        event.prevent_default();
                        focus_edge(true);
                    }
                    Key::ArrowLeft if page() == MenuPage::Notebook => {
                        event.prevent_default();
                        page.set(MenuPage::Main);
                    }
                    Key::Tab => {
                        event.prevent_default();
                        context.close_note_menu(true);
                    }
                    _ => {}
                }
            },

            if page() == MenuPage::Main {
                button {
                    id: "note-menu-open",
                    class: "context-menu-item context-menu-initial",
                    r#type: "button",
                    role: "menuitem",
                    onclick: {
                        let context = context.clone();
                        move |_| context.open_note_from_menu(id)
                    },
                    Icon { name: "note" }
                    span { "Open note" }
                }
                if trashed {
                    button {
                        id: "note-menu-history",
                        class: "context-menu-item",
                        r#type: "button",
                        role: "menuitem",
                        onclick: {
                            let context = context.clone();
                            move |_| context.open_history_from_menu(id)
                        },
                        Icon { name: "clock" }
                        span { "History" }
                    }
                    div { class: "context-menu-sep", role: "separator" }
                    button {
                        id: "note-menu-restore",
                        class: "context-menu-item",
                        r#type: "button",
                        role: "menuitem",
                        onclick: {
                            let context = context.clone();
                            move |_| context.restore_from_menu(id)
                        },
                        Icon { name: "undo" }
                        span { "Restore note" }
                    }
                } else {
                    button {
                        id: "note-menu-pin",
                        class: "context-menu-item",
                        r#type: "button",
                        role: "menuitem",
                        onclick: {
                            let context = context.clone();
                            move |_| context.toggle_pin_from_menu(id)
                        },
                        Icon { name: "pin" }
                        span {
                            if pinned { "Unpin note" } else { "Pin note" }
                        }
                        if pinned {
                            span { class: "context-menu-check", aria_hidden: "true", Icon { name: "check" } }
                        }
                    }
                    button {
                        id: "note-menu-archive",
                        class: "context-menu-item",
                        r#type: "button",
                        role: "menuitem",
                        onclick: {
                            let context = context.clone();
                            move |_| context.toggle_archive_from_menu(id)
                        },
                        Icon { name: "archive" }
                        span {
                            if archived { "Unarchive note" } else { "Archive note" }
                        }
                        if archived {
                            span { class: "context-menu-check", aria_hidden: "true", Icon { name: "check" } }
                        }
                    }
                    button {
                        id: "note-menu-history",
                        class: "context-menu-item",
                        r#type: "button",
                        role: "menuitem",
                        onclick: {
                            let context = context.clone();
                            move |_| context.open_history_from_menu(id)
                        },
                        Icon { name: "clock" }
                        span { "History" }
                    }
                    div { class: "context-menu-sep", role: "separator" }
                    div {
                        class: "context-menu-group",
                        role: "group",
                        aria_label: "Colour",
                        div { class: "context-menu-label", aria_hidden: "true", "Colour" }
                        div { class: "context-menu-swatches",
                            for (value, label) in COLORS {
                                button {
                                    key: "{value}",
                                    id: "note-menu-color-{value}",
                                    class: if color.as_deref() == Some(value) {
                                        "context-menu-swatch is-active"
                                    } else {
                                        "context-menu-swatch"
                                    },
                                    r#type: "button",
                                    role: "menuitemradio",
                                    aria_label: "Colour: {label}",
                                    aria_checked: if color.as_deref() == Some(value) { "true" } else { "false" },
                                    title: "{label}",
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.set_color_from_menu(id, Some(value))
                                    },
                                    span { class: "swatch-dot {tone_class(Some(value))}", aria_hidden: "true" }
                                }
                            }
                            button {
                                id: "note-menu-color-none",
                                class: if color.is_none() {
                                    "context-menu-color-none is-active"
                                } else {
                                    "context-menu-color-none"
                                },
                                r#type: "button",
                                role: "menuitemradio",
                                aria_label: "No colour",
                                aria_checked: if color.is_none() { "true" } else { "false" },
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.set_color_from_menu(id, None)
                                },
                                "No colour"
                            }
                        }
                    }
                    button {
                        id: "note-menu-notebook",
                        class: "context-menu-item",
                        r#type: "button",
                        role: "menuitem",
                        aria_haspopup: "menu",
                        onclick: move |_| page.set(MenuPage::Notebook),
                        Icon { name: "book" }
                        span { "Move to notebook" }
                        span { class: "context-menu-value",
                            {current_notebook.clone().unwrap_or_else(|| "No notebook".to_string())}
                        }
                    }
                    div { class: "context-menu-sep", role: "separator" }
                    button {
                        id: "note-menu-trash",
                        class: "context-menu-item is-danger",
                        r#type: "button",
                        role: "menuitem",
                        onclick: {
                            let context = context.clone();
                            move |_| context.trash_from_menu(id)
                        },
                        Icon { name: "trash" }
                        span { "Move to trash" }
                    }
                }
            } else {
                div { class: "context-menu-heading",
                    button {
                        id: "note-menu-back",
                        class: "context-menu-back",
                        r#type: "button",
                        role: "menuitem",
                        aria_label: "Back to note actions",
                        onclick: move |_| page.set(MenuPage::Main),
                        Icon { name: "arrow-left" }
                    }
                    span { "Move to notebook" }
                }
                div { class: "context-menu-sep", role: "separator" }
                button {
                    id: "note-menu-notebook-none",
                    class: if current_notebook.is_none() {
                        "context-menu-item context-menu-initial is-active"
                    } else {
                        "context-menu-item context-menu-initial"
                    },
                    r#type: "button",
                    role: "menuitemradio",
                    aria_checked: if current_notebook.is_none() { "true" } else { "false" },
                    onclick: {
                        let context = context.clone();
                        move |_| context.set_notebook_from_menu(id, None)
                    },
                    span { "No notebook" }
                    if current_notebook.is_none() {
                        span { class: "context-menu-check", aria_hidden: "true", Icon { name: "check" } }
                    }
                }
                for notebook in notebooks {
                    button {
                        key: "{notebook.id}",
                        id: "note-menu-notebook-{notebook.id}",
                        class: if summary.notebook_id == Some(notebook.id) {
                            "context-menu-item is-active"
                        } else {
                            "context-menu-item"
                        },
                        r#type: "button",
                        role: "menuitemradio",
                        aria_checked: if summary.notebook_id == Some(notebook.id) { "true" } else { "false" },
                        onclick: {
                            let context = context.clone();
                            move |_| context.set_notebook_from_menu(id, Some(notebook.id))
                        },
                        span { class: "context-menu-label-row", "{notebook.name}" }
                        if summary.notebook_id == Some(notebook.id) {
                            span { class: "context-menu-check", aria_hidden: "true", Icon { name: "check" } }
                        }
                    }
                }
            }
        }
    }
}

fn move_focus(step: isize) {
    let _ = document::eval(&format!(
        r#"(function () {{
  const menu = document.getElementById('note-context-menu');
  if (!menu) return;
  const items = Array.from(menu.querySelectorAll('[role="menuitem"], [role="menuitemradio"]'))
    .filter((item) => item.getClientRects().length > 0);
  if (items.length === 0) return;
  const current = items.indexOf(document.activeElement);
  const next = current === -1 ? 0 : (current + {step} + items.length) % items.length;
  items[next].focus();
}})();"#
    ));
}

fn focus_edge(last: bool) {
    let edge = if last {
        "items[items.length - 1]"
    } else {
        "items[0]"
    };
    let _ = document::eval(&format!(
        r#"(function () {{
  const menu = document.getElementById('note-context-menu');
  if (!menu) return;
  const items = Array.from(menu.querySelectorAll('[role="menuitem"], [role="menuitemradio"]'))
    .filter((item) => item.getClientRects().length > 0);
  if (items.length === 0) return;
  {edge}.focus();
}})();"#
    ));
}
