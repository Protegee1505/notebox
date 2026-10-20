//! A small transient message above the status bar.

use dioxus::prelude::*;

use crate::state::{AppContext, ToastKind};
use crate::ui::icons::Icon;

#[component]
pub fn ToastView() -> Element {
    let context = use_context::<AppContext>();
    let state = context.state.read();
    let toast = state.toast.clone();
    drop(state);
    let Some(toast) = toast else {
        return rsx! {};
    };
    let (class, icon) = match toast.kind {
        ToastKind::Info => ("toast is-info", "sparkle"),
        ToastKind::Success => ("toast is-success", "check"),
        ToastKind::Danger => ("toast is-danger", "x"),
    };
    rsx! {
        div { class: "{class}", role: "status", aria_live: "polite",
            span { class: "toast-icon", Icon { name: icon } }
            span { class: "toast-message", "{toast.message}" }
            button {
                class: "icon-button small",
                r#type: "button",
                aria_label: "Dismiss message",
                onclick: {
                    let context = context.clone();
                    move |_| context.state.write_unchecked().toast = None
                },
                Icon { name: "x" }
            }
        }
    }
}
