//! Attachment references inside Markdown.
//!
//! An attachment is stored by the SHA-256 of its bytes, and a page refers to it
//! with an ordinary relative Markdown URL:
//!
//! ```text
//! ![holiday](attachments/9f2c...e1.webp)
//! ```
//!
//! The path is the same one ADR-0006 writes inside an export archive, so an
//! exported note keeps working and an imported note keeps its links without a
//! rewrite. Nothing here touches the filesystem or the UI: the store uses
//! [`references`] to build its attachment index, and the app uses
//! [`parse_attachment_url`] to turn a link into bytes it can display.

use std::collections::HashSet;
use std::fmt::Write as _;

use pulldown_cmark::{Options, Parser, Tag};
use sha2::{Digest, Sha256};

/// Directory that holds attachments in a note and in an export.
pub const ATTACHMENTS_DIR: &str = "attachments";

/// Longest extension this build accepts in a reference.
const MAX_EXTENSION: usize = 16;

/// Longest display name kept for an attachment.
const MAX_NAME_CHARS: usize = 120;

/// One parsed `attachments/<hash>.<ext>` URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachmentUrl<'a> {
    /// Lowercase SHA-256 of the bytes.
    pub hash: &'a str,
    /// File extension without the dot, lowercase.
    pub extension: &'a str,
}

impl AttachmentUrl<'_> {
    /// MIME type inferred from the extension, when it is a known one.
    pub fn mime(&self) -> Option<&'static str> {
        mime_for_extension(self.extension)
    }
}

/// SHA-256 of `bytes` as lowercase hexadecimal.
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hash = String::with_capacity(64);
    for byte in digest {
        // Writing to a String cannot fail.
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

/// Parse a Markdown destination that points at an attachment.
///
/// Only an exact relative `attachments/` path with a 64-character hex hash and
/// a short lowercase extension is accepted. Absolute URLs, `..` and query
/// strings are not attachment references. An uppercase hash is accepted but
/// the caller stores and compares the lowercase form.
pub fn parse_attachment_url(url: &str) -> Option<AttachmentUrl<'_>> {
    let url = url.trim();
    let rest = url.strip_prefix(ATTACHMENTS_DIR)?.strip_prefix('/')?;
    let (hash, extension) = rest.rsplit_once('.')?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    if extension.is_empty()
        || extension.len() > MAX_EXTENSION
        || !extension
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() && !ch.is_ascii_uppercase())
    {
        return None;
    }
    Some(AttachmentUrl { hash, extension })
}

/// One attachment a note body refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentReference {
    /// Lowercase SHA-256 of the bytes.
    pub hash: String,
    /// File extension without the dot, lowercase.
    pub extension: String,
}

impl AttachmentReference {
    /// MIME type inferred from the extension, when it is a known one.
    pub fn mime(&self) -> Option<&'static str> {
        mime_for_extension(&self.extension)
    }
}

/// Every distinct attachment a note body refers to, in first-seen order.
///
/// Markdown links and images are both covered, including reference-style
/// links, because pulldown-cmark resolves those to their destination.
pub fn references(markdown: &str) -> Vec<AttachmentReference> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let mut seen = HashSet::new();
    let mut references = Vec::new();
    for event in Parser::new_ext(markdown, options) {
        let destination = match event {
            pulldown_cmark::Event::Start(Tag::Link { dest_url, .. })
            | pulldown_cmark::Event::Start(Tag::Image { dest_url, .. }) => dest_url,
            _ => continue,
        };
        let Some(parsed) = parse_attachment_url(&destination) else {
            continue;
        };
        let hash = parsed.hash.to_ascii_lowercase();
        if seen.insert(hash.clone()) {
            references.push(AttachmentReference {
                hash,
                extension: parsed.extension.to_ascii_lowercase(),
            });
        }
    }
    references
}

/// A link whose destination points at the attachment being removed.
struct LinkRemoval {
    start: usize,
    end: usize,
    content_start: Option<usize>,
    content_end: Option<usize>,
}

