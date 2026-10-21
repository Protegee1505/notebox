//! The command palette: one search box for every action.

use dioxus::prelude::*;

use crate::commands::filtered;
use crate::state::AppContext;
use crate::ui::icons::Icon;

#[component]
pub fn CommandPalette() -> Element {
    let context = use_context::<AppContext>();
    use_drop(move || {
        let _ = document::eval("document.getElementById('app-root')?.focus();");
    });
    let state = context.state.read();
    let query = state.palette_query.clone();
    let index = state.palette_index;
    drop(state);

    let commands = filtered(&query);
    let selected = commands.get(index).copied();

    rsx! {
        div { class: "overlay", role: "presentation",
            div {
                class: "overlay-backdrop",
                onclick: {
                    let context = context.clone();
                    move |_| context.close_palette()
                }
            }
            div {
                class: "palette",
                role: "dialog",
                aria_modal: "true",
                aria_label: "Command palette",
                onkeydown: move |event| {
                    if event.key() == Key::Tab {
                        event.prevent_default();
                        let backwards = event.modifiers().contains(Modifiers::SHIFT);
                        crate::ui::focus::trap_tab(".palette", backwards);
                    }
                },
                div { class: "palette-search",
                    Icon { name: "search" }
                    input {
                        id: "palette-input",
                        class: "palette-input",
                        r#type: "text",
                        placeholder: "Type a command or search",
                        aria_label: "Command palette",
                        autocomplete: "off",
                        autofocus: true,
                        value: "{query}",
                        oninput: {
                            let context = context.clone();
                            move |event| context.set_palette_query(event.value())
                        },
                        onkeydown: {
                            let context = context.clone();
                            move |event| {
                                match event.key() {
                                    Key::Escape => {
                                        event.prevent_default();
                                        event.stop_propagation();
                                        context.close_palette();
                                    }
                                    Key::ArrowDown => {
                                        event.prevent_default();
                                        event.stop_propagation();
                                        let count = filtered(&context.state.peek().palette_query).len();
                                        if count > 0 {
                                            let next = (context.state.peek().palette_index + 1) % count;
                                            context.set_palette_index(next);
                                        }
                                    }
                                    Key::ArrowUp => {
                                        event.prevent_default();
                                        event.stop_propagation();
                                        let count = filtered(&context.state.peek().palette_query).len();
                                        if count > 0 {
                                            let current = context.state.peek().palette_index;
                                            let next = (current + count - 1) % count;
                                            context.set_palette_index(next);
                                        }
                                    }
                                    Key::Enter => {
                                        event.prevent_default();
                                        event.stop_propagation();
                                        let query = context.state.peek().palette_query.clone();
                                        let index = context.state.peek().palette_index;
                                        if let Some(command) = filtered(&query).get(index).copied() {
                                            command.run(&context);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        },
                    }
                    kbd { class: "key-hint", "Esc" }
                }
                div { class: "palette-list", role: "listbox", aria_label: "Commands",
                    if commands.is_empty() {
                        div { class: "palette-empty",
                            Icon { name: "search" }
                            p { "No command matches \u{201c}{query}\u{201d}." }
                        }
                    } else {
                        for (row, command) in commands.iter().copied().enumerate() {
                            button {
                                key: "{command.label()}",
                                class: if row == index { "palette-row is-active" } else { "palette-row" },
                                r#type: "button",
                                role: "option",
                                aria_selected: if row == index { "true" } else { "false" },
                                onmouseenter: {
                                    let context = context.clone();
                                    move |_| context.set_palette_index(row)
                                },
                                onclick: {
                                    let context = context.clone();
                                    move |_| command.run(&context)
                                },
                                span { class: "palette-icon", Icon { name: command.icon() } }
                                span { class: "palette-label", "{command.label()}" }
                                if !command.hint().is_empty() {
                                    kbd { class: "key-hint", "{command.hint()}" }
                                }
                            }
                        }
                    }
                }
                footer { class: "palette-footer",
                    span { kbd { class: "key-hint", "\u{2191}\u{2193}" } " Navigate" }
                    span { kbd { class: "key-hint", "\u{21b5}" } " Run" }
                    span { kbd { class: "key-hint", "Esc" } " Close" }
                    span { class: "palette-tip",
                        if let Some(command) = selected {
                            "{command.keywords()}"
                        }
                    }
                }
            }
        }
    }
}
