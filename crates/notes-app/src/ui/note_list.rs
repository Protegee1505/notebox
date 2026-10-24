//! The note list: a Keep-style card grid or a compact list.

use dioxus::prelude::*;
use notes_core::plain_line;
use notes_store::{search_terms, NoteSummary, NoteView};

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
    let menu_target = state.note_menu.map(|menu| menu.note_id);
    let text_select_note = state.text_select_note;
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

    let can_add_tour = view == NoteView::Active && tag.is_none() && notebook.is_none();
    let heading = if !search.trim().is_empty() {
        "Search".to_string()
    } else {
        match view {
            NoteView::Active => notebook.clone().unwrap_or_else(|| {
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
                        } else if can_add_tour {
                            "Start with a blank note, or add three notes for a short welcome tour."
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
                        div { class: "empty-actions",
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
                            if can_add_tour {
                                button {
                                    class: "secondary-button",
                                    r#type: "button",
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.add_welcome_tour()
                                    },
                                    Icon { name: "sparkle" }
                                    span { "Add welcome tour" }
                                }
                            }
                        }
                    }
                }
            } else {
                div { class: "{list_class}",
                    role: "listbox",
                    aria_label: "Notes",
                    aria_multiselectable: "false",
                    onscroll: {
                        let context = context.clone();
                        move |_| {
                            if context.state.peek().note_menu.is_some() {
                                context.close_note_menu(false);
                            }
                        }
                    },
                    for note in notes {
                        NoteCard {
                            key: "{note.id}",
                            note: note.clone(),
                            snippet: snippets.iter().find(|(id, _)| *id == note.id).map(|(_, text)| text.clone()),
                            query: search.clone(),
                            selected: selected == Some(note.id),
                            menu_open: menu_target == Some(note.id),
                            select_text: text_select_note == Some(note.id),
                            trash_view: view == NoteView::Trash,
                        }
                    }
                }
            }

            // The sidebar is off-canvas on phones and tablets, so the list
            // carries its own new-note control there. CSS hides it whenever
            // the sidebar's button is on screen.
            button {
                class: "new-note-fab",
                r#type: "button",
                aria_label: "New note",
                title: "New note",
                onclick: {
                    let context = context.clone();
                    move |_| context.new_note()
                },
                Icon { name: "plus" }
            }
        }
    }
}

