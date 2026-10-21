//! The keyboard reference sheet.

use dioxus::prelude::*;

use crate::state::AppContext;
use crate::ui::icons::Icon;

const GROUPS: [(&str, [(&str, &str); 4]); 3] = [
    (
        "General",
        [
            ("Ctrl K", "Command palette"),
            ("Ctrl N", "New note"),
            ("Ctrl S", "Save now"),
            ("Esc", "Close a panel"),
        ],
    ),
    (
        "Editor",
        [
            ("Ctrl F", "Find in note"),
            ("Ctrl E", "Write and preview"),
            ("Ctrl Z", "Undo"),
            ("Ctrl Shift Z", "Redo"),
        ],
    ),
    (
        "Navigation",
        [
            ("Ctrl 1", "All notes"),
            ("Ctrl 2", "Archive"),
            ("Ctrl 3", "Trash"),
            ("Ctrl ,", "Settings"),
        ],
    ),
];

#[component]
pub fn ShortcutSheet() -> Element {
    let context = use_context::<AppContext>();
    use_drop(move || {
        let _ = document::eval("document.getElementById('app-root')?.focus();");
    });
    rsx! {
        div { class: "overlay", role: "presentation",
            div {
                class: "overlay-backdrop",
                onclick: {
                    let context = context.clone();
                    move |_| context.state.write_unchecked().shortcuts_open = false
                }
            }
            div { class: "shortcuts-sheet", role: "dialog", aria_modal: "true", aria_label: "Keyboard shortcuts",
                header { class: "sheet-header",
                    div {
                        h2 { "Keyboard shortcuts" }
                        p { "Small moves that keep your hands on the keys." }
                    }
                    button {
                        class: "icon-button",
                        r#type: "button",
                        aria_label: "Close shortcuts",
                        title: "Close",
                        autofocus: true,
                        onclick: {
                            let context = context.clone();
                            move |_| context.state.write_unchecked().shortcuts_open = false
                        },
                        Icon { name: "x" }
                    }
                }
                div { class: "shortcut-groups",
                    for (title, rows) in GROUPS {
                        section { key: "{title}", class: "shortcut-group",
                            h3 { "{title}" }
                            for (keys, label) in rows {
                                div { key: "{keys}", class: "shortcut-row",
                                    span { "{label}" }
                                    kbd { class: "key-hint", "{keys}" }
                                }
                            }
                        }
                    }
                }
                footer { class: "sheet-footer",
                    span { "Tip: type " }
                    kbd { class: "key-hint", "Ctrl K" }
                    span { " and start typing what you want to do." }
                }
            }
        }
    }
}
