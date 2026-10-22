//! Toolbar edits on the Markdown body.
//!
//! Browser selection offsets are UTF-16 code units, while Rust strings are
//! UTF-8 bytes. Every edit converts at the boundary, so an emoji before the
//! cursor cannot slice a character in half.

/// A replacement ready to write back to the text box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// New body.
    pub text: String,
    /// Selection to restore, in UTF-16 code units.
    pub selection: (usize, usize),
}

/// One formatting action from the toolbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// `**text**`
    Bold,
    /// `*text*`
    Italic,
    /// `~~text~~`
    Strike,
    /// `` `text` ``
    Code,
    /// A fenced block, or inline code for a short selection.
    CodeBlock,
    /// `[text](https://)`
    Link,
    /// `# `
    Heading,
    /// `- [ ] `
    Checklist,
    /// `- `
    Bullet,
    /// `1. `
    Numbered,
    /// `> `
    Quote,
}

/// Apply a toolbar action to `text`, with a selection from the text box.
pub fn apply(text: &str, start_utf16: usize, end_utf16: usize, mark: Mark) -> Edit {
    let start = utf16_to_byte(text, start_utf16);
    let end = utf16_to_byte(text, end_utf16);
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let end = end.min(text.len());

    let (new_text, byte_start, byte_end) = match mark {
        Mark::Bold => wrap(text, start, end, "**", "**"),
        Mark::Italic => wrap(text, start, end, "*", "*"),
        Mark::Strike => wrap(text, start, end, "~~", "~~"),
        Mark::Code => wrap(text, start, end, "`", "`"),
        Mark::CodeBlock if text[start..end].contains('\n') => {
            wrap(text, start, end, "```\n", "\n```")
        }
        Mark::CodeBlock => wrap(text, start, end, "`", "`"),
        Mark::Link => link(text, start, end),
        Mark::Heading => line_prefix(text, start, end, "# "),
        Mark::Checklist => line_prefix(text, start, end, "- [ ] "),
        Mark::Bullet => line_prefix(text, start, end, "- "),
        Mark::Numbered => line_prefix(text, start, end, "1. "),
        Mark::Quote => line_prefix(text, start, end, "> "),
    };

    Edit {
        selection: (
            byte_to_utf16(&new_text, byte_start),
            byte_to_utf16(&new_text, byte_end),
        ),
        text: new_text,
    }
}

/// Insert a block at the selection with blank lines around it.
///
/// Attachments read best as their own paragraph. Existing blank lines are
/// reused rather than doubled, and the caret lands after the block.
pub fn insert_block(text: &str, start_utf16: usize, end_utf16: usize, block: &str) -> Edit {
    let start = utf16_to_byte(text, start_utf16);
    let end = utf16_to_byte(text, end_utf16);
    let (start, end) = if start <= end {
        (start, end.min(text.len()))
    } else {
        (end, start)
    };
    let before = &text[..start];
    let after = &text[end..];
    let prefix = if before.is_empty() || before.ends_with("\n\n") {
        ""
    } else if before.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let suffix = if after.is_empty() || after.starts_with("\n\n") {
        ""
    } else if after.starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };

    let mut new_text =
        String::with_capacity(text.len() + block.len() + prefix.len() + suffix.len());
    new_text.push_str(before);
    new_text.push_str(prefix);
    new_text.push_str(block);
    new_text.push_str(suffix);
    let caret = new_text.len();
    new_text.push_str(after);
    let caret = byte_to_utf16(&new_text, caret);
    Edit {
        text: new_text,
        selection: (caret, caret),
    }
}

/// Wrap the selection, keeping it selected inside the markers.
fn wrap(
    text: &str,
    start: usize,
    end: usize,
    prefix: &str,
    suffix: &str,
) -> (String, usize, usize) {
    let mut out = String::with_capacity(text.len() + prefix.len() + suffix.len());
    out.push_str(&text[..start]);
    out.push_str(prefix);
    let inner_start = out.len();
    out.push_str(&text[start..end]);
    let inner_end = out.len();
    out.push_str(suffix);
    out.push_str(&text[end..]);
    (out, inner_start, inner_end)
}

/// Turn the selection into a Markdown link and select the URL to replace.
fn link(text: &str, start: usize, end: usize) -> (String, usize, usize) {
    let selected = &text[start..end];
    let label = if selected.is_empty() {
        "text"
    } else {
        selected
    };
    let mut out = String::with_capacity(text.len() + label.len() + 12);
    out.push_str(&text[..start]);
    out.push('[');
    let label_start = out.len();
    out.push_str(label);
    let label_end = out.len();
    out.push_str("](https://)");
    out.push_str(&text[end..]);

    if selected.is_empty() {
        (out, label_start, label_end)
    } else {
        let url_start = label_end + 2;
        (out, url_start, url_start + "https://".len())
    }
}