#[component]
fn NoteCard(
    note: NoteSummary,
    snippet: Option<String>,
    query: String,
    selected: bool,
    menu_open: bool,
    select_text: bool,
    trash_view: bool,
) -> Element {
    let context = use_context::<AppContext>();
    let id = note.id;
    let title = note_title(&note);
    let raw_preview = snippet
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| note.preview.clone());
    let has_image = note.first_image.is_some() || raw_preview.contains("![");
    let preview = preview_text(&raw_preview);
    let image_url = note
        .first_image
        .as_ref()
        .and_then(|image| context.state.read().card_images.get(&image.hash).cloned());
    let media = match image_url {
        Some(url) => rsx! {
            img {
                src: "{url}",
                alt: "",
                loading: "lazy",
                decoding: "async",
            }
        },
        None => rsx! {
            div { class: "note-media-fallback",
                Icon { name: "image" }
                span { aria_hidden: "true", "Image" }
            }
        },
    };
    let tags = tags_for_display(&note.tags, &query);
    let tag_count = note.tags.len();
    let pinned = note.pinned;
    let color = tone_class(note.color.as_deref());
    let stamp = if trash_view {
        note.trashed_at.unwrap_or(note.updated_at)
    } else {
        note.updated_at
    };
    let edited = edited_label(stamp);
    let class = format!(
        "note-card {color}{}{}{}{}{}",
        if has_image { " has-image" } else { "" },
        if selected { " is-selected" } else { "" },
        if menu_open { " is-menu-target" } else { "" },
        if select_text {
            " is-text-selectable"
        } else {
            ""
        },
        if trash_view { " is-trashed" } else { "" }
    );
    let card_id = format!("note-card-{id}");
    let keyboard_card_id = card_id.clone();
    let open_menu_from_keyboard = move || {
        let _ = document::eval(&format!(
            r#"const card = document.getElementById('{keyboard_card_id}');
            if (!card) return;
            const box = card.getBoundingClientRect();
            card.dispatchEvent(new MouseEvent('contextmenu', {{
                bubbles: true,
                cancelable: true,
                button: 2,
                clientX: box.right - 12,
                clientY: box.top + 24,
            }}));"#
        ));
    };
    let activate = {
        let context = context.clone();
        let query = query.clone();
        move || {
            if query.trim().is_empty() {
                context.select_note(id);
            } else {
                context.open_search_result(id, query.clone());
            }
        }
    };

    rsx! {
        article {
            id: "{card_id}",
            class: "{class}",
            role: "option",
            tabindex: "0",
            aria_selected: if selected { "true" } else { "false" },
            aria_haspopup: "menu",
            aria_keyshortcuts: "Shift+F10",
            onclick: {
                let activate = activate.clone();
                let context = context.clone();
                move |event| {
                    if context.state.peek().text_select_note == Some(id) {
                        event.prevent_default();
                        event.stop_propagation();
                        context.end_text_selection();
                        return;
                    }
                    activate()
                }
            },
            oncontextmenu: {
                let context = context.clone();
                move |event| {
                    if context.state.peek().text_select_note == Some(id) {
                        // Let the platform start or extend its text selection
                        // instead of replacing it with the card menu.
                        return;
                    }
                    event.prevent_default();
                    event.stop_propagation();
                    let point = event.client_coordinates();
                    context.open_note_menu(id, point.x, point.y);
                }
            },
            onkeydown: {
                let activate = activate.clone();
                let open_menu_from_keyboard = open_menu_from_keyboard.clone();
                move |event| {
                    match event.key() {
                        Key::Enter => {
                            event.prevent_default();
                            activate();
                        }
                        Key::Character(value) if value == " " => {
                            event.prevent_default();
                            activate();
                        }
                        Key::ArrowDown => {
                            event.prevent_default();
                            let _ = document::eval(
                                "document.activeElement?.nextElementSibling?.focus();",
                            );
                        }
                        Key::ArrowUp => {
                            event.prevent_default();
                            let _ = document::eval(
                                "document.activeElement?.previousElementSibling?.focus();",
                            );
                        }
                        Key::Home => {
                            event.prevent_default();
                            let _ = document::eval(
                                "document.activeElement?.parentElement?.firstElementChild?.focus();",
                            );
                        }
                        Key::End => {
                            event.prevent_default();
                            let _ = document::eval(
                                "document.activeElement?.parentElement?.lastElementChild?.focus();",
                            );
                        }
                        Key::F10 if event.modifiers().contains(Modifiers::SHIFT) => {
                            event.prevent_default();
                            open_menu_from_keyboard();
                        }
                        Key::ContextMenu => {
                            event.prevent_default();
                            open_menu_from_keyboard();
                        }
                        _ => {}
                    }
                }
            },
            div { class: "note-card-top",
                if pinned {
                    span { class: "pin-mark", title: "Pinned", Icon { name: "pin" } }
                }
                div { class: "note-tags",
                    for tag in tags.iter().take(2) {
                        span { class: "note-tag", "#" HighlightedText { text: tag.clone(), query: query.clone() } }
                    }
                    if tag_count > 2 {
                        span { class: "note-tag", "+{tag_count - 2}" }
                    }
                }
            }
            h3 { class: "note-title", HighlightedText { text: title, query: query.clone() } }
            if !preview.is_empty() {
                p { class: "note-preview", HighlightedText { text: preview, query: query.clone() } }
            }
            if has_image {
                div { class: "note-media", role: "img", aria_label: "Image attachment",
                    {media}
                }
            }
            footer { class: "note-meta",
                span { "{edited}" }
            }
        }
    }
}

/// One run of text, either plain or a search match.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TextSegment {
    text: String,
    marked: bool,
}

/// Lowercase `text` only when every character keeps its byte width.
///
/// A matching byte offset can then slice both strings safely. Rare mappings
/// such as `İ` (one character becoming two) return `None`; the caller shows
/// the original text without marks.
fn lowercase_aligned(text: &str) -> Option<String> {
    let mut lower = String::with_capacity(text.len());
    for character in text.chars() {
        let mut mapped = character.to_lowercase();
        let next = mapped.next()?;
        if mapped.next().is_some() || next.len_utf8() != character.len_utf8() {
            return None;
        }
        lower.push(next);
    }
    Some(lower)
}

