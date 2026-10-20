//! The native HTTP transport: one blocking `ureq` agent.
//!
//! Everything here mirrors `docs/API.md`: `POST /v1/auth/setup` with the setup
//! token header, everything else with a bearer token, every body wrapped in
//! [`Envelope`]. TLS is rustls, ureq's default, never OpenSSL — OpenSSL breaks
//! the Android cross-compile.
//!
//! The calls block. A caller on a UI thread must drive the engine on a worker
//! thread; the browser build gets its own transport in P7 and does not use
//! this file at all.

use std::time::Duration;

use async_trait::async_trait;
use notes_proto::{
    AuthRequest, AuthResponse, BlobChunk, BlobStored, DeviceList, Envelope, SyncRequest,
    SyncResponse, PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;

use crate::error::server_error;
use crate::{SyncError, SyncTransport};
use ureq::http::Response;
use ureq::{Agent, Body, RequestBuilder};

/// How long one request may take before it counts as a network failure.
pub const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Largest response the client will read, in bytes.
///
/// A sync page can hold 100 updates and an attachment chunk is one MiB, both
/// of which base64 inflates by a third. 32 MiB leaves room for a full page of
/// large notes without letting a hostile server exhaust memory.
pub const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;

/// A Worker reached over HTTP.
pub struct HttpTransport {
    base: String,
    agent: Agent,
}

impl HttpTransport {
    /// A transport for a Worker URL such as `https://notebox.example.workers.dev`.
    pub fn new(base_url: &str) -> Result<Self, SyncError> {
        Self::with_timeout(base_url, Duration::from_secs(REQUEST_TIMEOUT_SECS))
    }

    /// Same, with an explicit per-request timeout.
    pub fn with_timeout(base_url: &str, timeout: Duration) -> Result<Self, SyncError> {
        let base = base_url.trim().trim_end_matches('/').to_string();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(SyncError::Invalid(format!(
                "server URL must start with http:// or https://: {base}"
            )));
        }
        let agent = Agent::config_builder()
            .timeout_global(Some(timeout))
            // Error bodies carry the code the engine needs, so a 4xx or 5xx
            // is a response to read, not an error to raise.
            .http_status_as_error(false)
            .build()
            .new_agent();
        Ok(Self { base, agent })
    }

    /// The URL this transport talks to, without a trailing slash.
    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Send one request and decode its envelope.
    fn call<T: DeserializeOwned>(
        &self,
        response: Result<Response<Body>, ureq::Error>,
    ) -> Result<T, SyncError> {
        let mut response = response.map_err(network_error)?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|text| text.trim().parse::<i64>().ok());
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(network_error)?;
        if !(200..300).contains(&status) {
            return Err(server_error(status, retry_after, &bytes));
        }
        let envelope: Envelope<T> = serde_json::from_slice(&bytes)
            .map_err(|error| SyncError::Protocol(format!("status {status}: {error}")))?;
        if !envelope.is_current() {
            return Err(SyncError::ProtocolVersion {
                found: envelope.version,
                expected: PROTOCOL_VERSION,
            });
        }
        Ok(envelope.payload)
    }

    /// Attach the bearer token to a request in any state.
    fn bearer<S>(&self, request: RequestBuilder<S>, token: &str) -> RequestBuilder<S> {
        request.header("authorization", &format!("Bearer {token}"))
    }
}

#[async_trait(?Send)]
impl SyncTransport for HttpTransport {
    async fn setup(
        &self,
        setup_token: &str,
        request: &AuthRequest,
    ) -> Result<AuthResponse, SyncError> {
        let response = self
            .agent
            .post(self.url("/v1/auth/setup"))
            .header("x-setup-token", setup_token)
            .send_json(Envelope::new(request));
        self.call(response)
    }

    async fn login(&self, request: &AuthRequest) -> Result<AuthResponse, SyncError> {
        let response = self
            .agent
            .post(self.url("/v1/auth/login"))
            .send_json(Envelope::new(request));
        self.call(response)
    }

    async fn sync(&self, token: &str, request: &SyncRequest) -> Result<SyncResponse, SyncError> {
        let builder = self.bearer(self.agent.post(self.url("/v1/sync")), token);
        self.call(builder.send_json(Envelope::new(request)))
    }

    async fn put_blob(&self, token: &str, chunk: &BlobChunk) -> Result<BlobStored, SyncError> {
        let builder = self.bearer(
            self.agent
                .put(self.url(&format!("/v1/blob/{}", chunk.hash))),
            token,
        );
        self.call(builder.send_json(Envelope::new(chunk)))
    }

    async fn get_blob(&self, token: &str, hash: &str, index: u32) -> Result<BlobChunk, SyncError> {
        let builder = self.bearer(
            self.agent
                .get(self.url(&format!("/v1/blob/{hash}?index={index}"))),
            token,
        );
        self.call(builder.call())
    }

    async fn devices(&self, token: &str) -> Result<DeviceList, SyncError> {
        let builder = self.bearer(self.agent.get(self.url("/v1/devices")), token);
        self.call(builder.call())
    }

    async fn revoke_device(&self, token: &str, device_id: &str) -> Result<(), SyncError> {
        let builder = self.bearer(
            self.agent
                .delete(self.url(&format!("/v1/devices/{device_id}"))),
            token,
        );
        let mut response = builder.call().map_err(network_error)?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|text| text.trim().parse::<i64>().ok());
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_vec()
            .map_err(network_error)?;
        Err(server_error(status, retry_after, &bytes))
    }
}

fn network_error(error: ureq::Error) -> SyncError {
    SyncError::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::HttpTransport;
    use crate::SyncError;

    #[test]
    fn a_url_without_a_scheme_is_refused() {
        match HttpTransport::new("notebox.example.workers.dev") {
            Err(SyncError::Invalid(message)) => assert!(message.contains("http://")),
            other => panic!("unexpected: {:?}", other.map(|_| "accepted")),
        }
    }

    #[test]
    fn a_trailing_slash_is_not_doubled() {
        let transport = HttpTransport::new("https://notebox.example.workers.dev/").expect("new");
        assert_eq!(transport.base_url(), "https://notebox.example.workers.dev");
        assert_eq!(
            transport.url("/v1/devices"),
            "https://notebox.example.workers.dev/v1/devices"
        );
    }
}
