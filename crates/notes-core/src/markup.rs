//! Links and images found in the Markdown source.
//!
//! Write mode edits the source in a plain text box, so the UI cannot rely on
//! the rendered HTML to know what is a link. This module scans the same source
//! once and returns byte ranges that the editor can paint and hit-test. It is
//! deliberately UI-agnostic: no HTML and no colours.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::parse_attachment_url;

/// The largest number of tokens one scan returns.
///
/// A pathological note full of links would otherwise turn every keystroke
/// into thousands of DOM nodes. Beyond this the editor stays plain text.
pub const MAX_MARKUP_SPANS: usize = 2_000;

/// What a source range represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupKind {
    /// A Markdown link, an autolink or a bare web address.
    Link,
    /// A Markdown image.
    Image,
}

/// One highlighted source range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkupSpan {
    /// Byte offset of the first character, inclusive.
    pub start: usize,
    /// Byte offset after the last character, exclusive.
    pub end: usize,
    /// Link or image.
    pub kind: MarkupKind,
    /// Destination exactly as written in the note.
    pub destination: String,
    /// Lowercase content hash when this is an attachment image.
    pub attachment_hash: Option<String>,
    /// Whether the span came from a bare URL rather than Markdown syntax.
    pub bare: bool,
}

/// Find links and images outside code blocks and inline code.
///
/// Markdown links and images keep their whole source token, including the
/// brackets and destination, so the editor can underline exactly what was
/// typed. Bare `http://` and `https://` addresses are found in ordinary text
/// only. An image nested inside a link wins: clicking the image opens the
/// image, not the surrounding link.
pub fn markup_spans(source: &str) -> Vec<MarkupSpan> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let mut images = Vec::new();
    let mut links = Vec::new();
    let mut blocked = Vec::new();
    let mut code_block_start = None;

    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_block_start = Some(range.start),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = code_block_start.take() {
                    blocked.push(start..range.end.max(start));
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                blocked.push(range.clone());
                links.push(MarkupSpan {
                    start: range.start,
                    end: range.end,
                    kind: MarkupKind::Link,
                    destination: dest_url.to_string(),
                    attachment_hash: None,
                    bare: false,
                });
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                blocked.push(range.clone());
                let attachment_hash =
                    parse_attachment_url(&dest_url).map(|parsed| parsed.hash.to_ascii_lowercase());
                images.push(MarkupSpan {
                    start: range.start,
                    end: range.end,
                    kind: MarkupKind::Image,
                    destination: dest_url.to_string(),
                    attachment_hash,
                    bare: false,
                });
            }
            Event::Code(_) | Event::Html(_) | Event::InlineHtml(_) => {
                blocked.push(range.clone());
            }
            _ => {}
        }
    }

    // Scan the source as a whole instead of individual text events. Markdown
    // emphasis can split a URL like `Foo_(bar)` into several text events.
    let mut bare: Vec<MarkupSpan> = bare_url_ranges(source)
        .into_iter()
        .filter(|span| {
            !blocked
                .iter()
                .any(|blocked| blocked.start < span.end && span.start < blocked.end)
        })
        .filter_map(|span| {
            let destination = source.get(span.clone())?.to_string();
            Some(MarkupSpan {
                start: span.start,
                end: span.end,
                kind: MarkupKind::Link,
                destination,
                attachment_hash: None,
                bare: true,
            })
        })
        .collect();

    // Images are accepted first so an image inside a link remains the target.
    // Links and bare URLs that overlap an accepted token are then dropped.
    let mut accepted: Vec<MarkupSpan> = Vec::new();
    images.sort_by_key(|span| span.start);
    for span in images {
        if !accepted.iter().any(|taken| overlaps(&span, taken)) {
            accepted.push(span);
        }
    }
    links.sort_by_key(|span| span.start);
    for span in links {
        if !accepted.iter().any(|taken| overlaps(&span, taken)) {
            accepted.push(span);
        }
    }
    bare.sort_by_key(|span| span.start);
    for span in bare {
        if !accepted.iter().any(|taken| overlaps(&span, taken)) {
            accepted.push(span);
        }
    }
    accepted.sort_by_key(|span| (span.start, span.end));
    accepted.truncate(MAX_MARKUP_SPANS);
    accepted
}

fn overlaps(a: &MarkupSpan, b: &MarkupSpan) -> bool {
    a.start < b.end && b.start < a.end
}

/// Byte ranges of bare web addresses in one plain-text slice.
fn bare_url_ranges(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut search_from = 0;
    while let Some(relative) = text[search_from..].find("http") {
        let start = search_from + relative;
        let rest = &text[start..];
        let scheme_len = if rest.starts_with("https://") {
            8
        } else if rest.starts_with("http://") {
            7
        } else {
            search_from = start + 4;
            continue;
        };
        if start > 0 {
            let Some(previous) = text[..start].chars().next_back() else {
                search_from = start + 4;
                continue;
            };
            if previous.is_alphanumeric() || matches!(previous, '_' | '-' | '.' | '/' | '@') {
                search_from = start + 4;
                continue;
            }
        }

        let content_start = start + scheme_len;
        let mut end = content_start;
        let mut parens = 0i32;
        for (offset, character) in text[content_start..].char_indices() {
            if character.is_whitespace() || matches!(character, '<' | '>' | '"' | '\'' | '`') {
                break;
            }
            if character == '(' {
                parens += 1;
            } else if character == ')' {
                if parens == 0 {
                    break;
                }
                parens -= 1;
            }
            end = content_start + offset + character.len_utf8();
        }
        while end > content_start {
            let Some(last) = text[..end].chars().next_back() else {
                break;
            };
            if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | ']' | '}') {
                end -= last.len_utf8();
            } else {
                break;
            }
        }

        let has_host = text[content_start..end]
            .chars()
            .any(|character| !matches!(character, '/' | '?' | '#'));
        if end > content_start && has_host {
            found.push(start..end);
            search_from = end;
        } else {
            search_from = content_start;
        }
    }
    found
}

