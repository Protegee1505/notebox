//! The Markdown editor, its toolbar and the rendered view.

use dioxus::html::HasFileData;
use dioxus::prelude::*;
use notes_core::{parse_attachment_url, render_markdown_with_images};
use notes_proto::{NoteId, NotebookId};

use crate::markdown_ops::Mark;
use crate::media::{attachment_kind, format_size};
use crate::state::{
    edited_label, find_match_count, note_title, tone_class, word_count, AppContext, EditorMode,
    COLORS,
};
use crate::ui::icons::Icon;
use crate::ui::HistoryPanel;

/// A few pixels of scroll keep the row still; roughly one text line hides it.
const TAG_ROW_SCROLL_THRESHOLD: f64 = 24.0;

/// The note, mode and tag count the chip row was last measured on, plus
/// whether the body was scrolled past the top. A mismatch means the row is
/// visible again after a note switch, a mode switch or a tag edit.
type TagRowState = (Option<NoteId>, EditorMode, usize, bool);

/// Clipboard images cannot travel through Dioxus's paste event, so a page
/// listener hands them to the hidden file input instead. A single listener is
/// installed for the lifetime of the page; the app can unmount and remount the
/// editor without stacking handlers.
const PASTE_SCRIPT: &str = r#"
if (!window.__noteboxPasteBound) {
  window.__noteboxPasteBound = true;
  document.addEventListener('paste', (event) => {
    const target = event.target;
    if (!target || target.id !== 'note-editor') return;
    const files = Array.from(event.clipboardData?.files || [])
      .filter((file) => file.type.startsWith('image/'));
    if (files.length === 0) return;
    event.preventDefault();
    const transfer = new DataTransfer();
    for (const file of files) transfer.items.add(file);
    const input = document.getElementById('note-image-input');
    if (!input) return;
    input.files = transfer.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
}
"#;

#[component]
pub fn EditorPane() -> Element {
    let context = use_context::<AppContext>();
    use_effect(move || {
        let _ = document::eval(PASTE_SCRIPT);
    });
    let mut tag_draft = use_signal(String::new);
    let state = context.state.read();
    let selected = state.selected;
    let draft = state.draft.clone();
    let dirty = state.dirty;
    let saved_at = state.saved_at;
    let mode = state.editor_mode;
    let find_open = state.find_open;
    let find_query = state.find_query.clone();
    let find_index = state.find_index;
    let color_open = state.color_open;
    let history_open = state.history_open;
    let notebooks = state.notebooks.clone();
    let attachments = state.attachments.clone();
    let attachments_open = state.attachments_open;
    let attaching = state.attaching;
    let summary = selected.and_then(|id| state.notes.iter().find(|note| note.id == id).cloned());
    let pinned = summary.as_ref().map(|note| note.pinned).unwrap_or(false);
    let archived = summary.as_ref().map(|note| note.archived).unwrap_or(false);
    let trashed = summary
        .as_ref()
        .map(|note| note.trashed_at.is_some())
        .unwrap_or(false);
    let tags = summary
        .as_ref()
        .map(|note| note.tags.clone())
        .unwrap_or_default();
    let current_notebook = summary
        .as_ref()
        .and_then(|note| note.notebook_id)
        .map(|id| id.to_string())
        .unwrap_or_default();
    let note_color = summary
        .as_ref()
        .and_then(|note| note.color.clone())
        .unwrap_or_default();
    // The chip row folds away while the note body scrolls, then returns at the
    // top. Keying the state to the note, mode and tag count means a switch or
    // a tag edit shows the chips again without a reset effect.
    let tag_row = use_signal(|| (selected, mode, 0usize, false) as TagRowState);

    let Some(id) = selected else {
        return rsx! {
            main { class: "editor-pane is-empty", aria_label: "Editor",
                div { class: "editor-empty",
                    div { class: "empty-mark", Icon { name: "note" } }
                    h2 { "Your notes, in one place" }
                    p {
                        "Pick a note on the left, or start a new one with "
                        kbd { class: "key-hint", "Ctrl N" }
                        "."
                    }
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
                    div { class: "shortcut-strip",
                        span { kbd { class: "key-hint", "Ctrl K" } " Search" }
                        span { kbd { class: "key-hint", "Ctrl P" } " Palette" }
                        span { kbd { class: "key-hint", "Ctrl F" } " Find" }
                        span { kbd { class: "key-hint", "Ctrl E" } " Preview" }
                    }
                }
            }
        };
    };

    let preview_html = if mode == EditorMode::Preview {
        let urls = &state.attachment_urls;
        render_markdown_with_images(&draft, &|url| {
            parse_attachment_url(url).and_then(|parsed| urls.get(parsed.hash).cloned())
        })
    } else {
        String::new()
    };
    let pending_delete = state.pending_delete == Some(id);
    drop(state);

    let tag_count = tags.len();
    let (collapsed_note, collapsed_mode, collapsed_tags, collapsed) = *tag_row.read();
    let tags_collapsed = collapsed
        && collapsed_note == Some(id)
        && collapsed_mode == mode
        && collapsed_tags == tag_count;
    let words = word_count(&draft);
    let characters = draft.chars().count();
    let matches = find_match_count(&draft, &find_query);
    let title = summary
        .as_ref()
        .map(note_title)
        .unwrap_or_else(|| "Untitled note".to_string());
    let save_label = if dirty {
        "Unsaved".to_string()
    } else {
        edited_label(saved_at)
    };
    let tone = tone_class(if note_color.is_empty() {
        None
    } else {
        Some(note_color.as_str())
    });
    let (mode_icon, mode_label) = if mode == EditorMode::Preview {
        ("edit", "Write")
    } else {
        ("eye", "Preview")
    };
    let working_label = if attaching == 1 {
        "Compressing 1 image\u{2026}".to_string()
    } else {
        format!("Compressing {attaching} images\u{2026}")
    };

    rsx! {
        main { class: "editor-pane {tone}", aria_label: "Editor",
            ondragover: move |event| event.prevent_default(),
            ondrop: {
                let context = context.clone();
                move |event| {
                    event.prevent_default();
                    let files = event.files();
                    if !files.is_empty() {
                        context.attach_files(files);
                    }
                }
            },
            input {
                id: "note-image-input",
                class: "visually-hidden",
                r#type: "file",
                accept: "image/png,image/jpeg,image/gif,image/webp",
                multiple: true,
                tabindex: "-1",
                aria_label: "Image attachments",
                onchange: {
                    let context = context.clone();
                    move |event| {
                        let files = event.files();
                        context.attach_files(files);
                        let _ = document::eval(
                            "const el = document.getElementById('note-image-input');\
                             if (el) el.value = '';",
                        );
                    }
                },
            }
            header { class: "editor-header",
                button {
                    class: "icon-button back-button",
                    r#type: "button",
                    aria_label: "Back to notes",
                    title: "Back to notes",
                    onclick: {
                        let context = context.clone();
                        move |_| context.back_to_notes()
                    },
                    Icon { name: "arrow-left" }
                }
                div { class: "editor-where",
                    select {
                        class: "notebook-select",
                        aria_label: "Notebook",
                        value: "{current_notebook}",
                        onchange: {
                            let context = context.clone();
                            move |event| {
                                let value = event.value();
                                let id = if value.is_empty() { None } else { NotebookId::parse(&value).ok() };
                                context.set_notebook(id);
                            }
                        },
                        option { value: "", "No notebook" }
                        for notebook in notebooks {
                            option { key: "{notebook.id}", value: "{notebook.id}", "{notebook.name}" }
                        }
                    }
                    span { class: "save-state", title: "{save_label}",
                        if dirty {
                            span { class: "save-dot is-dirty", aria_hidden: "true" }
                            span { class: "save-label", "{save_label}" }
                        } else {
                            span { class: "save-dot", aria_hidden: "true" }
                            span { class: "save-label", "{save_label}" }
                        }
                    }
                }
                div { class: "editor-actions",
                    button {
                        class: if pinned { "icon-button is-active" } else { "icon-button" },
                        r#type: "button",
                        aria_label: if pinned { "Unpin note" } else { "Pin note" },
                        aria_pressed: if pinned { "true" } else { "false" },
                        title: if pinned { "Unpin note" } else { "Pin note" },
                        onclick: {
                            let context = context.clone();
                            move |_| context.toggle_pin()
                        },
                        Icon { name: "pin" }
                    }
                    button {
                        class: if archived { "icon-button is-active" } else { "icon-button" },
                        r#type: "button",
                        aria_label: if archived { "Unarchive note" } else { "Archive note" },
                        aria_pressed: if archived { "true" } else { "false" },
                        title: if archived { "Unarchive" } else { "Archive" },
                        onclick: {
                            let context = context.clone();
                            move |_| context.toggle_archive()
                        },
                        Icon { name: "archive" }
                    }
                    button {
                        class: if history_open { "icon-button is-active" } else { "icon-button" },
                        r#type: "button",
                        aria_label: "Note history",
                        aria_pressed: if history_open { "true" } else { "false" },
                        title: "History",
                        onclick: {
                            let context = context.clone();
                            move |_| {
                                if context.state.peek().history_open {
                                    context.close_history();
                                } else {
                                    context.open_history();
                                }
                            }
                        },
                        Icon { name: "clock" }
                    }
                    div { class: "menu-anchor",
                        button {
                            class: if color_open { "icon-button is-active" } else { "icon-button" },
                            r#type: "button",
                            aria_label: "Note colour",
                            title: "Note colour",
                            onclick: {
                                let context = context.clone();
                                move |event| {
                                    event.stop_propagation();
                                    let open = context.state.peek().color_open;
                                    context.state.write_unchecked().color_open = !open;
                                }
                            },
                            Icon { name: "palette" }
                        }
                        if color_open {
                            div {
                                class: "color-popover",
                                role: "menu",
                                aria_label: "Note colour",
                                onclick: move |event| event.stop_propagation(),
                                for (value, label) in COLORS {
                                    button {
                                        key: "{value}",
                                        class: if note_color == value { "color-swatch is-active" } else { "color-swatch" },
                                        r#type: "button",
                                        role: "menuitem",
                                        title: "{label}",
                                        aria_label: "{label}",
                                        onclick: {
                                            let context = context.clone();
                                            move |_| context.set_color(Some(value))
                                        },
                                        span { class: "swatch-dot tone-{value}" }
                                    }
                                }
                                button {
                                    class: "color-clear",
                                    r#type: "button",
                                    role: "menuitem",
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.set_color(None)
                                    },
                                    "No colour"
                                }
                            }
                        }
                    }
                    if trashed {
                        button {
                            class: "secondary-button editor-restore",
                            r#type: "button",
                            aria_label: "Restore note",
                            title: "Restore note",
                            onclick: {
                                let context = context.clone();
                                move |_| context.restore_note(id)
                            },
                            Icon { name: "undo" }
                            span { "Restore" }
                        }
                    } else {
                        button {
                            id: "trash-note",
                            class: "icon-button danger-hover",
                            r#type: "button",
                            aria_label: "Move note to trash",
                            aria_expanded: if pending_delete { "true" } else { "false" },
                            aria_controls: "delete-confirm",
                            title: "Move to trash",
                            onclick: {
                                let context = context.clone();
                                move |_| context.trash_selected()
                            },
                            Icon { name: "trash" }
                        }
                    }
                }
            }

            if pending_delete {
                div {
                    id: "delete-confirm",
                    class: "delete-confirm",
                    role: "alert",
                    p { "Move this note to the trash? You can restore it later." }
                    div { class: "delete-confirm-actions",
                        button {
                            id: "confirm-trash-cancel",
                            class: "secondary-button",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.cancel_trash()
                            },
                            "Cancel"
                        }
                        button {
                            class: "danger-button",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.confirm_trash()
                            },
                            "Move to trash"
                        }
                    }
                }
            }

            if find_open {
                div { class: "find-bar", role: "search",
                    Icon { name: "search" }
                    input {
                        id: "find-input",
                        class: "find-input",
                        r#type: "text",
                        placeholder: "Find in note",
                        aria_label: "Find in note",
                        autofocus: true,
                        value: "{find_query}",
                        oninput: {
                            let context = context.clone();
                            move |event| context.set_find_query(event.value())
                        },
                        onkeydown: {
                            let context = context.clone();
                            move |event| {
                                if event.key() == Key::Enter {
                                    event.prevent_default();
                                    let direction = if event.modifiers().contains(Modifiers::SHIFT) { -1 } else { 1 };
                                    context.find_next(direction);
                                } else if event.key() == Key::Escape {
                                    context.toggle_find();
                                }
                            }
                        },
                    }
                    span { class: "find-count",
                        if find_query.trim().is_empty() {
                            ""
                        } else if matches == 0 {
                            { "No matches".to_string() }
                        } else {
                            { format!("{} / {}", find_index.min(matches.saturating_sub(1)) + 1, matches) }
                        }
                    }
                    button {
                        class: "icon-button small",
                        r#type: "button",
                        aria_label: "Previous match",
                        title: "Previous",
                        onclick: {
                            let context = context.clone();
                            move |_| context.find_next(-1)
                        },
                        Icon { name: "chevron-down" }
                    }
                    button {
                        class: "icon-button small rotate-180",
                        r#type: "button",
                        aria_label: "Next match",
                        title: "Next",
                        onclick: {
                            let context = context.clone();
                            move |_| context.find_next(1)
                        },
                        Icon { name: "chevron-down" }
                    }
                    button {
                        class: "icon-button small",
                        r#type: "button",
                        aria_label: "Close find",
                        title: "Close",
                        onclick: {
                            let context = context.clone();
                            move |_| context.toggle_find()
                        },
                        Icon { name: "x" }
                    }
                }
            }

            div { class: "editor-toolbar", role: "toolbar", aria_label: "Formatting",
                div { class: "tool-group",
                    ToolButton {
                        icon: "undo",
                        label: "Undo",
                        onpick: {
                            let context = context.clone();
                            move |_| context.undo()
                        },
                    }
                    ToolButton {
                        icon: "redo",
                        label: "Redo",
                        onpick: {
                            let context = context.clone();
                            move |_| context.redo()
                        },
                    }
                }
                span { class: "tool-divider" }
                div { class: "tool-group",
                    MarkButton { icon: "bold", label: "Bold", mark: Mark::Bold }
                    MarkButton { icon: "italic", label: "Italic", mark: Mark::Italic }
                    MarkButton { icon: "strike", label: "Strikethrough", mark: Mark::Strike }
                }
                span { class: "tool-divider" }
                div { class: "tool-group",
                    MarkButton { icon: "heading", label: "Heading", mark: Mark::Heading }
                    MarkButton { icon: "checklist", label: "Checklist", mark: Mark::Checklist }
                    MarkButton { icon: "bullet", label: "Bulleted list", mark: Mark::Bullet }
                    MarkButton { icon: "numbered", label: "Numbered list", mark: Mark::Numbered }
                    MarkButton { icon: "quote", label: "Quote", mark: Mark::Quote }
                }
                span { class: "tool-divider" }
                div { class: "tool-group",
                    MarkButton { icon: "code", label: "Code", mark: Mark::Code }
                    MarkButton { icon: "code-block", label: "Code block", mark: Mark::CodeBlock }
                    MarkButton { icon: "link", label: "Link", mark: Mark::Link }
                    ToolButton {
                        icon: "image",
                        label: "Add image",
                        onpick: move |_| {
                            let _ = document::eval(
                                "document.getElementById('note-image-input')?.click();",
                            );
                        },
                    }
                }
                div { class: "toolbar-spacer" }
                div { class: "tool-group",
                    ToolButton {
                        icon: "search",
                        label: "Find in note",
                        onpick: {
                            let context = context.clone();
                            move |_| context.toggle_find()
                        },
                    }
                    ToolButton {
                        icon: mode_icon,
                        label: mode_label,
                        active: mode == EditorMode::Preview,
                        onpick: {
                            let context = context.clone();
                            move |_| context.toggle_preview()
                        },
                    }
                }
            }

            if !tags.is_empty() {
                div {
                    class: if tags_collapsed { "editor-tags is-collapsed" } else { "editor-tags" },
                    div { class: "editor-tags-inner",
                        for tag in tags {
                            span { key: "{tag}", class: "editor-tag",
                                "#{tag}"
                                button {
                                    class: "tag-remove",
                                    r#type: "button",
                                    aria_label: "Remove tag {tag}",
                                    onclick: {
                                        let context = context.clone();
                                        let tag = tag.clone();
                                        move |_| context.remove_tag(tag.clone())
                                    },
                                    Icon { name: "x" }
                                }
                            }
                        }
                    }
                }
            }

            div { class: "editor-body",
                if mode == EditorMode::Write {
                    textarea {
                        id: "note-editor",
                        class: "note-editor",
                        aria_label: "Note body. The first line is the title.",
                        spellcheck: "true",
                        placeholder: "Start writing. The first line becomes the title.",
                        value: "{draft}",
                        onmounted: move |_| crate::perf::editor_ready(),
                        oninput: {
                            let context = context.clone();
                            move |event| context.note_input(event.value())
                        },
                        onscroll: move |event| {
                            set_tag_row_scroll(
                                tag_row,
                                id,
                                mode,
                                tag_count,
                                event.scroll_top(),
                            );
                        },
                        onblur: {
                            let context = context.clone();
                            move |_| context.save_now()
                        },
                        onkeydown: {
                            let context = context.clone();
                            move |event| {
                                if event.key() == Key::Escape && context.state.peek().find_open {
                                    context.toggle_find();
                                }
                            }
                        },
                    }
                } else {
                    article {
                        class: "editor-preview markdown-body",
                        aria_label: "Rendered note",
                        dangerous_inner_html: "{preview_html}",
                        onscroll: move |event| {
                            set_tag_row_scroll(
                                tag_row,
                                id,
                                mode,
                                tag_count,
                                event.scroll_top(),
                            );
                        },
                    }
                }
            }

            if attaching > 0 {
                div { class: "attachment-working", role: "status",
                    span { class: "save-dot is-dirty" }
                    "{working_label}"
                }
            }

            if attachments_open && !attachments.is_empty() {
                div {
                    id: "attachments-panel",
                    class: "attachment-strip",
                    role: "region",
                    aria_label: "Attachments",
                    div { class: "attachment-strip-head",
                        span { class: "attachment-strip-title", "Attachments" }
                        span { class: "attachment-strip-count", "{attachments.len()}" }
                        button {
                            class: "icon-button small attachment-strip-close",
                            r#type: "button",
                            aria_label: "Close attachments",
                            title: "Close attachments",
                            onclick: {
                                let context = context.clone();
                                move |_| context.toggle_attachments()
                            },
                            Icon { name: "x" }
                        }
                    }
                    div { class: "attachment-rows",
                        for attachment in attachments.iter().take(12) {
                            div { key: "{attachment.hash}", class: "attachment-row",
                                span { class: "attachment-icon", Icon { name: "image" } }
                                div { class: "attachment-copy",
                                    span { class: "attachment-name", "{attachment.name}" }
                                    span { class: "attachment-detail",
                                        "{attachment_kind(&attachment.mime)} \u{00b7} {format_size(attachment.size)}"
                                        if !attachment.available {
                                            " \u{00b7} not downloaded"
                                        }
                                    }
                                }
                                if attachment.available {
                                    button {
                                        class: "attachment-insert",
                                        r#type: "button",
                                        title: "Insert this image at the caret",
                                        aria_label: "Insert {attachment.name}",
                                        onclick: {
                                            let context = context.clone();
                                            let attachment = attachment.clone();
                                            move |_| context.insert_attachment(attachment.clone())
                                        },
                                        "Insert"
                                    }
                                } else {
                                    span { class: "attachment-waiting", "Waiting" }
                                }
                            }
                        }
                    }
                    if attachments.len() > 12 {
                        span { class: "attachment-more",
                            "+{attachments.len() - 12} more"
                        }
                    }
                }
            }

            footer { class: "editor-status",
                div { class: "tag-entry",
                    Icon { name: "tag" }
                    input {
                        class: "tag-input",
                        r#type: "text",
                        placeholder: "Add tag",
                        aria_label: "Add a tag",
                        value: "{tag_draft}",
                        oninput: move |event| tag_draft.set(event.value()),
                        onkeydown: {
                            let context = context.clone();
                            move |event| {
                                if event.key() == Key::Enter {
                                    event.prevent_default();
                                    let tag = tag_draft.read().clone();
                                    tag_draft.set(String::new());
                                    context.add_tag(tag);
                                }
                            }
                        },
                    }
                    button {
                        class: "icon-button small",
                        r#type: "button",
                        aria_label: "Add tag",
                        title: "Add tag",
                        disabled: tag_draft.read().trim().is_empty(),
                        onclick: {
                            let context = context.clone();
                            move |_| {
                                let tag = tag_draft.read().clone();
                                tag_draft.set(String::new());
                                context.add_tag(tag);
                            }
                        },
                        Icon { name: "check" }
                    }
                }
                if !attachments.is_empty() {
                    button {
                        class: if attachments_open { "status-attachments is-active" } else { "status-attachments" },
                        r#type: "button",
                        aria_label: "Attachments",
                        aria_expanded: if attachments_open { "true" } else { "false" },
                        aria_controls: "attachments-panel",
                        title: if attachments_open { "Hide attachments" } else { "Show attachments" },
                        onclick: {
                            let context = context.clone();
                            move |_| context.toggle_attachments()
                        },
                        Icon { name: "image" }
                        span { "{attachments.len()}" }
                        span { class: "status-attachments-label",
                            if attachments.len() == 1 { "image" } else { "images" }
                        }
                    }
                }
                div { class: "status-counts",
                    span { class: "status-words", "{words} words" }
                    span { class: "status-sep status-words-sep", "\u{00b7}" }
                    span { "{characters} characters" }
                    if let Some(note) = summary.as_ref() {
                        span { class: "status-sep status-title-sep", "\u{00b7}" }
                        span { class: "status-title", "{title}" }
                        if note.pinned {
                            span { class: "status-pin", Icon { name: "pin" } }
                        }
                    }
                }
            }

            if history_open {
                HistoryPanel {}
            }
        }
    }
}

