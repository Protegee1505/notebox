//! The transport contract between the sync engine and a server.
//!
//! The engine never builds URLs, never parses JSON and never knows whether the
//! bytes travelled over `ureq`, a browser `fetch` or an in-process test double.
//! Every method carries the device token explicitly, so a transport holds no
//! session state of its own and can be shared between clients.
//!
//! Methods are `async` because the browser transport is. The native transport
//! is blocking inside, so a caller on a UI thread should drive the engine on a
//! worker thread.

use async_trait::async_trait;
use notes_proto::{
    AuthRequest, AuthResponse, BlobChunk, BlobStored, DeviceList, SyncRequest, SyncResponse,
};

use crate::SyncError;

/// One server, spoken to over some transport.
#[async_trait(?Send)]
pub trait SyncTransport {
    /// Create the single account, once, with the deployment's setup token.
    async fn setup(
        &self,
        setup_token: &str,
        request: &AuthRequest,
    ) -> Result<AuthResponse, SyncError>;

    /// Exchange a login key for a new device token.
    async fn login(&self, request: &AuthRequest) -> Result<AuthResponse, SyncError>;

    /// Clear a browser cookie session.
    ///
    /// Native clients keep their token until the caller forgets it, so the
    /// default is a no-op. The web transport posts to the Worker, which
    /// expires the `HttpOnly` cookie; without that, a reload would sign the
    /// browser straight back in.
    async fn logout(&self) -> Result<(), SyncError> {
        Ok(())
    }

    /// Push updates and pull everything after `request.cursor`.
    async fn sync(&self, token: &str, request: &SyncRequest) -> Result<SyncResponse, SyncError>;

    /// Store one attachment chunk.
    async fn put_blob(&self, token: &str, chunk: &BlobChunk) -> Result<BlobStored, SyncError>;

    /// Fetch one attachment chunk.
    async fn get_blob(&self, token: &str, hash: &str, index: u32) -> Result<BlobChunk, SyncError>;

    /// List the devices that can reach the account.
    async fn devices(&self, token: &str) -> Result<DeviceList, SyncError>;

    /// Revoke one device's token.
    async fn revoke_device(&self, token: &str, device_id: &str) -> Result<(), SyncError>;
}

/// A boxed transport forwards to whatever it holds.
///
/// This is what lets the app name one client type on native and web while the
/// concrete transport still comes from the platform.
#[async_trait(?Send)]
impl SyncTransport for Box<dyn SyncTransport> {
    async fn setup(
        &self,
        setup_token: &str,
        request: &AuthRequest,
    ) -> Result<AuthResponse, SyncError> {
        (**self).setup(setup_token, request).await
    }

    async fn login(&self, request: &AuthRequest) -> Result<AuthResponse, SyncError> {
        (**self).login(request).await
    }

    async fn logout(&self) -> Result<(), SyncError> {
        (**self).logout().await
    }

    async fn sync(&self, token: &str, request: &SyncRequest) -> Result<SyncResponse, SyncError> {
        (**self).sync(token, request).await
    }

    async fn put_blob(&self, token: &str, chunk: &BlobChunk) -> Result<BlobStored, SyncError> {
        (**self).put_blob(token, chunk).await
    }

    async fn get_blob(&self, token: &str, hash: &str, index: u32) -> Result<BlobChunk, SyncError> {
        (**self).get_blob(token, hash, index).await
    }

    async fn devices(&self, token: &str) -> Result<DeviceList, SyncError> {
        (**self).devices(token).await
    }

    async fn revoke_device(&self, token: &str, device_id: &str) -> Result<(), SyncError> {
        (**self).revoke_device(token, device_id).await
    }
}
