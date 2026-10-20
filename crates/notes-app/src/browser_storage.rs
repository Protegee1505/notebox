//! Browser database images in OPFS.
//!
//! SQLite runs in memory on the web. This module owns the small worker that
//! loads and saves a full database image, so a reload brings back local-only
//! notes, history and attachment bytes without an async `NoteStore`.
//!
//! The worker also takes a Web Lock. A second tab that cannot get the lock
//! shows the "already open" screen instead of writing over the first tab's
//! image.

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::{asset, manganis, Asset};
use futures_channel::oneshot;
use js_sys::Function;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{ErrorEvent, MessageEvent, Worker};

use notes_store::SqliteStore;

const WORKER_ASSET: Asset = asset!("/assets/opfs_store.js");

/// Why the OPFS store could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Another tab holds the store lock.
    Locked,
    /// OPFS or the worker is missing. The app falls back to memory.
    Unavailable(String),
}

enum LoadReply {
    Loaded { bytes: Vec<u8>, persisted: bool },
    Locked,
    Unavailable(String),
}

#[derive(Default)]
struct LoadSlot {
    reply: Option<LoadReply>,
    sender: Option<oneshot::Sender<()>>,
}

impl LoadSlot {
    fn resolve(&mut self, reply: LoadReply) {
        if self.reply.is_some() {
            return;
        }
        self.reply = Some(reply);
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(());
        }
    }
}

#[derive(Default)]
struct WorkerState {
    error: Option<String>,
    in_flight: bool,
    last_changes: u64,
}

/// The loaded worker, before it is attached to a store.
pub struct BrowserWorker {
    worker: Worker,
    _on_message: Function,
    _on_error: Function,
    state: Rc<RefCell<WorkerState>>,
}

/// An OPFS image writer attached to one live store.
pub struct BrowserPersistence {
    worker: Worker,
    _on_message: Function,
    _on_error: Function,
    state: Rc<RefCell<WorkerState>>,
    store: Rc<SqliteStore>,
}

/// Start the worker and read the saved image, if any.
pub async fn load() -> Result<(BrowserWorker, Vec<u8>, bool), LoadError> {
    let url = WORKER_ASSET.to_string();
    let worker = Worker::new(&url).map_err(|error| {
        LoadError::Unavailable(format!(
            "could not start the browser storage worker: {error:?}"
        ))
    })?;

    let state = Rc::new(RefCell::new(WorkerState::default()));
    let slot = Rc::new(RefCell::new(LoadSlot::default()));

    let on_message = Closure::<dyn FnMut(MessageEvent)>::new({
        let state = state.clone();
        let slot = slot.clone();
        move |event: MessageEvent| {
            let data = event.data();
            match string_field(&data, "type").as_deref() {
                Some("loaded") => {
                    let bytes = bytes_field(&data, "bytes");
                    let persisted = bool_field(&data, "persisted");
                    slot.borrow_mut()
                        .resolve(LoadReply::Loaded { bytes, persisted });
                }
                Some("locked") => {
                    slot.borrow_mut().resolve(LoadReply::Locked);
                }
                Some("unavailable") => {
                    let message = string_field(&data, "message")
                        .unwrap_or_else(|| "browser storage is unavailable".to_string());
                    slot.borrow_mut()
                        .resolve(LoadReply::Unavailable(message.clone()));
                    state.borrow_mut().error = Some(message);
                }
                Some("saved") => {
                    state.borrow_mut().in_flight = false;
                }
                Some("error") => {
                    let message = string_field(&data, "message")
                        .unwrap_or_else(|| "could not save to browser storage".to_string());
                    let mut state = state.borrow_mut();
                    state.error = Some(message);
                    state.in_flight = false;
                }
                _ => {}
            }
        }
    });

    let on_error = Closure::<dyn FnMut(ErrorEvent)>::new({
        let state = state.clone();
        let slot = slot.clone();
        move |event: ErrorEvent| {
            let message = if event.message().is_empty() {
                "the browser storage worker failed".to_string()
            } else {
                event.message()
            };
            state.borrow_mut().error = Some(message.clone());
            slot.borrow_mut().resolve(LoadReply::Unavailable(message));
        }
    });

    let on_message = function_from(on_message).map_err(LoadError::Unavailable)?;
    let on_error = function_from(on_error).map_err(LoadError::Unavailable)?;
    worker.set_onmessage(Some(&on_message));
    worker.set_onerror(Some(&on_error));

    let (sender, receiver) = oneshot::channel();
    slot.borrow_mut().sender = Some(sender);
    post_load(&worker).map_err(LoadError::Unavailable)?;

    receiver.await.map_err(|_| {
        LoadError::Unavailable("the browser storage worker stopped before loading".to_string())
    })?;
    let reply =
        slot.borrow_mut().reply.take().ok_or_else(|| {
            LoadError::Unavailable("the browser storage worker sent no reply".into())
        })?;

    match reply {
        LoadReply::Loaded { bytes, persisted } => Ok((
            BrowserWorker {
                worker,
                _on_message: on_message,
                _on_error: on_error,
                state,
            },
            bytes,
            persisted,
        )),
        LoadReply::Locked => Err(LoadError::Locked),
        LoadReply::Unavailable(message) => Err(LoadError::Unavailable(message)),
    }
}

