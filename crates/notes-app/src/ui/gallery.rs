//! The open note's attachment grid and full-image viewer.

use dioxus::prelude::*;

use crate::media::{attachment_kind, format_size, is_previewable_image};
use crate::state::{note_title, AppContext};
use crate::ui::icons::Icon;

#[component]
pub fn AttachmentGallery() -> Element {
    let context = use_context::<AppContext>();
    let state = context.state.read();
    let attachments = state.attachments.clone();
    let urls = state.attachment_urls.clone();
    let index = state.gallery_index.min(attachments.len().saturating_sub(1));
    let confirm = state.gallery_confirm_delete;
    let note_count = state.gallery_note_count;
    let deleting = state.attachment_deleting.clone();
    let connected = state.profile.sync_connected;
    let title = state
        .selected
        .and_then(|id| state.notes.iter().find(|note| note.id == id))
        .map(note_title)
        .unwrap_or_else(|| "Note".to_string());
    drop(state);

    let Some(attachment) = attachments.get(index).cloned() else {
        return rsx! {};
    };
    let total = attachments.len();
    let url = urls.get(&attachment.hash).cloned();
    let previewable = is_previewable_image(&attachment.mime);
    let deleting_this = deleting.as_deref() == Some(attachment.hash.as_str());
    let busy = deleting.is_some();
    let main_url = url.clone().unwrap_or_default();
    let confirm_open = confirm;

    rsx! {
        section {
            class: "attachment-gallery",
            role: "dialog",
            aria_label: "Attachments",
            aria_modal: "true",
            onkeydown: {
                let context = context.clone();
                move |event| {
                    if event.key() == Key::Tab {
                        event.prevent_default();
                        let backwards = event.modifiers().contains(Modifiers::SHIFT);
                        crate::ui::focus::trap_tab(".attachment-gallery", backwards);
                        return;
                    }
                    let handled = match event.key() {
                        Key::Escape => {
                            if confirm_open {
                                context.cancel_attachment_delete();
                            } else {
                                context.close_attachment_gallery();
                            }
                            true
                        }
                        Key::ArrowLeft => {
                            context.step_gallery(-1);
                            true
                        }
                        Key::ArrowRight => {
                            context.step_gallery(1);
                            true
                        }
                        Key::Home => {
                            context.select_gallery_index(0);
                            true
                        }
                        Key::End => {
                            context.select_gallery_index(total.saturating_sub(1));
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        event.prevent_default();
                        event.stop_propagation();
                    }
                }
            },

            header { class: "gallery-header",
                div { class: "gallery-heading",
                    div { class: "gallery-mark", Icon { name: "image" } }
                    div {
                        h2 { "Attachments" }
                        p { "{title}" }
                    }
                }
                span { class: "gallery-position", "{index + 1} of {total}" }
                button {
                    id: "gallery-close",
                    class: "icon-button",
                    r#type: "button",
                    aria_label: "Close attachment viewer",
                    title: "Close",
                    autofocus: true,
                    onmounted: move |_| {
                        let _ = document::eval(
                            "requestAnimationFrame(() => document.getElementById('gallery-close')?.focus());",
                        );
                    },
                    onclick: {
                        let context = context.clone();
                        move |_| context.close_attachment_gallery()
                    },
                    Icon { name: "x" }
                }
            }

            div { class: "gallery-body",
                div { class: "gallery-stage",
                    div { class: "gallery-canvas",
                        if previewable && attachment.available && url.is_some() {
                            img {
                                class: "gallery-main-image",
                                src: "{main_url}",
                                alt: "{attachment.name}",
                            }
                        } else if previewable {
                            div { class: "gallery-placeholder",
                                Icon { name: "image" }
                                strong {
                                    if attachment.available {
                                        "This image cannot be previewed"
                                    } else {
                                        "Waiting for this image"
                                    }
                                }
                                span {
                                    if attachment.available {
                                        "The file is on this device but its preview could not be built. Download it to open it."
                                    } else {
                                        "The file is linked in this note but its bytes have not arrived yet. It will download with the next sync."
                                    }
                                }
                            }
                        } else {
                            div { class: "gallery-placeholder",
                                Icon { name: "note" }
                                strong { "{attachment.name}" }
                                span { "{attachment_kind(&attachment.mime)} \u{00b7} {format_size(attachment.size)}" }
                                span { "This file type opens in its own app. Download it to view it." }
                            }
                        }

                        if total > 1 {
                            button {
                                class: "gallery-step is-previous",
                                r#type: "button",
                                aria_label: "Previous attachment",
                                title: "Previous",
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.step_gallery(-1)
                                },
                                Icon { name: "chevron-left" }
                            }
                            button {
                                class: "gallery-step is-next",
                                r#type: "button",
                                aria_label: "Next attachment",
                                title: "Next",
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.step_gallery(1)
                                },
                                Icon { name: "chevron-right" }
                            }
                        }
                    }

                    div { class: "gallery-meta",
                        div { class: "gallery-file",
                            strong { "{attachment.name}" }
                            span {
                                "{attachment_kind(&attachment.mime)} \u{00b7} {format_size(attachment.size)}"
                                if !attachment.available {
                                    " \u{00b7} not downloaded"
                                }
                            }
                        }
                        span { class: "gallery-used",
                            if note_count == 1 {
                                "Used in this note"
                            } else {
                                "Used in {note_count} notes"
                            }
                        }
                    }
                }

                aside { class: "gallery-sidebar", aria_label: "Attachment controls",
                    div { class: "gallery-grid", role: "listbox", aria_label: "Attachments in this note",
                        for (item_index, item) in attachments.iter().enumerate() {
                            button {
                                key: "{item.hash}",
                                class: if item_index == index { "gallery-thumb is-selected" } else { "gallery-thumb" },
                                r#type: "button",
                                role: "option",
                                aria_selected: if item_index == index { "true" } else { "false" },
                                aria_label: "Show {item.name}",
                                title: "{item.name}",
                                onclick: {
                                    let context = context.clone();
                                    move |_| context.select_gallery_index(item_index)
                                },
                                if let Some(url) = urls.get(&item.hash).filter(|_| is_previewable_image(&item.mime)) {
                                    img {
                                        class: "gallery-thumb-image",
                                        src: "{url}",
                                        alt: "",
                                        loading: "lazy",
                                    }
                                } else if is_previewable_image(&item.mime) {
                                    span { class: "gallery-thumb-fallback", Icon { name: "image" } }
                                } else {
                                    span { class: "gallery-thumb-fallback", Icon { name: "note" } }
                                }
                            }
                        }
                    }

                    div { class: "gallery-actions",
                        button {
                            class: "secondary-button",
                            r#type: "button",
                            disabled: !attachment.available || busy,
                            title: if attachment.available { "Save this file to your device" } else { "This file has not downloaded yet" },
                            onclick: {
                                let context = context.clone();
                                let attachment = attachment.clone();
                                move |_| context.download_attachment(attachment.clone())
                            },
                            Icon { name: "download" }
                            span { "Download" }
                        }
                        button {
                            class: "secondary-button",
                            r#type: "button",
                            disabled: busy || note_count <= 1,
                            title: if note_count <= 1 {
                                "This is the only note that uses the file. Delete removes it from the device and the cloud."
                            } else {
                                "Remove this image from this note and keep its file for the other notes"
                            },
                            onclick: {
                                let context = context.clone();
                                let attachment = attachment.clone();
                                move |_| context.remove_attachment_from_note(attachment.clone())
                            },
                            Icon { name: "x" }
                            span { "Remove from note" }
                        }
                        button {
                            id: "gallery-delete",
                            class: "danger-button",
                            r#type: "button",
                            disabled: busy,
                            title: "Delete this file from this note, this device and the cloud",
                            onclick: {
                                let context = context.clone();
                                move |_| context.request_attachment_delete()
                            },
                            Icon { name: "trash" }
                            span { "Delete" }
                        }
                    }

                    if deleting_this {
                        div { class: "gallery-busy", role: "status",
                            span { class: "save-dot is-dirty" }
                            "Deleting the cloud copy..."
                        }
                    }

                    if confirm {
                        div { class: "gallery-confirm", role: "alert",
                            strong { "Delete {attachment.name}?" }
                            span {
                                if connected {
                                    "This removes it from this note, this device and the cloud. Older versions of this note will show a missing image."
                                } else {
                                    "This removes it from this note and this device. Older versions of this note will show a missing image."
                                }
                            }
                            if note_count > 1 {
                                span { class: "gallery-confirm-warning",
                                    "It is used in {note_count} notes. Remove it from the others first."
                                }
                            }
                            div { class: "gallery-confirm-actions",
                                button {
                                    class: "danger-button",
                                    r#type: "button",
                                    disabled: busy || note_count > 1,
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.confirm_attachment_delete()
                                    },
                                    "Delete permanently"
                                }
                                button {
                                    id: "gallery-delete-cancel",
                                    class: "text-button",
                                    r#type: "button",
                                    autofocus: true,
                                    onmounted: move |_| {
                                        let _ = document::eval(
                                            "requestAnimationFrame(() => document.getElementById('gallery-delete-cancel')?.focus());",
                                        );
                                    },
                                    onclick: {
                                        let context = context.clone();
                                        move |_| context.cancel_attachment_delete()
                                    },
                                    "Cancel"
                                }
                            }
                        }
                    } else if note_count <= 1 {
                        p { class: "gallery-note",
                            "There is no separate attachment library: Delete is the only way to remove the last copy."
                        }
                    } else {
                        p { class: "gallery-note",
                            "Remove from note keeps the file for the other notes. Only Delete removes its bytes."
                        }
                    }
                }
            }
        }
    }
}
