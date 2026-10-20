//! Request and response payloads for the sync server.
//!
//! Every body is wrapped in an [`Envelope`], so the version check happens once
//! in the transport. Binary fields travel as standard padded base64: JSON is
//! easy to inspect with curl and cheap enough at this volume.
//!
//! The server never parses a note update. It stores the opaque bytes and hands
//! them back in commit order, so merging stays on the devices.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Envelope, NoteId};

/// Maximum number of updates one push may carry.
///
/// PLAN.md caps a push at 25 updates to stay far below the free D1 query
/// limit. The server rejects a larger request instead of splitting it.
pub const MAX_PUSH_UPDATES: usize = 25;

/// Maximum size of one encoded note update.
///
/// D1 rows are limited to 2 MB. An update this large is a bug on the client;
/// snapshots travel through the blob endpoints instead.
pub const MAX_UPDATE_BYTES: usize = 1024 * 1024;

/// Maximum decoded size of one attachment chunk, matching the D1 row limit
/// with room for the base64 envelope.
pub const MAX_BLOB_CHUNK_BYTES: usize = 1024 * 1024;

/// Maximum number of chunks in one attachment.
pub const MAX_BLOB_CHUNKS: u32 = 1024;

/// Length in bytes of the Argon2id login key a client sends.
pub const LOGIN_KEY_BYTES: usize = 32;

/// Length in characters of a device token and of its hex SHA-256 hash.
pub const TOKEN_HEX_CHARS: usize = 64;

/// Length in characters of a lowercase hex SHA-256 digest.
pub const SHA256_HEX_CHARS: usize = 64;

/// Maximum username length in characters.
pub const MAX_USERNAME_CHARS: usize = 64;

/// Maximum device name length in characters.
pub const MAX_DEVICE_NAME_CHARS: usize = 100;

/// Login and setup payload.
///
/// `login_key` is the Argon2id output, computed on the device from the
/// passphrase. The server only sees it and stores a fast hash of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthRequest {
    /// Account name chosen during setup.
    pub username: String,
    /// Argon2id output, exactly [`LOGIN_KEY_BYTES`] bytes.
    pub login_key: Base64Bytes,
    /// Name shown in the device list.
    pub device_name: String,
}

/// What the server returns after setup or login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthResponse {
    /// Stable id of the device this token belongs to.
    pub device_id: String,
    /// Bearer token. The web client also receives it as an HttpOnly cookie.
    pub token: String,
}

/// One note update on its way to the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushUpdate {
    /// Note the update belongs to.
    pub note_id: NoteId,
    /// Opaque encoded Loro update.
    pub blob: Base64Bytes,
}

/// One note update on its way back to a device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PulledUpdate {
    /// Server commit order. Devices use it as the pull cursor.
    pub seq: i64,
    /// Note the update belongs to.
    pub note_id: NoteId,
    /// Opaque encoded Loro update.
    pub blob: Base64Bytes,
    /// Device that wrote it, so a device can skip its own updates.
    pub device_id: String,
    /// Unix seconds when the server accepted it.
    pub created_at: i64,
}

/// Push up to [`MAX_PUSH_UPDATES`] updates, then pull everything after
/// `cursor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRequest {
    /// Last server sequence number this device has stored.
    pub cursor: i64,
    /// Updates made since the previous push. May be empty.
    pub updates: Vec<PushUpdate>,
}

/// One page of the update log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Sequence number to send as the cursor next time.
    pub cursor: i64,
    /// Updates from other devices, oldest first.
    pub updates: Vec<PulledUpdate>,
    /// Whether another page is waiting; pull again without delay.
    pub more: bool,
}

/// One chunk of a content-addressed attachment.
///
/// The same shape is used for upload and download. `size` and `mime` describe
/// the whole attachment, not the chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobChunk {
    /// Lowercase hex SHA-256 of the whole attachment.
    pub hash: String,
    /// Zero-based chunk index.
    pub index: u32,
    /// Total number of chunks in the attachment.
    pub total: u32,
    /// Decoded size of the whole attachment in bytes.
    pub size: u64,
    /// MIME type such as `image/webp`.
    pub mime: String,
    /// Chunk bytes.
    pub data: Base64Bytes,
}

