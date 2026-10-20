//! Markdown to HTML for the rendered note view.
//!
//! Notes can contain anything a person pastes, including HTML and links with
//! hostile schemes, and the rendered view is injected into the app's own page.
//! Raw HTML is therefore shown as text, never executed, and link and image
//! destinations are limited to schemes that cannot run script.

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag};

/// Render a note body to HTML.
///
/// Tables, strikethrough, task lists and footnotes are enabled, matching the
/// toolbar in `PLAN.md`. Raw HTML is escaped.
pub fn render_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);

    let events = Parser::new_ext(markdown, options).map(neutralize);
    let mut out = String::with_capacity(markdown.len() + markdown.len() / 2);
    html::push_html(&mut out, events);
    out
}

/// Turn anything the HTML renderer would emit verbatim into plain text.
fn neutralize(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Html(html) => Event::Text(html),
        Event::InlineHtml(html) => Event::Text(html),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Link {
            link_type,
            dest_url: safe_url(&dest_url),
            title,
            id,
        }),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Event::Start(Tag::Image {
            link_type,
            dest_url: safe_url(&dest_url),
            title,
            id,
        }),
        event => event,
    }
}

/// Allow only destinations that cannot execute script in the page.
///
/// Relative links, fragments, `http`, `https`, `mailto` and `tel` pass;
/// anything else, `javascript:` and `data:` in particular, becomes `#`.
fn safe_url(url: &str) -> CowStr<'static> {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    let allowed = match lower.find(':') {
        // A colon before any slash is a scheme; relative paths have none.
        Some(colon) if !lower[..colon].contains(['/', '?', '#']) => {
            matches!(&lower[..colon], "http" | "https" | "mailto" | "tel" | "ftp")
        }
        _ => true,
    };
    if allowed {
        CowStr::Boxed(trimmed.to_string().into_boxed_str())
    } else {
        CowStr::Borrowed("#")
    }
}

#[cfg(test)]
mod tests {
    use super::render_markdown;

    #[test]
    fn headings_and_emphasis_render() {
        let html = render_markdown("# Title\n\nsome **bold** and *italic*");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn lists_and_checkboxes_render() {
        let html = render_markdown("- [ ] todo\n- [x] done\n\n1. one");
        assert!(html.contains("type=\"checkbox\""));
        assert!(html.contains("checked"));
        assert!(html.contains("<ol>"));
    }

    #[test]
    fn tables_render() {
        let html = render_markdown("| a | b |\n| - | - |\n| 1 | 2 |");
        assert!(html.contains("<table>"));
        assert!(html.contains("<td>1</td>"));
    }

    #[test]
    fn raw_html_is_shown_as_text_not_executed() {
        let html = render_markdown("<script>alert('x')</script>\n\n<img src=x onerror=y>");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<img src=x"));
    }

    #[test]
    fn javascript_links_are_neutralized() {
        let html = render_markdown("[click](javascript:alert(1))");
        assert!(!html.contains("javascript:"));
        assert!(html.contains("href=\"#\""));
    }

    #[test]
    fn ordinary_links_and_images_survive() {
        let html = render_markdown("[ok](https://example.com \"t\") ![pic](images/a.png)");
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("src=\"images/a.png\""));
    }

    #[test]
    fn code_blocks_are_escaped() {
        let html = render_markdown("```\n<b>not bold</b>\n```");
        assert!(html.contains("&lt;b&gt;"));
    }
}
