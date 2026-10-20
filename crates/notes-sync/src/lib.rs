//! Sync engine, device login and the HTTP transport.
//!
//! The transport sits behind a trait so the engine can be tested against an
//! in-memory server before the real Worker is involved.

#![forbid(unsafe_code)]
