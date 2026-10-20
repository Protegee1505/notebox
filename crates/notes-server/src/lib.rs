//! Server handlers behind a [`Storage`] trait, plus the Cloudflare Worker
//! adapter for `wasm32`.
//!
//! The handler logic stays platform-neutral so the same crate can run as an
//! axum server if the Worker free tier ever stops fitting. Only the adapter
//! module may depend on `worker`.

#![forbid(unsafe_code)]

/// Durable server-side storage contract.
///
/// The D1 implementation and an in-memory test implementation both implement
/// this trait. Methods arrive with Phase 2.
pub trait Storage {}
