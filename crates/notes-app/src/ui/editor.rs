//! The Markdown editor, its toolbar and the rendered view.

use dioxus::html::HasFileData;
use dioxus::prelude::*;
use notes_core::{
    external_url, markup_spans, parse_attachment_url, render_markdown_with_images, MarkupKind,
    MarkupSpan, MAX_MARKUP_SPANS,
};
use notes_proto::{NoteId, NotebookId};
use notes_store::Attachment;

use crate::editor_markup::{self, SegmentKind};
use crate::links::{self, OPEN_URL_JS};
use crate::markdown_ops::Mark;
use crate::media::{attachment_kind, format_size};
use crate::state::{
    edited_label, find_match_count, note_title, tone_class, word_count, AppContext, EditorMode,
    COLORS,
};
use crate::ui::icons::Icon;
use crate::ui::{AttachmentGallery, HistoryPanel};

/// A few pixels of scroll keep the row still; roughly one text line hides it.
const TAG_ROW_SCROLL_THRESHOLD: f64 = 24.0;

/// Notes above this size stay a plain textarea. The overlay only helps when
/// there are a readable number of links, and a very large body would cost a
/// DOM node on every keystroke.
const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;

/// The note, mode and tag count the chip row was last measured on, plus
/// whether the body was scrolled past the top. A mismatch means the row is
/// visible again after a note switch, a mode switch or a tag edit.
type TagRowState = (Option<NoteId>, EditorMode, usize, bool);

/// The note and UTF-16 caret the token chip was last computed for.
type CaretState = (Option<NoteId>, Option<usize>);