impl BrowserWorker {
    /// Attach this worker to the store it should save.
    pub fn attach(self, store: Rc<SqliteStore>) -> Rc<BrowserPersistence> {
        self.state.borrow_mut().last_changes = store.change_count();
        Rc::new(BrowserPersistence {
            worker: self.worker,
            _on_message: self._on_message,
            _on_error: self._on_error,
            state: self.state,
            store,
        })
    }
}

impl BrowserPersistence {
    /// Serialize and send the database when it has changed.
    ///
    /// Normally at most one image is in flight; writes during a save are
    /// picked up on the next call after the worker acknowledges it. `force`
    /// is for the page-hide path, where there may be no next call: it sends
    /// the newer image even if an older one is still being written. The
    /// worker's write chain keeps them ordered.
    pub fn flush_if_needed(&self, force: bool) -> Result<(), String> {
        let changes = self.store.change_count();
        {
            let state = self.state.borrow();
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            if changes == state.last_changes || (state.in_flight && !force) {
                return Ok(());
            }
        }

        let image = self
            .store
            .to_image()
            .map_err(|error| format!("could not serialize the browser database: {error}"))?;
        let array = js_sys::Uint8Array::from(image.as_slice());
        let buffer = array.buffer();
        let message = js_sys::Object::new();
        set_field(&message, "type", &JsValue::from_str("save"))?;
        set_field(&message, "bytes", &buffer)?;
        let transfer = js_sys::Array::new();
        transfer.push(&buffer);
        self.worker
            .post_message_with_transfer(&message, &transfer)
            .map_err(|error| format!("could not send the browser database image: {error:?}"))?;

        let mut state = self.state.borrow_mut();
        state.in_flight = true;
        state.last_changes = changes;
        Ok(())
    }
}

fn post_load(worker: &Worker) -> Result<(), String> {
    let message = js_sys::Object::new();
    set_field(&message, "type", &JsValue::from_str("load"))?;
    worker
        .post_message(&message)
        .map_err(|error| format!("could not ask for the saved database: {error:?}"))
}

fn set_field(object: &js_sys::Object, key: &str, value: &JsValue) -> Result<(), String> {
    js_sys::Reflect::set(object, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|error| format!("could not build a browser storage message: {error:?}"))
}

fn string_field(value: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(value, &JsValue::from_str(key))
        .ok()
        .and_then(|value| value.as_string())
}

fn bool_field(value: &JsValue, key: &str) -> bool {
    js_sys::Reflect::get(value, &JsValue::from_str(key))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn bytes_field(value: &JsValue, key: &str) -> Vec<u8> {
    let Some(value) = js_sys::Reflect::get(value, &JsValue::from_str(key)).ok() else {
        return Vec::new();
    };
    if let Ok(buffer) = value.clone().dyn_into::<js_sys::ArrayBuffer>() {
        return js_sys::Uint8Array::new(&buffer).to_vec();
    }
    if let Ok(array) = value.dyn_into::<js_sys::Uint8Array>() {
        return array.to_vec();
    }
    Vec::new()
}

fn function_from<T>(closure: Closure<T>) -> Result<Function, String>
where
    T: ?Sized + wasm_bindgen::closure::WasmClosure + 'static,
{
    closure
        .into_js_value()
        .dyn_into::<Function>()
        .map_err(|_| "the browser storage callback was not a function".to_string())
}