/// Add (or toggle off) a marker at the start of every line in the selection.
fn line_prefix(text: &str, start: usize, end: usize, prefix: &str) -> (String, usize, usize) {
    let line_start = text[..start]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let search_end = if end > start { end } else { start };
    let last_start = text[..search_end]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let block_end = text[last_start..]
        .find('\n')
        .map(|index| last_start + index)
        .unwrap_or(text.len());
    let block = &text[line_start..block_end];
    let lines: Vec<&str> = block.split('\n').collect();
    let remove = lines.iter().all(|line| line.starts_with(prefix));

    let mut rewritten = Vec::with_capacity(lines.len());
    for line in &lines {
        if remove {
            rewritten.push(line.strip_prefix(prefix).unwrap_or(line).to_string());
        } else if line.starts_with(prefix) {
            rewritten.push((*line).to_string());
        } else {
            rewritten.push(format!("{prefix}{line}"));
        }
    }
    let new_block = rewritten.join("\n");

    let mut out = String::with_capacity(text.len() + new_block.len());
    out.push_str(&text[..line_start]);
    let block_start = out.len();
    out.push_str(&new_block);
    let block_finish = out.len();
    out.push_str(&text[block_end..]);

    if start == end {
        // Keep the caret in the line where it was, shifted by the marker.
        let offset = start - line_start;
        let prefix_len = prefix.len();
        let new_offset = if remove {
            offset.saturating_sub(prefix_len)
        } else if lines.first().is_some_and(|line| line.starts_with(prefix)) {
            offset
        } else {
            offset + prefix_len
        };
        let caret = block_start + new_offset.min(new_block.len());
        (out, caret, caret)
    } else {
        (out, block_start, block_finish)
    }
}

/// Byte offset in `text` for a UTF-16 code-unit offset.
pub(crate) fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (index, character) in text.char_indices() {
        if units + character.len_utf16() > offset {
            return index;
        }
        units += character.len_utf16();
        if units == offset {
            return index + character.len_utf8();
        }
    }
    text.len()
}

/// UTF-16 code-unit offset for a byte offset.
fn byte_to_utf16(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{apply, insert_block, Mark};

    #[test]
    fn a_block_gets_blank_lines_without_doubling_them() {
        let edit = insert_block("hello", 5, 5, "![pic](attachments/a.webp)");
        assert_eq!(edit.text, "hello\n\n![pic](attachments/a.webp)");
        assert_eq!(edit.selection.0, 33);
    }

    #[test]
    fn a_block_after_a_blank_line_does_not_add_more() {
        let edit = insert_block("hello\n\n", 7, 7, "![pic](attachments/a.webp)");
        assert_eq!(edit.text, "hello\n\n![pic](attachments/a.webp)");
    }

    #[test]
    fn a_block_between_paragraphs_keeps_them_apart() {
        let edit = insert_block("one\ntwo", 3, 3, "![x](attachments/b.webp)");
        assert_eq!(edit.text, "one\n\n![x](attachments/b.webp)\n\ntwo");
    }

    #[test]
    fn a_block_handles_an_emoji_before_the_caret() {
        let edit = insert_block("😀 ", 3, 3, "![x](attachments/b.webp)");
        assert_eq!(edit.text, "😀 \n\n![x](attachments/b.webp)");
    }

    #[test]
    fn bold_wraps_a_selected_word() {
        let edit = apply("hello world", 6, 11, Mark::Bold);
        assert_eq!(edit.text, "hello **world**");
        assert_eq!(edit.selection, (8, 13));
    }

    #[test]
    fn emoji_before_the_selection_does_not_break_slicing() {
        // "😀 " is three UTF-16 code units: the surrogate pair and a space.
        let edit = apply("😀 hi", 3, 5, Mark::Bold);
        assert_eq!(edit.text, "😀 **hi**");
        assert_eq!(edit.selection, (5, 7));
    }

    #[test]
    fn a_bare_caret_gets_markers_around_it() {
        let edit = apply("ab", 1, 1, Mark::Italic);
        assert_eq!(edit.text, "a**b"); // the two markers meet around an empty selection
        assert_eq!(edit.selection, (2, 2));
    }

    #[test]
    fn link_selects_the_url_when_text_is_selected() {
        let edit = apply("see docs", 4, 8, Mark::Link);
        assert_eq!(edit.text, "see [docs](https://)");
        assert_eq!(&edit.text[edit.selection.0..edit.selection.1], "https://");
    }

    #[test]
    fn checklist_prefixes_each_selected_line_and_toggles_off() {
        let edit = apply("one\ntwo", 0, 7, Mark::Checklist);
        assert_eq!(edit.text, "- [ ] one\n- [ ] two");
        let toggled = apply(
            &edit.text,
            edit.selection.0,
            edit.selection.1,
            Mark::Checklist,
        );
        assert_eq!(toggled.text, "one\ntwo");
    }

    #[test]
    fn a_line_prefix_keeps_the_caret_on_its_line() {
        let edit = apply("hello world", 7, 7, Mark::Bullet);
        assert_eq!(edit.text, "- hello world");
        assert_eq!(edit.selection, (9, 9));
    }

    #[test]
    fn heading_toggles_off_at_the_line_start() {
        let edit = apply("# title", 0, 0, Mark::Heading);
        assert_eq!(edit.text, "title");
        assert_eq!(edit.selection, (0, 0));
    }

    #[test]
    fn a_multiline_code_block_uses_fences() {
        let edit = apply("a\nb", 0, 3, Mark::CodeBlock);
        assert_eq!(edit.text, "```\na\nb\n```");
    }
}
