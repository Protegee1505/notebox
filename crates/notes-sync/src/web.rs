//! The browser transport: `fetch` against the Worker that served this page.
//!
//! Web builds are served by the same Worker as the API, so requests are
//! same-origin and the `HttpOnly` session cookie travels by itself. The bearer
//! token is still accepted (and sent when the client has one) because a local
//! `wrangler dev` over plain http cannot set a `Secure` cookie.
//!
//! There is no CORS handling here on purpose: a cross-origin Worker needs a
//! different deployment, not a wider permission surface. Serve the web build
//! from the Worker and sign in there.

use async_trait::async_trait;
use notes_proto::{
    envelope, AuthRequest, AuthResponse, BlobChunk, BlobStored, CredentialUpdateRequest,
    CredentialUpdateResponse, DeviceList, Envelope, SyncRequest, SyncResponse, PROTOCOL_VERSION,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, Headers, Request, RequestCredentials, RequestInit, RequestMode, Response,
};

use crate::error::server_error;
use crate::{SyncError, SyncTransport};

/// How long one request may take before it counts as a network failure.
pub const REQUEST_TIMEOUT_SECS: u64 = 30;

/// A Worker reached with the browser's `fetch`.
pub struct WebTransport {
    base: String,
}

impl WebTransport {
    /// A transport for the origin serving this page, or another Worker URL.
    pub fn new(base_url: &str) -> Result<Self, SyncError> {
        Ok(Self {
            base: crate::util::base_url(base_url)?,
        })
    }

    /// The URL this transport talks to, without a trailing slash.
    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Send one request and return its status, headers and body.
    async fn send(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        setup_token: Option<&str>,
        body: Option<String>,
    ) -> Result<Reply, SyncError> {
        let headers = Headers::new().map_err(js_error)?;
        if let Some(token) = token.filter(|value| !value.is_empty()) {
            headers
                .set("authorization", &format!("Bearer {token}"))
                .map_err(js_error)?;
        }
        if let Some(setup_token) = setup_token {
            headers
                .set("x-setup-token", setup_token)
                .map_err(js_error)?;
        }
        if body.is_some() {
            headers
                .set("content-type", "application/json")
                .map_err(js_error)?;
        }

        let init = RequestInit::new();
        init.set_method(method);
        init.set_headers(&headers);
        init.set_mode(RequestMode::Cors);
        init.set_credentials(RequestCredentials::Include);
        if let Some(body) = body.as_deref() {
            init.set_body(&JsValue::from_str(body));
        }

        // A hung fetch would leave the controller "syncing" until reload, so
        // abort it on the same 30 second budget as the native agent.
        let controller = AbortController::new().map_err(js_error)?;
        init.set_signal(Some(&controller.signal()));
        let window = web_sys::window().ok_or_else(|| {
            SyncError::Transport("this build has no browser window to send with".to_string())
        })?;
        let timer_controller = controller.clone();
        let callback = Closure::once_into_js(move || timer_controller.abort());
        let timer = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.unchecked_ref::<js_sys::Function>(),
                (REQUEST_TIMEOUT_SECS * 1_000) as i32,
            )
            .map_err(js_error)?;

        let request = Request::new_with_str_and_init(&self.url(path), &init).map_err(js_error)?;
        let fetched = JsFuture::from(window.fetch_with_request(&request)).await;

        let response = match fetched {
            Ok(value) => match value.dyn_into::<Response>() {
                Ok(response) => response,
                Err(value) => {
                    window.clear_timeout_with_handle(timer);
                    return Err(SyncError::Transport(format!("fetch returned {value:?}")));
                }
            },
            Err(_error) if controller.signal().aborted() => {
                window.clear_timeout_with_handle(timer);
                return Err(SyncError::Transport(format!(
                    "the request took longer than {REQUEST_TIMEOUT_SECS} seconds"
                )));
            }
            Err(error) => {
                window.clear_timeout_with_handle(timer);
                return Err(SyncError::Transport(js_message(error)));
            }
        };

        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .ok()
            .flatten()
            .and_then(|value| value.trim().parse::<i64>().ok());
        // The timeout covers the body too: a stalled response would otherwise
        // leave the app "Syncing" with no way to finish until a reload.
        let body = match response.text() {
            Ok(promise) => JsFuture::from(promise).await,
            Err(error) => {
                window.clear_timeout_with_handle(timer);
                return Err(SyncError::Transport(js_message(error)));
            }
        };
        window.clear_timeout_with_handle(timer);
        let body = match body {
            Ok(value) => value.as_string().unwrap_or_default(),
            Err(_error) if controller.signal().aborted() => {
                return Err(SyncError::Transport(format!(
                    "the request took longer than {REQUEST_TIMEOUT_SECS} seconds"
                )));
            }
            Err(error) => return Err(SyncError::Transport(js_message(error))),
        };
        Ok(Reply {
            status,
            retry_after,
            body,
        })
    }

    /// Decode a successful envelope.
    fn decode<T: DeserializeOwned>(reply: Reply) -> Result<T, SyncError> {
        if !(200..300).contains(&reply.status) {
            return Err(server_error(
                reply.status,
                reply.retry_after,
                reply.body.as_bytes(),
            ));
        }
        let envelope: Envelope<T> = serde_json::from_str(&reply.body)
            .map_err(|error| SyncError::Protocol(format!("status {}: {error}", reply.status)))?;
        if !envelope.is_current() {
            return Err(SyncError::ProtocolVersion {
                found: envelope.version,
                expected: PROTOCOL_VERSION,
            });
        }
        Ok(envelope.payload)
    }

    fn encode<T: Serialize>(value: &T) -> Result<String, SyncError> {
        serde_json::to_string(&envelope(value))
            .map_err(|error| SyncError::Protocol(format!("could not encode the request: {error}")))
    }
}