/// Keep the highlight mirror exactly over the textarea.
///
/// The mirror is only painted with backgrounds and underlines, so it may sit
/// above the textarea and stay non-interactive. Ctrl/Cmd+click compares the
/// pointer with each token's rectangle in the click handler. The listener is
/// installed once for the page; remounting the editor only refreshes the
/// element references.
const EDITOR_SYNC_SCRIPT: &str = r#"
(() => {
  const syncNow = () => {
    const editor = document.getElementById('note-editor');
    const mirror = document.querySelector('.editor-highlights');
    const inner = mirror?.querySelector('.editor-highlights-inner');
    if (!editor || !mirror || !inner) return;
    mirror.style.left = editor.offsetLeft + 'px';
    mirror.style.top = editor.offsetTop + 'px';
    mirror.style.width = editor.clientWidth + 'px';
    mirror.style.height = editor.clientHeight + 'px';
    inner.style.transform =
      'translate(' + (-editor.scrollLeft) + 'px,' + (-editor.scrollTop) + 'px)';
    mirror.style.visibility = 'visible';
    if (!editor.__noteboxMirrorObserver && 'ResizeObserver' in window) {
      const observer = new ResizeObserver(() => {
        if (!editor.isConnected) {
          observer.disconnect();
          return;
        }
        schedule();
      });
      editor.__noteboxMirrorObserver = observer;
      observer.observe(editor);
    }
    // Centering the reading column ties the mirror to the pane's left edge as
    // well as the textarea's size. Collapsing the sidebar moves the column
    // without resizing it, so watch the offset parent too.
    if (!editor.__noteboxMirrorPaneObserver && 'ResizeObserver' in window) {
      const paneObserver = new ResizeObserver(() => {
        if (!editor.isConnected) {
          paneObserver.disconnect();
          return;
        }
        schedule();
      });
      editor.__noteboxMirrorPaneObserver = paneObserver;
      if (editor.offsetParent) paneObserver.observe(editor.offsetParent);
    }
  };
  const schedule = () => {
    if (window.__noteboxSyncFrame) return;
    window.__noteboxSyncFrame = requestAnimationFrame(() => {
      window.__noteboxSyncFrame = 0;
      syncNow();
    });
  };
  window.__noteboxSyncEditor = schedule;
  if (!window.__noteboxEditorSyncBound) {
    window.__noteboxEditorSyncBound = true;
    document.addEventListener('scroll', (event) => {
      if (event.target?.id === 'note-editor') syncNow();
    }, true);
    document.addEventListener('input', (event) => {
      if (event.target?.id === 'note-editor') schedule();
    }, true);
    window.addEventListener('resize', schedule);
    window.visualViewport?.addEventListener('resize', schedule);
    window.addEventListener('orientationchange', schedule);
  }
  syncNow();
})();
"#;

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
    let gallery_open = state.gallery_open;
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
    let caret = use_signal(|| (None, None) as CaretState);

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

    let candidate_spans = if mode == EditorMode::Write && draft.len() <= MAX_HIGHLIGHT_BYTES {
        markup_spans(&draft)
    } else {
        Vec::new()
    };
    // A truncated scan would underline only part of a link-dense note.
    let spans = if candidate_spans.len() < MAX_MARKUP_SPANS {
        candidate_spans
    } else {
        Vec::new()
    };
    let token_actions = token_actions_json(&draft, &spans);
    let segments = editor_markup::segments(&draft, &spans);
    let (caret_note, caret_offset) = *caret.read();
    let active_index = if mode == EditorMode::Write && caret_note == Some(id) {
        caret_offset.and_then(|offset| editor_markup::token_at(&draft, &spans, offset))
    } else {
        None
    };
    let active_span = active_index.and_then(|index| spans.get(index)).cloned();
    let active_markup = active_span
        .as_ref()
        .and_then(|span| active_markup_for(span, &attachments));
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
                        onpick: {
                            let context = context.clone();
                            move |_| context.pick_image()
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
                    if !spans.is_empty() {
                        div {
                            class: "editor-highlights",
                            aria_hidden: "true",
                            onmounted: move |_| {
                                let _ = document::eval(EDITOR_SYNC_SCRIPT);
                            },
                            pre { class: "editor-highlights-inner editor-text",
                                for segment in segments.iter() {
                                    if segment.span.is_some() {
                                        span {
                                            key: "{segment.start}",
                                            class: segment_class(segment.kind),
                                            "{&draft[segment.start..segment.end]}"
                                        }
                                    } else {
                                        "{&draft[segment.start..segment.end]}"
                                    }
                                }
                            }
                        }
                    }
                    textarea {
                        id: "note-editor",
                        class: "note-editor editor-text",
                        aria_label: "Note body. The first line is the title.",
                        spellcheck: "true",
                        placeholder: "Start writing. The first line becomes the title.",
                        value: "{draft}",
                        onmounted: {
                            let mut caret = caret;
                            move |_| {
                                caret.set((Some(id), None));
                                crate::perf::editor_ready();
                                let _ = document::eval(EDITOR_SYNC_SCRIPT);
                            }
                        },
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
                        onclick: {
                            let context = context.clone();
                            let spans = spans.clone();
                            let token_actions = token_actions.clone();
                            let note = id;
                            move |event| {
                                remember_caret(caret, note);
                                let modifiers = event.modifiers();
                                if !(modifiers.contains(Modifiers::CONTROL)
                                    || modifiers.contains(Modifiers::META))
                                {
                                    return;
                                }
                                event.prevent_default();
                                let point = event.client_coordinates();
                                let script = format!(
                                    "const editor = document.getElementById('note-editor');\
                                     const mirror = document.querySelector('.editor-highlights');\
                                     if (!editor || !mirror) return null;\
                                     const x = {}, y = {};\
                                     const ranges = {};\
                                     const tokens = Array.from(mirror.querySelectorAll('.md-token'));\
                                     const token = tokens.find((el) => {{\
                                       const rect = el.getBoundingClientRect();\
                                       return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;\
                                     }});\
                                     if (!token) return null;\
                                     const index = tokens.indexOf(token);\
                                     const range = ranges[index];\
                                     if (!range) return null;\
                                     const url = range[2];\
                                     if (url) {{\
                                       {OPEN_URL_JS}\
                                       return null;\
                                     }}\
                                     return [index, editor.selectionStart];",
                                    point.x, point.y, token_actions
                                );
                                let eval = document::eval(&script);
                                let context = context.clone();
                                let spans = spans.clone();
                                spawn(async move {
                                    if let Ok(Some((index, caret))) =
                                        eval.join::<Option<(usize, usize)>>().await
                                    {
                                        if context.state.peek().selected != Some(note) {
                                            return;
                                        }
                                        if let Some(span) = spans.get(index) {
                                            open_markup_span(&context, span, Some(caret));
                                        }
                                    }
                                });
                            }
                        },
                        onkeyup: move |_| remember_caret(caret, id),
                        onselect: move |_| remember_caret(caret, id),
                        onkeydown: {
                            let context = context.clone();
                            let spans = spans.clone();
                            let token_actions = token_actions.clone();
                            move |event| {
                                let command = event.modifiers().contains(Modifiers::CONTROL)
                                    || event.modifiers().contains(Modifiers::META);
                                if command && event.key() == Key::Enter {
                                    event.prevent_default();
                                    open_token_at_caret(
                                        context.clone(),
                                        id,
                                        spans.clone(),
                                        token_actions.clone(),
                                    );
                                } else if event.key() == Key::Escape
                                    && context.state.peek().find_open
                                {
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
                        onclick: {
                            let context = context.clone();
                            move |event| {
                                let point = event.client_coordinates();
                                let script = format!(
                                    "const el = document.elementFromPoint({}, {});\
                                     return el?.closest('[data-attachment-hash]')?.getAttribute('data-attachment-hash') ?? null;",
                                    point.x, point.y
                                );
                                let eval = document::eval(&script);
                                let context = context.clone();
                                spawn(async move {
                                    if let Ok(Some(hash)) = eval.join::<Option<String>>().await {
                                        context.open_attachment_gallery(Some(hash));
                                    }
                                });
                            }
                        },
                        onkeydown: {
                            let context = context.clone();
                            move |event| {
                                let key = event.key();
                                let space = matches!(&key, Key::Character(ch) if ch == " ");
                                if key != Key::Enter && !space {
                                    return;
                                }
                                // The rendered image carries the hash; links do
                                // not. Enter on a link keeps its normal action,
                                // while Space on the focused image opens it.
                                if space {
                                    event.prevent_default();
                                }
                                let eval = document::eval(
                                    "const el = document.activeElement;\
                                     return el?.closest('[data-attachment-hash]')?.getAttribute('data-attachment-hash') ?? null;",
                                );
                                let context = context.clone();
                                spawn(async move {
                                    if let Ok(Some(hash)) = eval.join::<Option<String>>().await {
                                        context.open_attachment_gallery(Some(hash));
                                    }
                                });
                            }
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
                            id: "attachment-gallery-button",
                            class: "text-button attachment-browse",
                            r#type: "button",
                            onclick: {
                                let context = context.clone();
                                move |_| context.open_attachment_gallery(None)
                            },
                            Icon { name: "grid" }
                            span { "Browse all" }
                        }
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
                if let Some(active) = active_markup.clone() {
                    button {
                        id: "token-open",
                        class: "status-token",
                        r#type: "button",
                        aria_label: "{active.aria}",
                        title: "{active.title}",
                        onclick: {
                            let context = context.clone();
                            let action = active.action.clone();
                            move |event| {
                                event.stop_propagation();
                                match action.clone() {
                                    MarkupAction::Link(url) => {
                                        if !links::open_external(&url) {
                                            context.toast_info("That link has no web address to open");
                                        }
                                    }
                                    MarkupAction::Image(hash) => {
                                        context.open_editor_attachment(hash, caret_offset)
                                    }
                                }
                            }
                        },
                        Icon { name: active.icon }
                        span { class: "status-token-label", "{active.label}" }
                        span { class: "status-token-detail", "{active.detail}" }
                    }
                }
                if !attachments.is_empty() {
                    button {
                        id: "status-attachments-button",
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

            if gallery_open {
                AttachmentGallery {}
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

/// One token action offered in the editor's status bar.
#[derive(Clone)]
struct ActiveMarkup {
    action: MarkupAction,
    label: &'static str,
    detail: String,
    aria: String,
    title: String,
    icon: &'static str,
}

/// What tapping the status-bar chip does.
#[derive(Clone)]
enum MarkupAction {
    /// Open a web or email address outside the app.
    Link(String),
    /// Open one of the note's images in the viewer.
    Image(String),
}

/// The status-bar action for the token under the caret, when it has one.
fn active_markup_for(span: &MarkupSpan, attachments: &[Attachment]) -> Option<ActiveMarkup> {
    match span.kind {
        MarkupKind::Image => {
            if let Some(hash) = span.attachment_hash.as_deref() {
                let name = attachments
                    .iter()
                    .find(|attachment| attachment.hash == hash)
                    .map(|attachment| attachment.name.clone())
                    .unwrap_or_else(|| "image".to_string());
                let detail = editor_markup::shorten(&name, 24);
                return Some(ActiveMarkup {
                    action: MarkupAction::Image(hash.to_string()),
                    label: "View image",
                    aria: format!("View image: {detail}"),
                    title: "View the full image (Ctrl+Enter)".to_string(),
                    icon: "image",
                    detail,
                });
            }
            let url = external_url(&span.destination)?;
            let detail = link_detail(&url);
            Some(ActiveMarkup {
                action: MarkupAction::Link(url),
                label: "Open image",
                aria: format!("Open image in your browser: {detail}"),
                title: "Open the image in your browser (Ctrl+Enter)".to_string(),
                icon: "image",
                detail,
            })
        }
        MarkupKind::Link => {
            let url = external_url(&span.destination)?;
            let detail = link_detail(&url);
            Some(ActiveMarkup {
                action: MarkupAction::Link(url),
                label: "Open link",
                aria: format!("Open link in your browser: {detail}"),
                title: "Open the link in your browser (Ctrl+Enter)".to_string(),
                icon: "link",
                detail,
            })
        }
    }
}

/// Host or address shown next to an open action, kept short.
fn link_detail(url: &str) -> String {
    editor_markup::link_host(url)
        .map(|host| editor_markup::shorten(&host, 28))
        .unwrap_or_else(|| editor_markup::shorten(url, 32))
}

/// Open a token's destination: an attachment image or an external address.
fn open_markup_span(context: &AppContext, span: &MarkupSpan, caret: Option<usize>) {
    match span.kind {
        MarkupKind::Image => {
            if let Some(hash) = span.attachment_hash.as_deref() {
                context.open_editor_attachment(hash.to_string(), caret);
                return;
            }
            if let Some(url) = external_url(&span.destination) {
                if links::open_external(&url) {
                    return;
                }
            }
            context.toast_info("That image cannot be opened");
        }
        MarkupKind::Link => {
            if let Some(url) = external_url(&span.destination) {
                if links::open_external(&url) {
                    return;
                }
            }
            context.toast_info("That link has no web address to open");
        }
    }
}

/// Open the token at the caret, for Ctrl+Enter.
///
/// The hit test runs in one synchronous script so a web link keeps the
/// keyboard gesture. Opening a tab after an async round trip would be blocked
/// as a popup. Images return their index and open the viewer from Rust.
fn open_token_at_caret(
    context: AppContext,
    note: NoteId,
    spans: Vec<MarkupSpan>,
    token_actions: String,
) {
    let script = format!(
        "const editor = document.getElementById('note-editor');\
         if (!editor || document.activeElement !== editor) return null;\
         const caret = editor.selectionStart;\
         const ranges = {token_actions};\
         const index = ranges.findIndex((range) => caret >= range[0] && caret <= range[1]);\
         if (index < 0) return null;\
         const url = ranges[index][2];\
         if (url) {{\
           {OPEN_URL_JS}\
           return null;\
         }}\
         return [index, caret];"
    );
    let eval = document::eval(&script);
    spawn(async move {
        if let Ok(Some((index, caret))) = eval.join::<Option<(usize, usize)>>().await {
            if context.state.peek().selected != Some(note) {
                return;
            }
            if let Some(span) = spans.get(index) {
                open_markup_span(&context, span, Some(caret));
            }
        }
    });
}

/// UTF-16 token ranges with their sanitized external URL, for the scripts.
///
/// The editor's caret is a UTF-16 offset but the scanner returns byte ranges.
/// One pass keeps this linear even for a note near the highlight size cap.
fn token_actions_json(text: &str, spans: &[MarkupSpan]) -> String {
    if spans.is_empty() {
        return "[]".to_string();
    }
    let mut entries: Vec<(usize, usize, Option<String>)> = Vec::with_capacity(spans.len());
    let mut byte_cursor = 0;
    let mut utf16_cursor = 0;
    for span in spans {
        if span.start < byte_cursor || span.end > text.len() || span.start >= span.end {
            // The scanner never returns this, but keep indices aligned with
            // the rendered token list if that ever changes.
            entries.push((0, 0, None));
            continue;
        }
        utf16_cursor += text[byte_cursor..span.start].encode_utf16().count();
        let start = utf16_cursor;
        utf16_cursor += text[span.start..span.end].encode_utf16().count();
        let end = utf16_cursor;
        byte_cursor = span.end;
        let url = match span.kind {
            MarkupKind::Link => external_url(&span.destination),
            MarkupKind::Image if span.attachment_hash.is_none() => external_url(&span.destination),
            MarkupKind::Image => None,
        };
        entries.push((start, end, url));
    }
    serde_json::to_string(&entries).unwrap_or_else(|_| "[]".to_string())
}

/// Remember which token the caret sits in, for the status-bar chip.
fn remember_caret(mut caret: Signal<CaretState>, note: NoteId) {
    let eval = document::eval(
        "const editor = document.getElementById('note-editor');\
         return editor && document.activeElement === editor ? editor.selectionStart : null;",
    );
    spawn(async move {
        if let Ok(Some(offset)) = eval.join::<Option<usize>>().await {
            let next = (Some(note), Some(offset));
            if *caret.peek() != next {
                caret.set(next);
            }
        }
    });
}

/// Class that paints one overlay token.
fn segment_class(kind: SegmentKind) -> &'static str {
    match kind {
        SegmentKind::Link => "md-token md-link-token",
        SegmentKind::Image => "md-token md-image-token",
        SegmentKind::Text => "",
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
