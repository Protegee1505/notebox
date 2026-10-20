//! Dioxus UI for web, desktop and Android.
//!
//! Phase 0 keeps this to the smallest possible app so the S1 boot spike
//! measures the framework, not the product.

#![forbid(unsafe_code)]

use dioxus::prelude::*;

/// Root component. Replaced by the app shell in Phase 4.
#[component]
pub fn App() -> Element {
    rsx! {
        main {
            id: "app",
            h1 { "Notebox" }
            p { "Bootstrap build. No features yet." }
        }
    }
}