/// Split `text` into plain and highlighted runs for `query`.
///
/// Search is case-insensitive and prefix-based at a word boundary, matching
/// the FTS index. Literal brackets in a note stay literal: no markup is added
/// to the stored or excerpt text.
fn highlight_segments(text: &str, query: &str) -> Vec<TextSegment> {
    let plain = || {
        vec![TextSegment {
            text: text.to_string(),
            marked: false,
        }]
    };
    if text.is_empty() || query.trim().is_empty() {
        return plain();
    }
    // Every character must keep its byte width when lowercased, or offsets in
    // the lowercase copy no longer point at the same place in `text`. One
    // unusual character (such as `İ`) makes the whole result plain instead of
    // risking a panic or marking the wrong bytes.
    let Some(lower) = lowercase_aligned(text) else {
        return plain();
    };
    let terms: Vec<String> = search_terms(query)
        .into_iter()
        .map(|term| term.to_lowercase())
        .filter(|term| !term.is_empty())
        .collect();
    if terms.is_empty() {
        return plain();
    }

    let mut ranges = Vec::new();
    for term in terms {
        let mut from = 0;
        while let Some(offset) = lower[from..].find(&term) {
            let start = from + offset;
            let end = start + term.len();
            let at_boundary = start == 0
                || !text[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
            if at_boundary {
                ranges.push((start, end));
            }
            from = end.max(start + 1);
        }
    }
    if ranges.is_empty() {
        return plain();
    }
    ranges.sort_by_key(|(start, _)| *start);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut segments = Vec::new();
    let mut cursor = 0;
    for (start, end) in merged {
        if start > cursor {
            segments.push(TextSegment {
                text: text[cursor..start].to_string(),
                marked: false,
            });
        }
        segments.push(TextSegment {
            text: text[start..end].to_string(),
            marked: true,
        });
        cursor = end;
    }
    if cursor < text.len() {
        segments.push(TextSegment {
            text: text[cursor..].to_string(),
            marked: false,
        });
    }
    segments
}

/// Plain card text for a preview, including a search snippet.
///
/// Stored bodies and search snippets can both contain the Markdown source of
/// an image. A complete image becomes its alt text. A snippet can cut the URL
/// before the closing bracket, so the remaining fragment is removed too.
fn preview_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    while let Some(offset) = text[cursor..].find("![") {
        let start = cursor + offset;
        out.push_str(&text[cursor..start]);
        let after = &text[start + 2..];
        let Some(bracket) = after.find(']') else {
            // A snippet can start inside a long image URL. Drop the fragment.
            cursor = text.len();
            break;
        };
        let bracket = start + 2 + bracket;
        let alt = text[start + 2..bracket].trim();
        if !alt.is_empty() {
            if !out.chars().next_back().is_some_and(char::is_whitespace) {
                out.push(' ');
            }
            out.push_str(alt);
            if !text[bracket + 1..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                out.push(' ');
            }
        }
        let mut next = bracket + 1;
        if text[next..].starts_with('(') {
            match text[next + 1..].find(')') {
                Some(paren) => next += paren + 2,
                None => next = text.len(),
            }
        }
        cursor = next;
    }
    out.push_str(&text[cursor..]);
    plain_line(&out)
}

/// Show matching tags first so a tag-only hit is visible before the two-tag cap.
fn tags_for_display(tags: &[String], query: &str) -> Vec<String> {
    let terms: Vec<String> = search_terms(query)
        .into_iter()
        .map(|term| term.to_lowercase())
        .collect();
    if terms.is_empty() {
        return tags.to_vec();
    }
    let mut ordered = tags.to_vec();
    ordered.sort_by_key(|tag| {
        let tag = tag.to_lowercase();
        !terms.iter().any(|term| tag.starts_with(term))
    });
    ordered
}

#[component]
fn HighlightedText(text: String, query: String) -> Element {
    let segments = highlight_segments(&text, &query);
    rsx! {
        for (index, segment) in segments.into_iter().enumerate() {
            if segment.marked {
                mark { key: "{index}", class: "search-mark", "{segment.text}" }
            } else {
                span { key: "{index}", "{segment.text}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{highlight_segments, preview_text, tags_for_display};

    fn marked(text: &str, query: &str) -> Vec<String> {
        highlight_segments(text, query)
            .into_iter()
            .filter(|segment| segment.marked)
            .map(|segment| segment.text)
            .collect()
    }

    #[test]
    fn highlights_are_case_insensitive_and_keep_literal_brackets() {
        let segments = highlight_segments("Use [Draft] here, draft twice", "draft");
        let rebuilt: String = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(rebuilt, "Use [Draft] here, draft twice");
        assert_eq!(
            marked("Use [Draft] here, draft twice", "draft").join("|"),
            "Draft|draft"
        );
    }

    #[test]
    fn highlights_start_at_word_boundaries_and_match_prefixes() {
        assert_eq!(marked("quicksilver is quick", "qui").join("|"), "qui|qui");
        assert!(marked("unquick", "qui").is_empty());
        assert_eq!(marked("The Brown Fox", "brown fox").join("|"), "Brown|Fox");
        assert_eq!(
            marked("Café menu, CAFÉ list", "café").join("|"),
            "Café|CAFÉ"
        );
    }

    #[test]
    fn unusual_lowercase_never_slices_inside_a_character() {
        let segments = highlight_segments("İÅ draft", "draft");
        assert!(segments.iter().all(|segment| !segment.marked));
        let rebuilt: String = segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect();
        assert_eq!(rebuilt, "İÅ draft");
    }

    #[test]
    fn previews_never_show_raw_image_markup() {
        assert_eq!(
            preview_text("Look ![sun](attachments/a.webp) here"),
            "Look sun here"
        );
        assert_eq!(preview_text("![image](attachments/a.webp"), "image");
        assert_eq!(preview_text("before ![image] after"), "before image after");
        assert!(!preview_text("a ![image]...").contains("!["));
    }

    #[test]
    fn tag_highlights_move_matching_tags_before_the_cap() {
        let tags = vec![
            "personal".to_string(),
            "reading".to_string(),
            "books".to_string(),
        ];
        assert_eq!(
            tags_for_display(&tags, "boo"),
            vec!["books", "personal", "reading"]
        );
        assert_eq!(tags_for_display(&tags, ""), tags);
    }
}