/// A URL that may be opened outside the app.
///
/// Only `http`, `https` and `mailto` are accepted. In particular this rejects
/// `javascript:`, `data:`, `file:`, `tel:` and protocol-relative URLs, and
/// requires an actual host after `http(s)://`. Dioxus's native WebView
/// navigation handler opens the accepted schemes in the system browser.
pub fn external_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    let colon = trimmed.find(':')?;
    let scheme = &trimmed[..colon];
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    let rest = &trimmed[colon + 1..];
    match scheme.as_str() {
        "http" | "https" => {
            let authority = rest.strip_prefix("//")?;
            let host = authority.split(['/', '?', '#']).next().unwrap_or_default();
            if host.is_empty() {
                None
            } else {
                Some(format!("{scheme}:{rest}"))
            }
        }
        "mailto" => {
            let target = rest.split('?').next().unwrap_or_default();
            if target.is_empty() {
                None
            } else {
                Some(format!("mailto:{rest}"))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{external_url, markup_spans, MarkupKind};

    fn hash() -> String {
        "a".repeat(64)
    }

    #[test]
    fn finds_links_and_attachment_images() {
        let hash = hash();
        let source = format!("see [docs](https://example.com) and ![pic](attachments/{hash}.webp)");
        let spans = markup_spans(&source);
        assert_eq!(spans.len(), 2);
        assert_eq!(
            &source[spans[0].start..spans[0].end],
            "[docs](https://example.com)"
        );
        assert_eq!(spans[0].kind, MarkupKind::Link);
        assert_eq!(spans[0].destination, "https://example.com");
        assert!(!spans[0].bare);
        assert_eq!(
            &source[spans[1].start..spans[1].end],
            format!("![pic](attachments/{hash}.webp)")
        );
        assert_eq!(spans[1].kind, MarkupKind::Image);
        assert_eq!(spans[1].attachment_hash.as_deref(), Some(hash.as_str()));
    }

    #[test]
    fn finds_bare_urls_without_sentence_punctuation() {
        let source = "Watch https://www.youtube.com/watch?v=njy_h7TLJlo&t=41s, then go.";
        let spans = markup_spans(source);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            &source[spans[0].start..spans[0].end],
            "https://www.youtube.com/watch?v=njy_h7TLJlo&t=41s"
        );
        assert_eq!(spans[0].kind, MarkupKind::Link);
        assert!(spans[0].bare);
    }

    #[test]
    fn keeps_balanced_parentheses_and_drops_a_closing_one() {
        let source = "(see https://en.wikipedia.org/wiki/Foo_(bar))";
        let spans = markup_spans(source);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            &source[spans[0].start..spans[0].end],
            "https://en.wikipedia.org/wiki/Foo_(bar)"
        );
    }

    #[test]
    fn ignores_code_spans_blocks_and_link_labels() {
        let source = "`[x](https://inline.example)`\n\n```\nhttps://block.example\n```\n\n\
                      [https://label.example](https://target.example)";
        let spans = markup_spans(source);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            &source[spans[0].start..spans[0].end],
            "[https://label.example](https://target.example)"
        );
        assert_eq!(spans[0].destination, "https://target.example");
        assert!(!spans[0].bare);
    }

    #[test]
    fn resolves_reference_links_and_autolinks() {
        let source = "[docs][d]\n\n[d]: https://example.com\n\n<https://auto.example>";
        let spans = markup_spans(source);
        assert_eq!(spans.len(), 3);
        assert_eq!(&source[spans[0].start..spans[0].end], "[docs][d]");
        assert_eq!(spans[0].destination, "https://example.com");
        assert_eq!(&source[spans[1].start..spans[1].end], "https://example.com");
        assert!(spans[1].bare);
        assert_eq!(
            &source[spans[2].start..spans[2].end],
            "<https://auto.example>"
        );
    }

    #[test]
    fn nested_image_is_the_token_and_outer_link_is_dropped() {
        let hash = hash();
        let source = format!("[![alt](attachments/{hash}.webp)](https://example.com)");
        let spans = markup_spans(&source);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].kind, MarkupKind::Image);
        assert_eq!(spans[0].attachment_hash.as_deref(), Some(hash.as_str()));
    }

    #[test]
    fn ranges_are_utf8_byte_offsets() {
        let source = "🙂 [link](https://example.com)";
        let spans = markup_spans(source);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            &source[spans[0].start..spans[0].end],
            "[link](https://example.com)"
        );
    }

    #[test]
    fn rejects_hostile_or_incomplete_external_urls() {
        assert_eq!(
            external_url("https://example.com/a").as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(
            external_url(" https://example.com ").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            external_url(" HTTPS://Example.com/A ").as_deref(),
            Some("https://Example.com/A")
        );
        assert_eq!(
            external_url("MAILTO:a@example.com").as_deref(),
            Some("mailto:a@example.com")
        );
        assert_eq!(external_url("tel:+15551234"), None);
        assert_eq!(external_url("javascript:alert(1)"), None);
        assert_eq!(external_url("data:text/html,hi"), None);
        assert_eq!(external_url("file:///etc/passwd"), None);
        assert_eq!(external_url("https://"), None);
        assert_eq!(external_url("https://?q"), None);
        assert_eq!(external_url("//example.com"), None);
        assert_eq!(external_url("mailto:"), None);
        assert_eq!(external_url("https://exa mple.com"), None);
    }
}