/// Collapse the chip row once the note body scrolls, and only for the note,
/// mode and tag list that was measured. Peeking avoids a re-render on every
/// scroll tick; the signal changes only when the row should move.
fn set_tag_row_scroll(
    mut tag_row: Signal<TagRowState>,
    note: NoteId,
    mode: EditorMode,
    tag_count: usize,
    scroll_top: f64,
) {
    let next: TagRowState = (
        Some(note),
        mode,
        tag_count,
        scroll_top > TAG_ROW_SCROLL_THRESHOLD,
    );
    if *tag_row.peek() != next {
        tag_row.set(next);
    }
}

#[component]
fn MarkButton(icon: &'static str, label: &'static str, mark: Mark) -> Element {
    let context = use_context::<AppContext>();
    rsx! {
        ToolButton {
            icon,
            label,
            onpick: move |_| context.apply_mark(mark),
        }
    }
}

#[component]
fn ToolButton(
    icon: &'static str,
    label: &'static str,
    onpick: EventHandler<()>,
    #[props(default = false)] active: bool,
) -> Element {
    rsx! {
        button {
            class: if active { "tool-button is-active" } else { "tool-button" },
            r#type: "button",
            title: "{label}",
            aria_label: "{label}",
            aria_pressed: if active { "true" } else { "false" },
            onmousedown: move |event| event.prevent_default(),
            onclick: move |_| onpick.call(()),
            Icon { name: icon }
        }
    }
}
