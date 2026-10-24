//! Android platform bridge.
//!
//! `MainActivity.kt` owns the Android APIs and exposes a tiny
//! `NoteboxAndroid` JavaScript interface. This module talks to that interface
//! through Dioxus `document::eval`, so the app keeps `forbid(unsafe_code)` and
//! does not need a JNI callback layer of its own.
//!
//! The bridge handles three things the WebView cannot do by itself:
//! selecting a file with the Storage Access Framework, creating an export
//! document, and translating the system Back gesture into the app's normal
//! [`AppContext::escape`] handling.

use std::path::{Path, PathBuf};

use dioxus::prelude::document;
use serde::Deserialize;

use crate::state::AppContext;

/// One message emitted by the Kotlin side of the bridge.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Event {
    /// A picked image was copied into the app cache.
    Image { path: String, name: String },
    /// A picked backup was copied into the app cache.
    Backup { path: String, name: String },
    /// A create-document request finished.
    Saved { name: String, action: String },
    /// The user left a picker or save dialog without choosing anything.
    Cancel { action: String },
    /// Kotlin could not start or finish the operation.
    Error { message: String },
    /// The system Back gesture was handled by the WebView.
    Back,
    /// The activity is going to the background.
    Pause,
}

/// Open the system document picker for an image.
pub fn pick_image() {
    eval("window.NoteboxAndroid && window.NoteboxAndroid.pick('image');");
}

/// Open the system document picker for a Notebox backup.
pub fn pick_backup() {
    eval("window.NoteboxAndroid && window.NoteboxAndroid.pick('backup');");
}

/// Ask Android to save a cache file through the create-document picker.
///
/// The file is written to the app cache first so the Kotlin side can copy it
/// without passing image or backup bytes through JavaScript. `action` lets the
/// result handler choose the right toast.
pub fn save_file(path: &Path, name: &str, mime: &str, action: &str) {
    let path = js_string(&path.to_string_lossy());
    let name = js_string(name);
    let mime = js_string(mime);
    let action = js_string(action);
    eval(&format!(
        "window.NoteboxAndroid && window.NoteboxAndroid.save({path}, {name}, {mime}, {action});"
    ));
}

/// Keep the JavaScript bridge alive and dispatch its messages.
///
/// The evaluated script never resolves; that keeps Dioxus's eval channel open
/// so the interval and event listeners can call `dioxus.send` later. The
/// interval is installed once per page and uses the current eval sender, which
/// makes a Dioxus hot reload replace the receiver without stacking timers.
pub async fn listen(context: AppContext) {
    let mut eval = document::eval(
        r#"
        (() => {
          window.__noteboxAndroidSend = (value) => {
            try { dioxus.send(value); } catch (_) {}
          };
          if (!window.__noteboxAndroidPoll) {
            window.__noteboxAndroidPoll = setInterval(() => {
              try {
                const value = window.NoteboxAndroid && window.NoteboxAndroid.takeResult();
                if (value) window.__noteboxAndroidSend(value);
              } catch (_) {}
            }, 250);
          }
          window.__noteboxHandleBack = () => {
            if (!window.__noteboxCanGoBack) return false;
            window.__noteboxAndroidSend(JSON.stringify({ kind: 'back' }));
            return true;
          };
          if (!window.__noteboxPauseBound) {
            window.__noteboxPauseBound = true;
            window.addEventListener('notebox-pause', () => {
              window.__noteboxAndroidSend(JSON.stringify({ kind: 'pause' }));
            });
            document.addEventListener('visibilitychange', () => {
              if (document.visibilityState === 'hidden') {
                window.__noteboxAndroidSend(JSON.stringify({ kind: 'pause' }));
              }
            });
          }
        })();
        await new Promise(() => {});
        "#,
    );

    while let Ok(raw) = eval.recv::<String>().await {
        handle(&context, &raw);
    }
}

/// Update the JavaScript flag Android reads before deciding whether Back
/// should close an overlay or finish the activity.
pub fn set_can_go_back(can_go_back: bool) {
    eval(&format!("window.__noteboxCanGoBack = {can_go_back};"));
}

fn handle(context: &AppContext, raw: &str) {
    let event = match serde_json::from_str::<Event>(raw) {
        Ok(event) => event,
        Err(error) => {
            context.toast_danger(format!("Android bridge error: {error}"));
            return;
        }
    };

    match event {
        Event::Image { path, name } => {
            context.attach_image_paths(vec![(name, PathBuf::from(path))]);
        }
        Event::Backup { path, name } => {
            context.choose_restore_path(name, PathBuf::from(path));
        }
        Event::Saved { name, action } => {
            let verb = if action == "export" {
                "Exported"
            } else {
                "Saved"
            };
            context.toast_success(format!("{verb} {name}"));
        }
        Event::Cancel { .. } => {}
        Event::Error { message } => context.toast_danger(message),
        Event::Back => context.android_back(),
        Event::Pause => context.save_now(),
    }
}

fn eval(script: &str) {
    let _ = document::eval(script);
}

fn js_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}