/// Remove the references to one attachment from a note body.
///
/// Images are removed whole. Links keep their label text and lose only the
/// attachment URL, so a sentence that happened to link a file stays readable.
/// This is an ordinary text edit: the note's history still contains the old
/// reference, and the bytes are removed separately.
pub fn remove_attachment_references(markdown: &str, hash: &str) -> String {
    let hash = hash.to_ascii_lowercase();
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    // Byte ranges covered by image syntax and by the opening and closing
    // punctuation of a link. Events are nested, so track the image start and
    // only remove the outer image once.
    let mut removals: Vec<std::ops::Range<usize>> = Vec::new();
    let mut image_start: Option<usize> = None;
    let mut image_depth = 0usize;
    let mut link: Option<LinkRemoval> = None;
    for (event, range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            pulldown_cmark::Event::Start(Tag::Image { dest_url, .. }) => {
                if image_depth == 0 && reference_matches(&dest_url, &hash) {
                    image_start = Some(range.start);
                }
                image_depth += 1;
                note_link_content(&mut link, &range);
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Image) => {
                image_depth = image_depth.saturating_sub(1);
                if image_depth == 0 {
                    if let Some(start) = image_start.take() {
                        removals.push(start..range.end);
                    }
                }
            }
            pulldown_cmark::Event::Start(Tag::Link { dest_url, .. })
                if reference_matches(&dest_url, &hash) =>
            {
                link = Some(LinkRemoval {
                    start: range.start,
                    end: range.end,
                    content_start: None,
                    content_end: None,
                });
            }
            pulldown_cmark::Event::End(pulldown_cmark::TagEnd::Link) => {
                if let Some(link) = link.take() {
                    match (link.content_start, link.content_end) {
                        (Some(content_start), Some(content_end)) if content_start < content_end => {
                            removals.push(link.start..content_start);
                            removals.push(content_end..link.end);
                        }
                        _ => removals.push(link.start..link.end),
                    }
                }
            }
            _ => {
                note_link_content(&mut link, &range);
            }
        }
    }

    if removals.is_empty() {
        return markdown.to_string();
    }
    removals.sort_by_key(|range| range.start);
    let mut out = String::with_capacity(markdown.len());
    let mut cursor = 0;
    for range in removals {
        if range.start < cursor {
            // Nested image and link ranges overlap; the second removal starts
            // inside the first and has already been covered.
            continue;
        }
        out.push_str(&markdown[cursor..range.start]);
        cursor = range.end;
    }
    out.push_str(&markdown[cursor..]);
    out
}

fn note_link_content(link: &mut Option<LinkRemoval>, range: &std::ops::Range<usize>) {
    let Some(link) = link.as_mut() else {
        return;
    };
    if link.content_start.is_none() {
        link.content_start = Some(range.start);
    }
    link.content_end = Some(range.end);
}

fn reference_matches(destination: &str, hash: &str) -> bool {
    parse_attachment_url(destination).is_some_and(|parsed| parsed.hash.eq_ignore_ascii_case(hash))
}

/// The relative URL a note uses for an attachment.
pub fn attachment_url(hash: &str, mime: &str) -> String {
    format!(
        "{ATTACHMENTS_DIR}/{hash}.{}",
        extension_for_mime(mime).unwrap_or("bin")
    )
}

/// Extension used for a MIME type, without the dot.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    let mime = mime.split(';').next().unwrap_or(mime).trim();
    match mime.to_ascii_lowercase().as_str() {
        "image/webp" => Some("webp"),
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/avif" => Some("avif"),
        "image/svg+xml" => Some("svg"),
        "application/pdf" => Some("pdf"),
        "text/plain" => Some("txt"),
        _ => None,
    }
}

/// MIME type for a file extension, without the dot.
pub fn mime_for_extension(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "webp" => Some("image/webp"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "avif" => Some("image/avif"),
        "svg" => Some("image/svg+xml"),
        "pdf" => Some("application/pdf"),
        "txt" => Some("text/plain"),
        _ => None,
    }
}

