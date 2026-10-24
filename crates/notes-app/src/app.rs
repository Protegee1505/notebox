//! The application root: layout, keyboard shortcuts and the autosave ticker.

use std::time::Duration;

use dioxus::prelude::*;
use futures_timer::Delay;
use notes_store::NoteView;

use crate::commands::filtered;
use crate::state::{AppContext, Screen};
use crate::ui::{
    CommandPalette, EditorPane, LoginScreen, NoteContextMenu, NoteList, SettingsPane,
    ShortcutSheet, Sidebar, ToastView, TopBar, UpdateBanner,
};

const STYLE: Asset = asset!("/assets/main.css");

#[component]
pub fn App() -> Element {
    let context = use_hook(AppContext::new);
    use_context_provider(|| context.clone());

    // html, body and the static crash panel paint with the same tokens as the
    // app, so an area the virtual keyboard exposes below the app is themed
    // instead of showing the light default.
    let theme_context = context.clone();
    let theme_attr = use_memo(move || theme_context.state.read().theme.attr());
    use_effect(move || {
        let _ = document::eval(&format!(
            "document.documentElement.setAttribute('data-theme', '{}');",
            theme_attr()
        ));
    });

    #[cfg(target_arch = "wasm32")]
    use_hook({
        let context = context.clone();
        move || crate::pwa::listen(context)
    });

    let tick_context = context.clone();
    use_future(move || {
        let context = tick_context.clone();
        async move {
            loop {
                Delay::new(Duration::from_millis(700)).await;
                context.tick();
            }
        }
    });

    // Storage opens after the first paint so the browser can show a boot
    // screen instead of a blank page while OPFS is read.
    let start_context = context.clone();
    use_future(move || {
        let context = start_context.clone();
        async move {
            context.start().await;
        }
    });

    let state = context.state.read();
    let screen = state.screen;
    let theme = state.theme;
    let mobile_list = state.mobile_list;
    let browsing = state.browsing;
    let sidebar_collapsed = state.sidebar_collapsed;
    let palette_open = state.palette_open;
    let shortcuts_open = state.shortcuts_open;
    let note_menu = state.note_menu;
    let storage_error = state.storage_error.clone();
    let starting = state.starting;
    let storage_blocked = state.storage_blocked.clone();
    let update_ready = state.update_ready && !state.update_dismissed;
    drop(state);

    let mut workspace_class = String::from("workspace");
    if screen == Screen::Settings {
        workspace_class.push_str(" settings-mode");
    }
    if mobile_list {
        workspace_class.push_str(" show-list");
    } else {
        workspace_class.push_str(" show-editor");
    }
    if browsing && screen == Screen::Notes {
        workspace_class.push_str(" browsing");
    }
    if sidebar_collapsed {
        workspace_class.push_str(" nav-collapsed");
    }

    rsx! {
        document::Title { "Notebox" }
        document::Stylesheet { href: STYLE }

        div {
            id: "app-root",
            class: "app",
            "data-theme": theme.attr(),
            tabindex: "-1",
            onmounted: move |_| {
                let _ = document::eval(
                    "setTimeout(() => document.getElementById('app-root')?.focus(), 0);",
                );
            },
            onclick: {
                let context = context.clone();
                move |_| {
                    // A click anywhere outside the colour menu or account
                    // panel dismisses it and still reaches the control under
                    // the finger. There is no full-screen backdrop that could
                    // swallow the tap.
                    let cleared_text_selection = {
                        let mut state = context.state.write_unchecked();
                        if state.color_open {
                            state.color_open = false;
                        }
                        if state.account_open {
                            state.account_open = false;
                        }
                        if state.note_menu.is_some() {
                            state.note_menu = None;
                        }
                        state.text_select_note.take().is_some()
                    };
                    if cleared_text_selection {
                        let _ =
                            document::eval("window.getSelection()?.removeAllRanges();");
                    }
                }
            },
            oncontextmenu: {
                let context = context.clone();
                move |_| {
                    // A secondary click outside a card closes a menu that is
                    // already open, then lets the platform handle the click.
                    if context.state.peek().note_menu.is_some() {
                        context.close_note_menu(false);
                    }
                }
            },
            onkeydown: {
                let context = context.clone();
                move |event| handle_shortcut(&context, &event)
            },

            if starting {
                main { class: "fatal-screen",
                    div { class: "fatal-card",
                        h1 { "Notebox" }
                        p { "Opening your notes..." }
                    }
                }
            } else if let Some(message) = storage_blocked {
                main { class: "fatal-screen",
                    div { class: "fatal-card",
                        h1 { "Notebox is already open" }
                        p { "{message}" }
                        p { class: "card-note",
                            "Only one tab can write this browser's notes. Nothing was deleted."
                        }
                        button {
                            class: "secondary-button",
                            r#type: "button",
                            onclick: move |_| {
                                let _ = document::eval("location.reload();");
                            },
                            "Reload"
                        }
                    }
                }
            } else if screen == Screen::Login {
                LoginScreen {}
            } else {
                if update_ready {
                    UpdateBanner {}
                }
                TopBar {}
                if let Some(error) = storage_error {
                    main { class: "fatal-screen",
                        div { class: "fatal-card",
                            h1 { "The local database did not open" }
                            p { "{error}" }
                            p { class: "card-note",
                                "Nothing was deleted. The saved data was left untouched. Fix the problem, then restart Notebox."
                            }
                        }
                    }
                } else {
                    div { class: "{workspace_class}",
                        Sidebar {}
                        if screen == Screen::Settings {
                            SettingsPane {}
                        } else {
                            NoteList {}
                            EditorPane {}
                        }
                    }
                }
            }

            if let Some(menu) = note_menu {
                NoteContextMenu { key: "{menu.note_id}-{menu.x}-{menu.y}", menu }
            }
            if palette_open {
                CommandPalette {}
            }
            if shortcuts_open {
                ShortcutSheet {}
            }
            ToastView {}
        }
    }
}

