//! Turning Markdown token ranges into pieces the editor overlay can paint.
//!
//! The scanner in `notes-core` returns byte ranges. The editor works with a
//! UTF-16 caret reported by the browser, so this module keeps both
//! conversions and the small display helpers in one tested place.

use notes_core::{MarkupKind, MarkupSpan};

use crate::markdown_ops::utf16_to_byte;

/// One contiguous piece of the source drawn in the overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Byte offset into the note body, inclusive.
    pub start: usize,
    /// Byte offset into the note body, exclusive.
    pub end: usize,
    /// Plain text or a link/image token.
    pub kind: SegmentKind,
    /// Index into the `MarkupSpan` slice when this is a token.
    pub span: Option<usize>,
}

/// What a segment paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    /// Ordinary text with no decoration.
    Text,
    /// A Markdown link, autolink or bare URL.
    Link,
    /// A Markdown image.
    Image,
}

/// Split `text` into plain and token segments without changing the source.
pub fn segments(text: &str, spans: &[MarkupSpan]) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut cursor = 0;
    for (index, span) in spans.iter().enumerate() {
        if span.start < cursor || span.end > text.len() || span.start >= span.end {
            continue;
        }
        if span.start > cursor {
            out.push(Segment {
                start: cursor,
                end: span.start,
                kind: SegmentKind::Text,
                span: None,
            });
        }
        out.push(Segment {
            start: span.start,
            end: span.end,
            kind: match span.kind {
                MarkupKind::Link => SegmentKind::Link,
                MarkupKind::Image => SegmentKind::Image,
            },
            span: Some(index),
        });
        cursor = span.end;
    }
    if cursor < text.len() {
        out.push(Segment {
            start: cursor,
            end: text.len(),
            kind: SegmentKind::Text,
            span: None,
        });
    }
    out
}

/// Index of the link or image token containing a UTF-16 caret offset.
///
/// A caret immediately after a token still belongs to it, so pressing
/// Ctrl+Enter at the closing bracket opens the token just typed.
pub fn token_at(text: &str, spans: &[MarkupSpan], caret_utf16: usize) -> Option<usize> {
    let byte = utf16_to_byte(text, caret_utf16);
    spans
        .iter()
        .position(|span| span.start <= byte && byte <= span.end)
}

/// Short host shown next to an "Open link" action.
pub fn link_host(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let (scheme, rest) = trimmed.split_once(':')?;
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => {
            let authority = rest.strip_prefix("//")?;
            let host = authority.split(['/', '?', '#']).next()?;
            let host = host.rsplit('@').next()?;
            let host = host.strip_prefix("www.").unwrap_or(host);
            if host.is_empty() {
                None
            } else {
                Some(host.to_string())
            }
        }
        "mailto" => {
            let target = rest.split('?').next()?.trim();
            if target.is_empty() {
                None
            } else {
                Some(target.to_string())
            }
        }
        _ => None,
    }
}

/// Keep a label compact without splitting a character.
pub fn shorten(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

#[cfg(test)]
mod tests {
    use notes_core::{markup_spans, MarkupKind};

    use super::{link_host, segments, shorten, token_at, SegmentKind};

    fn hash() -> String {
        "b".repeat(64)
    }

    #[test]
    fn segments_cover_the_source_in_order() {
        let hash = hash();
        let source =
            format!("🙂 [docs](https://example.com) then ![pic](attachments/{hash}.webp) done");
        let spans = markup_spans(&source);
        let pieces = segments(&source, &spans);
        let rebuilt: String = pieces
            .iter()
            .map(|segment| &source[segment.start..segment.end])
            .collect();
        assert_eq!(rebuilt, source);
        assert_eq!(
            pieces
                .iter()
                .filter(|segment| segment.kind == SegmentKind::Link)
                .count(),
            1
        );
        assert_eq!(
            pieces
                .iter()
                .filter(|segment| segment.kind == SegmentKind::Image)
                .count(),
            1
        );
        assert_eq!(
            &source[spans[0].start..spans[0].end],
            "[docs](https://example.com)"
        );
    }

    #[test]
    fn caret_maps_from_utf16_to_a_token() {
        let source = "🙂 [link](https://example.com) tail";
        let spans = markup_spans(source);
        assert_eq!(spans[0].kind, MarkupKind::Link);
        let link_start = source[..spans[0].start].encode_utf16().count();
        assert_eq!(token_at(source, &spans, link_start), Some(0));
        assert_eq!(token_at(source, &spans, link_start + 3), Some(0));
        assert_eq!(token_at(source, &spans, 0), None);
        assert_eq!(
            token_at(source, &spans, source.encode_utf16().count()),
            None
        );
    }

    #[test]
    fn hosts_are_short_and_readable() {
        assert_eq!(
            link_host("https://www.youtube.com/watch?v=1").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(
            link_host("https://user@example.com/path").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            link_host("mailto:a@example.com").as_deref(),
            Some("a@example.com")
        );
        assert_eq!(link_host("tel:+15551234"), None);
        assert_eq!(link_host("relative/path"), None);
    }

    #[test]
    fn shortening_respects_character_boundaries() {
        assert_eq!(shorten("image", 8), "image");
        assert_eq!(shorten("a long attachment name", 8), "a long \u{2026}");
        assert_eq!(shorten("🙂🙂🙂🙂", 3), "🙂🙂\u{2026}");
    }
}