/// Acknowledgement of a stored chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobStored {
    /// Attachment hash.
    pub hash: String,
    /// Chunk index that is now stored.
    pub index: u32,
}

/// One device in the settings list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device id.
    pub id: String,
    /// Name given at login.
    pub name: String,
    /// Unix seconds when the device first logged in.
    pub created_at: i64,
    /// Unix seconds when the server last saw this device.
    pub last_seen: i64,
    /// Whether the token making this request belongs to this device.
    pub current: bool,
}

/// All devices that can reach the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceList {
    /// Devices, oldest first.
    pub devices: Vec<DeviceInfo>,
}

/// Error body returned with a non-success status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Stable machine-readable code, such as `invalid_credentials`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// Seconds to wait before retrying, on a rate-limit response.
    pub retry_after: Option<i64>,
}

/// Raw bytes stored as a standard padded base64 string in JSON.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Base64Bytes(Vec<u8>);

impl Base64Bytes {
    /// Wrap raw bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// The wrapped bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no bytes.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Take the bytes back out.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl From<Vec<u8>> for Base64Bytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for Base64Bytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Serialize for Base64Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use base64::Engine as _;
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Base64Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use base64::Engine as _;
        let text = String::deserialize(deserializer)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text.as_bytes())
            .map_err(|error| serde::de::Error::custom(format!("invalid base64: {error}")))?;
        Ok(Self(bytes))
    }
}

/// Whether `value` is a lowercase hex SHA-256 digest.
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Whether `value` looks like a device token or token hash.
pub fn is_token_hex(value: &str) -> bool {
    value.len() == TOKEN_HEX_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Wrap a payload in the current envelope version.
pub fn envelope<T>(payload: T) -> Envelope<T> {
    Envelope::new(payload)
}

#[cfg(test)]
mod tests {
    use super::{is_sha256_hex, is_token_hex, Base64Bytes, SyncRequest, MAX_PUSH_UPDATES};
    use crate::{Envelope, NoteId, PROTOCOL_VERSION};

    #[test]
    fn bytes_round_trip_as_base64() {
        let bytes = Base64Bytes::new(vec![0, 1, 2, 250, 255]);
        let json = serde_json::to_string(&bytes).expect("serialize");
        assert_eq!(json, "\"AAEC+v8=\"");
        let back: Base64Bytes = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, bytes);
    }

    #[test]
    fn invalid_base64_is_rejected() {
        let error = serde_json::from_str::<Base64Bytes>("\"not base64!\"");
        assert!(error.is_err());
    }

    #[test]
    fn a_sync_request_survives_json() {
        let request = SyncRequest {
            cursor: 42,
            updates: vec![super::PushUpdate {
                note_id: NoteId::new(),
                blob: Base64Bytes::new(vec![9, 8, 7]),
            }],
        };
        let json = serde_json::to_string(&Envelope::new(&request)).expect("serialize");
        let back: Envelope<SyncRequest> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.version, PROTOCOL_VERSION);
        assert_eq!(back.payload, request);
    }

    #[test]
    fn hex_guards_accept_only_the_expected_shape() {
        let good = "a".repeat(64);
        assert!(is_sha256_hex(&good));
        assert!(is_token_hex(&good));
        assert!(!is_sha256_hex(&"A".repeat(64)));
        assert!(!is_sha256_hex(&"a".repeat(63)));
        assert!(!is_token_hex("zz"));
    }

    #[test]
    fn the_plan_limits_are_exported() {
        assert_eq!(MAX_PUSH_UPDATES, 25);
        assert_eq!(super::MAX_BLOB_CHUNK_BYTES, 1024 * 1024);
        assert_eq!(super::LOGIN_KEY_BYTES, 32);
    }
}
