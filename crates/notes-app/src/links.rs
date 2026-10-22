//! Open web and email links outside the app.

use dioxus::prelude::*;

/// Open the JavaScript string variable `url` in a new tab.
///
/// Firefox runs queued microtasks (including a pending Dioxus render) inside
/// `window.open`. That render re-enters the event dispatch while Dioxus still
/// holds its element arena borrow, which panics. A temporary anchor click is
/// still a user gesture, so the browser permits the tab, but it does not run
/// the microtask queue before the event handler returns.
pub(crate) const OPEN_URL_JS: &str = "\
  const anchor = document.createElement('a');\
  anchor.href = url;\
  anchor.target = '_blank';\
  anchor.rel = 'noopener noreferrer';\
  anchor.style.display = 'none';\
  document.body.appendChild(anchor);\
  anchor.click();\
  anchor.remove();";

/// Open a destination in the system browser.
///
/// Returns `false` for a destination the allowlist rejects. On the web this
/// opens a new tab from a temporary anchor. On desktop and mobile the WebView's
/// navigation handler intercepts `location.href` and hands the URL to the
/// system browser, so the app itself never navigates away.
pub fn open_external(url: &str) -> bool {
    let Some(target) = notes_core::external_url(url) else {
        return false;
    };
    let Ok(literal) = serde_json::to_string(&target) else {
        return false;
    };
    #[cfg(target_arch = "wasm32")]
    let script = format!("const url = {literal};\n{OPEN_URL_JS}\n");
    #[cfg(not(target_arch = "wasm32"))]
    let script = format!("window.location.href = {literal};");
    let _ = document::eval(&script);
    true
}
