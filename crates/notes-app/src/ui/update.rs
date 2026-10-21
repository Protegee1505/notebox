//! The "new version ready" strip above the workspace.

use dioxus::prelude::*;

use crate::state::AppContext;
use crate::ui::icons::Icon;

#[component]
pub fn UpdateBanner() -> Element {
    let context = use_context::<AppContext>();
    let state = context.state.read();
    let applying = state.update_applying;
    drop(state);

    rsx! {
        aside { class: "update-banner", role: "status", aria_live: "polite",
            span { class: "update-badge", Icon { name: "refresh" } }
            div { class: "update-copy",
                strong { "A new version of Notebox is ready." }
                span { "Your notes are saved on this device. Reload when you are ready." }
            }
            div { class: "update-actions",
                button {
                    class: "primary-button",
                    r#type: "button",
                    disabled: applying,
                    onclick: {
                        let context = context.clone();
                        move |_| context.apply_update()
                    },
                    if applying { "Reloading…" } else { "Reload" }
                }
                button {
                    class: "text-button",
                    r#type: "button",
                    disabled: applying,
                    onclick: {
                        let context = context.clone();
                        move |_| context.dismiss_update()
                    },
                    "Later"
                }
            }
        }
    }
}
