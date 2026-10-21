//! Bridge to the service worker that makes the web build installable.
//!
//! `index.html` owns registration and the browser lifecycle. It publishes
//! `window.noteboxPwa` with a `ready` flag and an `applyUpdate` function, and
//! fires `notebox-update-ready` when a new worker is waiting. Rust only mirrors
//! that state and calls the function when the user chooses to reload.

use dioxus::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::state::AppContext;

/// Listen for the update-ready event and pick up a worker that was already
/// waiting before the app mounted.
pub fn listen(context: AppContext) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let event_context = context.clone();
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
        event_context.mark_update_ready();
    });
    let callback: &js_sys::Function = closure.as_ref().unchecked_ref();
    if window
        .add_event_listener_with_callback("notebox-update-ready", callback)
        .is_ok()
    {
        // The listener lives for the page, like the app root itself.
        closure.forget();
    }

    if update_is_ready() {
        context.mark_update_ready();
    }
}

/// Ask the waiting worker to activate; the page script reloads on
/// `controllerchange`.
pub fn apply_update() {
    let _ = document::eval("window.noteboxPwa?.applyUpdate?.();");
}

/// Whether `index.html` already saw a waiting worker.
fn update_is_ready() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(bridge) = js_sys::Reflect::get(&window, &js_sys::JsString::from("noteboxPwa")) else {
        return false;
    };
    if bridge.is_undefined() || bridge.is_null() {
        return false;
    }
    js_sys::Reflect::get(&bridge, &js_sys::JsString::from("ready"))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}
