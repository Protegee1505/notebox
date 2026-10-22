//! Open web and email links outside the app.

use dioxus::prelude::*;

/// Open a destination in the system browser.
///
/// Returns `false` for a destination the allowlist rejects. On the web this
/// uses a new tab. On desktop and mobile the WebView's navigation handler
/// intercepts `location.href` and hands the URL to the system browser, so the
/// app itself never navigates away.
pub fn open_external(url: &str) -> bool {
    let Some(target) = notes_core::external_url(url) else {
        return false;
    };
    let Ok(literal) = serde_json::to_string(&target) else {
        return false;
    };
    #[cfg(target_arch = "wasm32")]
    let script = format!("window.open({literal}, '_blank', 'noopener,noreferrer');");
    #[cfg(not(target_arch = "wasm32"))]
    let script = format!("window.location.href = {literal};");
    let _ = document::eval(&script);
    true
}