/// Global shortcuts. Panels handle their own keys first and stop propagation.
fn handle_shortcut(context: &AppContext, event: &KeyboardEvent) {
    let modifiers = event.modifiers();
    let command = modifiers.contains(Modifiers::CONTROL) || modifiers.contains(Modifiers::META);
    let key = event.key();

    if key == Key::Escape {
        context.escape();
        return;
    }
    if context.state.peek().screen == Screen::Login {
        return;
    }
    if !command {
        return;
    }
    // Every command changes the view or the document, so a card menu must not
    // stay open while its target may be moving.
    context.close_note_menu(false);
    context.end_text_selection();
    let shift = modifiers.contains(Modifiers::SHIFT);
    match key {
        Key::Character(value) => match value.to_lowercase().as_str() {
            "k" => {
                event.prevent_default();
                context.focus_search();
            }
            "p" => {
                event.prevent_default();
                if context.state.peek().palette_open {
                    context.close_palette();
                } else {
                    context.open_palette();
                }
            }
            "n" => {
                event.prevent_default();
                context.new_note();
            }
            "s" => {
                event.prevent_default();
                context.save_now();
                context.toast_success("Saved");
            }
            "f" => {
                event.prevent_default();
                if !context.state.peek().find_open {
                    context.toggle_find();
                }
            }
            "e" => {
                event.prevent_default();
                context.toggle_preview();
            }
            "z" if shift => {
                event.prevent_default();
                context.redo();
            }
            "z" => {
                event.prevent_default();
                context.undo();
            }
            "1" => {
                event.prevent_default();
                context.set_view(NoteView::Active);
            }
            "2" => {
                event.prevent_default();
                context.set_view(NoteView::Archived);
            }
            "3" => {
                event.prevent_default();
                context.set_view(NoteView::Trash);
            }
            "," => {
                event.prevent_default();
                context.open_settings();
            }
            _ => {}
        },
        Key::Enter if context.state.peek().palette_open => {
            event.prevent_default();
            let query = context.state.peek().palette_query.clone();
            let index = context.state.peek().palette_index;
            if let Some(command) = filtered(&query).get(index).copied() {
                command.run(context);
            }
        }
        _ => {}
    }
}
