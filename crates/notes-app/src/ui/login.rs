//! The account screen.

use dioxus::prelude::*;

use crate::state::{AppContext, Screen};
use crate::ui::icons::Icon;

#[component]
pub fn LoginScreen() -> Element {
    let context = use_context::<AppContext>();
    let state = context.state.read();
    let username = state.login.username.clone();
    let passphrase = state.login.passphrase.clone();
    let server = state.login.server.clone();
    let device = state.login.device.clone();
    let error = state.login.error.clone();
    let connecting = state.sync.status.busy();
    drop(state);
    let local_note = if cfg!(target_arch = "wasm32") {
        "Signed in, this tab keeps a working copy and syncs when it can. A reload pulls your notes back from your Worker."
    } else {
        "Signed in, this device keeps a full local copy and syncs when it can. Signing out never deletes a note."
    };

    rsx! {
        main { class: "login-screen",
            section { class: "login-brand",
                div { class: "login-brand-top",
                    span { class: "brand-mark large", "nb" }
                    span { class: "brand-name", "Notebox" }
                }
                div { class: "login-pitch",
                    h1 { "Every note, everywhere, even offline." }
                    p {
                        "A quiet home for writing. One account, a full copy on each device, and a history you can go back through."
                    }
                    ul { class: "feature-list",
                        li {
                            span { class: "feature-icon", Icon { name: "sparkle" } }
                            div {
                                strong { "Local-first by design" }
                                span { "Write on a plane. Merge when you land." }
                            }
                        }
                        li {
                            span { class: "feature-icon", Icon { name: "clock" } }
                            div {
                                strong { "Full edit history" }
                                span { "Every version stays; restore without losing the rest." }
                            }
                        }
                        li {
                            span { class: "feature-icon", Icon { name: "database" } }
                            div {
                                strong { "Yours to keep" }
                                span { "SQLite on the device, a free Worker for sync." }
                            }
                        }
                    }
                }
                p { class: "login-footnote", "Your passphrase is stretched on this device. The server never sees it." }
            }

            section { class: "login-panel",
                button {
                    class: "icon-button login-close",
                    r#type: "button",
                    aria_label: "Back to notes",
                    title: "Back to notes",
                    disabled: connecting,
                    onclick: {
                        let context = context.clone();
                        move |_| context.go(Screen::Notes)
                    },
                    Icon { name: "arrow-left" }
                }
                div { class: "login-form-wrap",
                    div { class: "login-heading",
                        h2 { "Sign in" }
                        p { "Use the username and passphrase you chose when you set up your Worker." }
                    }
                    if let Some(message) = error {
                        div { class: "form-error", role: "alert",
                            span { "{message}" }
                        }
                    }
                    form {
                        class: "login-form",
                        aria_busy: connecting,
                        onsubmit: {
                            let context = context.clone();
                            move |event| {
                                event.prevent_default();
                                if !connecting {
                                    context.submit_login();
                                }
                            }
                        },
                        label { class: "field",
                            span { "Username" }
                            input {
                                class: "input",
                                r#type: "text",
                                autocomplete: "username",
                                placeholder: "your name",
                                value: "{username}",
                                disabled: connecting,
                                oninput: {
                                    let context = context.clone();
                                    move |event| context.state.write_unchecked().login.username = event.value()
                                },
                            }
                        }
                        label { class: "field",
                            span { "Passphrase" }
                            input {
                                class: "input",
                                r#type: "password",
                                autocomplete: "current-password",
                                placeholder: "six random words",
                                value: "{passphrase}",
                                disabled: connecting,
                                oninput: {
                                    let context = context.clone();
                                    move |event| context.state.write_unchecked().login.passphrase = event.value()
                                },
                            }
                        }
                        label { class: "field",
                            span { "Worker URL" }
                            input {
                                class: "input",
                                r#type: "url",
                                autocomplete: "url",
                                placeholder: "https://notebox.you.workers.dev",
                                value: "{server}",
                                disabled: connecting,
                                oninput: {
                                    let context = context.clone();
                                    move |event| context.state.write_unchecked().login.server = event.value()
                                },
                            }
                        }
                        label { class: "field",
                            span { "Device name" }
                            input {
                                class: "input",
                                r#type: "text",
                                autocomplete: "off",
                                placeholder: "This device",
                                value: "{device}",
                                disabled: connecting,
                                oninput: {
                                    let context = context.clone();
                                    move |event| context.state.write_unchecked().login.device = event.value()
                                },
                            }
                        }
                        button {
                            class: "primary-button wide",
                            r#type: "submit",
                            disabled: connecting,
                            if connecting { "Connecting…" } else { "Connect" }
                        }
                    }
                    div { class: "login-divider", span { "or" } }
                    button {
                        class: "secondary-button wide",
                        r#type: "button",
                        disabled: connecting,
                        onclick: {
                            let context = context.clone();
                            move |_| context.continue_local()
                        },
                        "Continue on this device"
                    }
                    p { class: "login-note", "{local_note}" }
                }
            }
        }
    }
}