/// Reduce a supplied file name to a safe display name with the right suffix.
///
/// The bytes decide the format, not the name the user's camera chose, so the
/// extension always matches `mime`. Path separators and control characters are
/// removed, and an empty result becomes `attachment.<ext>`.
pub fn normalize_file_name(name: &str, mime: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem).trim();
    let stem: String = stem
        .chars()
        .filter(|ch| !ch.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    let stem = stem.trim_matches(['.', ' ']).trim();
    let stem = if stem.is_empty() { "attachment" } else { stem };
    match extension_for_mime(mime) {
        Some(extension) => format!("{stem}.{extension}"),
        None => format!("{stem}.bin"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        attachment_url, content_hash, mime_for_extension, normalize_file_name,
        parse_attachment_url, references, remove_attachment_references,
    };

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn a_reference_round_trips() {
        let url = attachment_url(HASH, "image/webp");
        assert_eq!(url, format!("attachments/{HASH}.webp"));
        let parsed = parse_attachment_url(&url).expect("parse");
        assert_eq!(parsed.hash, HASH);
        assert_eq!(parsed.extension, "webp");
        assert_eq!(parsed.mime(), Some("image/webp"));
    }

    #[test]
    fn unsafe_or_foreign_urls_are_not_attachments() {
        assert!(parse_attachment_url("https://example.com/a.webp").is_none());
        assert!(parse_attachment_url("attachments/../a.webp").is_none());
        assert!(parse_attachment_url("attachments/short.webp").is_none());
        assert!(parse_attachment_url(&format!("attachments/{HASH}")).is_none());
        assert!(parse_attachment_url(&format!("attachments/{HASH}.WEBP")).is_none());
        assert!(parse_attachment_url(&format!("attachments/{HASH}.webp?x=1")).is_none());
    }

    #[test]
    fn links_and_images_both_count_once() {
        let body = format!(
            "![a](attachments/{HASH}.webp)\n\n[x](attachments/{HASH}.webp)\n\n![b](https://example.com/b.png)"
        );
        let found = references(&body);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].hash, HASH);
        assert_eq!(found[0].extension, "webp");
        assert_eq!(found[0].mime(), Some("image/webp"));
    }

    #[test]
    fn reference_style_links_are_found() {
        let body = format!("![a][pic]\n\n[pic]: attachments/{HASH}.png");
        let found = references(&body);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].hash, HASH);
        assert_eq!(found[0].extension, "png");
    }

    #[test]
    fn an_attachment_can_be_removed_from_the_note_body() {
        let other = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        let body = format!(
            "Trip\n\n![holiday](attachments/{HASH}.webp)\n\nSee [the guide](attachments/{HASH}.pdf) and [other](attachments/{other}.png)."
        );
        let edited = remove_attachment_references(&body, HASH);
        assert!(!edited.contains(HASH));
        assert!(
            edited.contains("See the guide and [other]"),
            "edited body: {edited:?}"
        );
        assert!(edited.contains(other));
        assert!(references(&edited)
            .iter()
            .all(|reference| reference.hash != HASH));

        assert_eq!(remove_attachment_references("unchanged", HASH), "unchanged");
    }

    #[test]
    fn a_reference_style_attachment_link_keeps_its_label() {
        let body = format!("Read [the report][file].\n\n[file]: attachments/{HASH}.pdf");
        let edited = remove_attachment_references(&body, HASH);
        assert!(
            edited.contains("Read the report."),
            "edited body: {edited:?}"
        );
        assert!(
            edited.contains("[file]: attachments/"),
            "the now-unused definition may stay"
        );
        assert!(references(&edited).is_empty());
    }

    #[test]
    fn names_are_reduced_to_a_safe_suffix() {
        assert_eq!(
            normalize_file_name("../../Camera/IMG 1.PNG", "image/webp"),
            "IMG 1.webp"
        );
        assert_eq!(normalize_file_name("", "image/jpeg"), "attachment.jpg");
        assert_eq!(normalize_file_name("..", "image/png"), "attachment.png");
        assert_eq!(
            normalize_file_name("report", "application/pdf"),
            "report.pdf"
        );
        assert_eq!(
            normalize_file_name("x.exe", "application/octet-stream"),
            "x.bin"
        );
    }

    #[test]
    fn the_content_hash_matches_sha256() {
        assert_eq!(
            content_hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn known_extensions_have_mime_types() {
        assert_eq!(mime_for_extension("webp"), Some("image/webp"));
        assert_eq!(mime_for_extension("JPG"), Some("image/jpeg"));
        assert_eq!(mime_for_extension("zip"), None);
    }
}
