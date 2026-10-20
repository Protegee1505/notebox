//! Inline media URLs and human-readable sizes.
//!
//! Attachments live in SQLite as bytes. The rendered note is injected into the
//! page, so the simplest cross-platform URL is a `data:` URL. A 400 KB WebP
//! becomes about 530 KB of text once, cached per content hash by the app; the
//! preview re-renders from that cache without touching the database again.
//!
//! A browser could use `blob:` URLs and save memory, but `data:` works in the
//! browser, the desktop WebView and Android with one code path. P7 can swap the
//! resolver for object URLs behind the same helper.

use base64::Engine as _;

/// An inline image URL the renderer accepts.
pub fn image_data_url(mime: &str, bytes: &[u8]) -> String {
    let mime = if mime.starts_with("image/") {
        mime
    } else {
        "application/octet-stream"
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// Size for the attachment row, using decimal units a person recognises.
pub fn format_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.0} KB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

/// Short label for an attachment row, without dragging the whole name in.
pub fn attachment_kind(mime: &str) -> &'static str {
    match mime {
        "image/webp" | "image/png" | "image/jpeg" | "image/gif" | "image/avif" => "Image",
        "application/pdf" => "PDF",
        "text/plain" => "Text",
        _ => "File",
    }
}

#[cfg(test)]
mod tests {
    use super::{attachment_kind, format_size, image_data_url};

    #[test]
    fn a_data_url_carries_the_mime_type_and_bytes() {
        assert_eq!(
            image_data_url("image/webp", b"abc"),
            "data:image/webp;base64,YWJj"
        );
        assert!(image_data_url("text/html", b"abc").starts_with("data:application/octet-stream"));
    }

    #[test]
    fn sizes_use_readable_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(2 * 1024), "2 KB");
        assert_eq!(format_size(3 * 1024 * 1024 / 2), "1.5 MB");
    }

    #[test]
    fn kinds_are_short_labels() {
        assert_eq!(attachment_kind("image/webp"), "Image");
        assert_eq!(attachment_kind("application/pdf"), "PDF");
        assert_eq!(attachment_kind("application/zip"), "File");
    }
}
