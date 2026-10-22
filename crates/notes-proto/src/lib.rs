//! Wire types, ids and the versioned envelope shared by client and server.
//!
//! This crate must stay free of UI and platform-specific dependencies so it
//! compiles for native, `wasm32-unknown-unknown` and Android.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod api;

pub use api::{
    envelope, is_sha256_hex, is_token_hex, AuthRequest, AuthResponse, Base64Bytes, BlobChunk,
    BlobStored, CredentialUpdateRequest, CredentialUpdateResponse, DeviceInfo, DeviceList,
    ErrorBody, PulledUpdate, PushUpdate, StorageUsage, SyncRequest, SyncResponse, UsagePart,
    D1_DATABASE_LIMIT_BYTES, LOGIN_KEY_BYTES, MAX_BLOB_CHUNKS, MAX_BLOB_CHUNK_BYTES,
    MAX_DEVICE_NAME_CHARS, MAX_PUSH_UPDATES, MAX_UPDATE_BYTES, MAX_USERNAME_CHARS,
    SHA256_HEX_CHARS, TOKEN_HEX_CHARS,
};

/// Current wire-format version. Bump only with a compatible decoding path.
pub const PROTOCOL_VERSION: u16 = 1;

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// Ids are random UUIDv4 values rendered as lowercase hex. They are the
        /// same on every device, so they are safe to put in a URL or a filename.
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// A fresh random id.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wrap an existing UUID.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// The wrapped UUID.
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Parse the canonical hyphenated form.
            pub fn parse(value: &str) -> Result<Self, uuid::Error> {
                Ok(Self(Uuid::parse_str(value)?))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self(uuid)
            }
        }

        impl AsRef<Uuid> for $name {
            fn as_ref(&self) -> &Uuid {
                &self.0
            }
        }
    };
}

id_type!(NoteId, "Stable identifier of one note.");
id_type!(NotebookId, "Stable identifier of one notebook.");

/// A notebook as shown in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notebook {
    /// Stable id, preserved across devices and exports.
    pub id: NotebookId,
    /// Display name.
    pub name: String,
    /// Unix seconds when the notebook was created.
    pub created_at: i64,
}

/// A versioned wire payload.
///
/// Every request and response body sits in one of these, so a future format
/// change can be detected instead of misread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// [`PROTOCOL_VERSION`] at the time the payload was written.
    pub version: u16,
    /// The message itself.
    pub payload: T,
}

impl<T> Envelope<T> {
    /// Wrap a payload with the current protocol version.
    pub fn new(payload: T) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            payload,
        }
    }

    /// Whether the payload was written by a protocol version this build knows.
    pub fn is_current(&self) -> bool {
        self.version == PROTOCOL_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::{Envelope, NoteId, Notebook, NotebookId, PROTOCOL_VERSION};

    #[test]
    fn note_ids_round_trip_through_text() {
        let id = NoteId::new();
        let text = id.to_string();
        assert_eq!(NoteId::parse(&text).expect("parse"), id);
        assert_eq!(text.len(), 36);
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = NotebookId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, format!("\"{id}\""));
        let back: NotebookId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, id);
    }

    #[test]
    fn a_notebook_survives_json() {
        let notebook = Notebook {
            id: NotebookId::new(),
            name: "Work".to_string(),
            created_at: 1_700_000_000,
        };
        let json = serde_json::to_string(&notebook).expect("serialize");
        let back: Notebook = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, notebook);
    }

    #[test]
    fn protocol_version_is_one() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    #[test]
    fn an_envelope_carries_the_version() {
        let envelope = Envelope::new("hello");
        assert!(envelope.is_current());
        assert_eq!(envelope.payload, "hello");
    }
}