struct Reply {
    status: u16,
    retry_after: Option<i64>,
    body: String,
}

#[async_trait(?Send)]
impl SyncTransport for WebTransport {
    async fn setup(
        &self,
        setup_token: &str,
        request: &AuthRequest,
    ) -> Result<AuthResponse, SyncError> {
        let reply = self
            .send(
                "POST",
                "/v1/auth/setup",
                None,
                Some(setup_token),
                Some(Self::encode(request)?),
            )
            .await?;
        Self::decode(reply)
    }

    async fn login(&self, request: &AuthRequest) -> Result<AuthResponse, SyncError> {
        let reply = self
            .send(
                "POST",
                "/v1/auth/login",
                None,
                None,
                Some(Self::encode(request)?),
            )
            .await?;
        Self::decode(reply)
    }

    async fn change_credentials(
        &self,
        token: &str,
        request: &CredentialUpdateRequest,
    ) -> Result<CredentialUpdateResponse, SyncError> {
        let reply = self
            .send(
                "POST",
                "/v1/auth/credentials",
                Some(token),
                None,
                Some(Self::encode(request)?),
            )
            .await?;
        Self::decode(reply)
    }

    async fn logout(&self) -> Result<(), SyncError> {
        let reply = self
            .send("POST", "/v1/auth/logout", None, None, None)
            .await?;
        if (200..300).contains(&reply.status) {
            Ok(())
        } else {
            Err(server_error(
                reply.status,
                reply.retry_after,
                reply.body.as_bytes(),
            ))
        }
    }

    async fn sync(&self, token: &str, request: &SyncRequest) -> Result<SyncResponse, SyncError> {
        let reply = self
            .send(
                "POST",
                "/v1/sync",
                Some(token),
                None,
                Some(Self::encode(request)?),
            )
            .await?;
        Self::decode(reply)
    }

    async fn put_blob(&self, token: &str, chunk: &BlobChunk) -> Result<BlobStored, SyncError> {
        let reply = self
            .send(
                "PUT",
                &format!("/v1/blob/{}", chunk.hash),
                Some(token),
                None,
                Some(Self::encode(chunk)?),
            )
            .await?;
        Self::decode(reply)
    }

    async fn get_blob(&self, token: &str, hash: &str, index: u32) -> Result<BlobChunk, SyncError> {
        let reply = self
            .send(
                "GET",
                &format!("/v1/blob/{hash}?index={index}"),
                Some(token),
                None,
                None,
            )
            .await?;
        Self::decode(reply)
    }

    async fn devices(&self, token: &str) -> Result<DeviceList, SyncError> {
        let reply = self
            .send("GET", "/v1/devices", Some(token), None, None)
            .await?;
        Self::decode(reply)
    }

    async fn revoke_device(&self, token: &str, device_id: &str) -> Result<(), SyncError> {
        let reply = self
            .send(
                "DELETE",
                &format!("/v1/devices/{device_id}"),
                Some(token),
                None,
                None,
            )
            .await?;
        if (200..300).contains(&reply.status) {
            Ok(())
        } else {
            Err(server_error(
                reply.status,
                reply.retry_after,
                reply.body.as_bytes(),
            ))
        }
    }
}

fn js_error(error: JsValue) -> SyncError {
    SyncError::Transport(js_message(error))
}

fn js_message(error: JsValue) -> String {
    error
        .dyn_ref::<js_sys::Error>()
        .map(|error| String::from(error.message()))
        .unwrap_or_else(|| format!("{error:?}"))
}

#[cfg(test)]
mod tests {
    use super::WebTransport;
    use crate::SyncError;

    #[test]
    fn a_url_without_a_scheme_is_refused() {
        match WebTransport::new("notebox.example.workers.dev") {
            Err(SyncError::Invalid(message)) => assert!(message.contains("http://")),
            other => panic!("unexpected: {:?}", other.map(|_| "accepted")),
        }
    }

    #[test]
    fn a_trailing_slash_is_not_doubled() {
        let transport = WebTransport::new("https://notebox.example.workers.dev/").expect("new");
        assert_eq!(transport.base_url(), "https://notebox.example.workers.dev");
        assert_eq!(
            transport.url("/v1/devices"),
            "https://notebox.example.workers.dev/v1/devices"
        );
    }
}
