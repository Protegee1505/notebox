//! Server handlers behind a [`Storage`] trait, plus the Cloudflare Worker
//! adapter for `wasm32`.
//!
//! The handler logic stays platform-neutral so the same crate can run as an
//! axum server if the Worker free tier ever stops fitting. Only the D1 and
//! Worker modules depend on `worker`.

#![forbid(unsafe_code)]

mod auth;
mod error;
mod memory;
mod service;
mod storage;

pub use auth::{
    backoff_seconds, constant_time_eq, new_token, sha256_hex, LOGIN_BACKOFF_BASE_SECS,
    LOGIN_BACKOFF_MAX_SECS, LOGIN_RATE_LIMIT_AFTER,
};
pub use error::{ServerError, StorageError};
pub use memory::MemoryStorage;
pub use service::{Server, ServerConfig, DEVICE_TOUCH_INTERVAL_SECS, SYNC_PAGE};
pub use storage::{AccountRow, BlobMetaRow, DeviceRow, NewUpdate, Storage, UpdateRow};

#[cfg(target_arch = "wasm32")]
mod d1;
#[cfg(target_arch = "wasm32")]
mod worker_http;

#[cfg(target_arch = "wasm32")]
pub use d1::D1Storage;

/// Worker fetch entry point.
///
/// Only the adapter module is platform-specific; everything below it is the
/// same code the native tests drive.
#[cfg(target_arch = "wasm32")]
#[worker::event(fetch)]
pub async fn fetch(
    req: worker::Request,
    env: worker::Env,
    _ctx: worker::Context,
) -> worker::Result<worker::Response> {
    worker_http::handle(req, env).await
}
