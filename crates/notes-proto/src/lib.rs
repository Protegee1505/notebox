//! Wire types, ids and the versioned envelope shared by client and server.
//!
//! This crate must stay free of UI and platform-specific dependencies so it
//! compiles for native, `wasm32-unknown-unknown` and Android.

/// Current wire-format version. Bump only with a compatible decoding path.
pub const PROTOCOL_VERSION: u16 = 1;

#[cfg(test)]
mod tests {
    use super::PROTOCOL_VERSION;

    #[test]
    fn protocol_version_is_one() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }
}
