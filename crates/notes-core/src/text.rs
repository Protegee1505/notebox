//! Turning a text-box edit into the smallest CRDT operation.
//!
//! A native text box reports the whole new value after each input event. The
//! prefix and suffix shared with the previous value are left untouched; only
//! the differing middle becomes a Loro delete and insert. This keeps edits
//! small, preserves remote cursor positions, and avoids running a full diff on
//! every keystroke.

use loro::LoroText;

use crate::NoteError;

/// The middle section that changed between two text-box values.
///
/// Positions and lengths are in Unicode scalar values, matching [`LoroText`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextReplacement<'a> {
    /// Unicode scalar position where the replacement starts.
    pub char_pos: usize,
    /// Number of Unicode scalars removed from the old value.
    pub removed_chars: usize,
    /// Text inserted in their place, borrowed from the new value.
    pub inserted: &'a str,
}

impl TextReplacement<'_> {
    /// Whether the two values are identical.
    pub fn is_noop(&self) -> bool {
        self.removed_chars == 0 && self.inserted.is_empty()
    }
}

/// Compute the single replacement that turns `old` into `new`.
///
/// The common prefix and suffix are found by Unicode scalar, never by byte, so
/// the result is safe for emoji and Android IME composition text. The middle is
/// not diffed further: for a text box this is the range the user actually
/// changed.
pub fn diff_replacement<'a>(old: &str, new: &'a str) -> TextReplacement<'a> {
    let old_len = old.chars().count();
    let new_len = new.chars().count();

    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();

    if prefix == old_len && prefix == new_len {
        return TextReplacement {
            char_pos: prefix,
            removed_chars: 0,
            inserted: "",
        };
    }

    // Do not let the backward scan overlap the shared prefix.
    let max_suffix = (old_len - prefix).min(new_len - prefix);
    let suffix = old
        .chars()
        .rev()
        .zip(new.chars().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(max_suffix);

    let inserted_start = byte_offset_of_char(new, prefix);
    let inserted_end = byte_offset_of_char(new, new_len - suffix);

    TextReplacement {
        char_pos: prefix,
        removed_chars: old_len - prefix - suffix,
        inserted: &new[inserted_start..inserted_end],
    }
}

/// Apply a prefix/suffix replacement to a Loro text container.
pub fn apply_replacement(text: &LoroText, edit: &TextReplacement<'_>) -> Result<(), NoteError> {
    if edit.removed_chars > 0 {
        text.delete(edit.char_pos, edit.removed_chars)?;
    }
    if !edit.inserted.is_empty() {
        text.insert(edit.char_pos, edit.inserted)?;
    }
    Ok(())
}

/// Byte offset of the `n`th Unicode scalar, or `text.len()` at the end.
fn byte_offset_of_char(text: &str, n: usize) -> usize {
    text.char_indices()
        .nth(n)
        .map_or(text.len(), |(offset, _)| offset)
}

#[cfg(test)]
mod tests {
    use super::diff_replacement;

    #[test]
    fn identical_values_are_a_noop() {
        let edit = diff_replacement("hello", "hello");
        assert!(edit.is_noop());
        assert_eq!(edit.char_pos, 5);
    }

    #[test]
    fn typing_at_the_end_inserts_one_character() {
        let edit = diff_replacement("hello", "hello!");
        assert_eq!(edit.char_pos, 5);
        assert_eq!(edit.removed_chars, 0);
        assert_eq!(edit.inserted, "!");
    }

    #[test]
    fn inserting_in_the_middle_keeps_the_suffix() {
        let edit = diff_replacement("hello world", "hello brave world");
        assert_eq!(edit.char_pos, 6);
        assert_eq!(edit.removed_chars, 0);
        assert_eq!(edit.inserted, "brave ");
    }

    #[test]
    fn replacing_a_word_removes_only_that_word() {
        let edit = diff_replacement("hello world", "hello there");
        assert_eq!(edit.char_pos, 6);
        assert_eq!(edit.removed_chars, 5);
        assert_eq!(edit.inserted, "there");
    }

    #[test]
    fn emoji_are_counted_as_unicode_scalars() {
        let edit = diff_replacement("hi 😀 there", "hi 😀😀 there");
        assert_eq!(edit.char_pos, 4);
        assert_eq!(edit.removed_chars, 0);
        assert_eq!(edit.inserted, "😀");
    }

    #[test]
    fn deletion_at_the_start_keeps_the_tail() {
        let edit = diff_replacement("remove this", "this");
        assert_eq!(edit.char_pos, 0);
        assert_eq!(edit.removed_chars, 7);
        assert_eq!(edit.inserted, "");
    }

    #[test]
    fn a_fully_different_value_replaces_everything() {
        let edit = diff_replacement("abc", "xyz");
        assert_eq!(edit.char_pos, 0);
        assert_eq!(edit.removed_chars, 3);
        assert_eq!(edit.inserted, "xyz");
    }
}
