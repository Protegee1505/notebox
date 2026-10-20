//! Login key derivation: Argon2id on the device, never on the server.
//!
//! A free Worker gets 10 ms of CPU per request, so the server cannot stretch a
//! passphrase. The device does it and sends the 32-byte result; the server
//! stores only a fast SHA-256 of it. The salt is derived from the username
//! rather than stored, which keeps the wire format to one field and lets a
//! fresh device log in with nothing but the username and passphrase.
//!
//! The parameters below are pinned by a known-answer test. Tuning them changes
//! every derived key, which would lock every existing device out, so it is a
//! deliberate, documented change.

use argon2::{Algorithm, Argon2, Params, Version};
use notes_proto::LOGIN_KEY_BYTES;
use sha2::{Digest, Sha256};

use crate::SyncError;

/// Label hashed together with the username to build the Argon2id salt.
///
/// It keeps this key distinct from any other key derived from the same
/// passphrase elsewhere.
pub const SALT_LABEL: &[u8] = b"notebox-login-salt-v1";

/// Argon2id memory cost in 1 KiB blocks: 19 MiB.
///
/// The floor OWASP recommends for Argon2id. S3 measures the real cost on a
/// mid-range phone; raising it is a breaking change for existing devices.
pub const ARGON2_MEMORY_KIB: u32 = 19 * 1024;

/// Argon2id passes over the memory.
pub const ARGON2_ITERATIONS: u32 = 2;

/// Argon2id lanes. One keeps cheap phones from thrashing.
pub const ARGON2_PARALLELISM: u32 = 1;

/// Length of the derived salt in bytes.
pub const SALT_BYTES: usize = 16;

/// The username exactly as it goes on the wire: trimmed, case preserved.
///
/// The server stores the trimmed form and compares usernames byte for byte, so
/// the client must trim the same way before deriving the login key.
pub fn normalize_username(username: &str) -> String {
    username.trim().to_string()
}

/// The Argon2id salt for a username: `SHA-256(SALT_LABEL || username)` cut to
/// [`SALT_BYTES`].
pub fn login_salt(username: &str) -> [u8; SALT_BYTES] {
    let mut digest = Sha256::new();
    digest.update(SALT_LABEL);
    digest.update(normalize_username(username).as_bytes());
    let digest = digest.finalize();
    let mut salt = [0u8; SALT_BYTES];
    salt.copy_from_slice(&digest[..SALT_BYTES]);
    salt
}

/// Stretch a passphrase into the login key the server compares.
///
/// The same username and passphrase always give the same key, on every device
/// and every platform.
pub fn derive_login_key(
    username: &str,
    passphrase: &str,
) -> Result<[u8; LOGIN_KEY_BYTES], SyncError> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(LOGIN_KEY_BYTES),
    )
    .map_err(|error| SyncError::KeyDerivation(error.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let salt = login_salt(username);
    let mut key = [0u8; LOGIN_KEY_BYTES];
    argon2
        .hash_password_into(passphrase.as_bytes(), &salt, &mut key)
        .map_err(|error| SyncError::KeyDerivation(error.to_string()))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::{derive_login_key, login_salt, normalize_username};
    use crate::util::hex;

    #[test]
    fn the_salt_is_a_function_of_the_trimmed_username() {
        assert_eq!(login_salt("alice"), login_salt("  alice  "));
        assert_ne!(login_salt("alice"), login_salt("Alice"));
        assert_eq!(login_salt("alice").len(), super::SALT_BYTES);
        assert_eq!(normalize_username(" alice "), "alice");
    }

    /// Pins the algorithm, the parameters and the salt in one test.
    ///
    /// Changing any of them breaks login for every device that already has an
    /// account, so this test failing is the signal to write an ADR first.
    #[test]
    fn the_login_key_matches_a_pinned_vector() {
        let key = derive_login_key("alice", "correct horse battery staple").expect("derive");
        assert_eq!(
            hex(&key),
            "7c3f72027c293e1e62a66c312e50908a769281c386f4eae0cdc50e109e643093",
            "the login key changed; existing devices could no longer log in"
        );
    }

    /// A guard against a parameter change that would make login feel broken.
    ///
    /// The bound is deliberately loose: a debug build takes about 0.6 s alone
    /// and over a second while the rest of the suite runs, and CI machines are
    /// slower still. Five seconds still catches a cost raised by an order of
    /// magnitude. The pinned-vector test catches any parameter change at all.
    #[test]
    fn deriving_a_key_stays_within_five_seconds() {
        let started = std::time::Instant::now();
        let _ = derive_login_key("alice", "correct horse battery staple").expect("derive");
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "deriving a login key took {elapsed:?}"
        );
    }

    #[test]
    fn a_different_username_or_passphrase_gives_a_different_key() {
        let base = derive_login_key("alice", "hunter2 hunter2").expect("derive");
        let other_user = derive_login_key("bob", "hunter2 hunter2").expect("derive");
        let other_pass = derive_login_key("alice", "hunter3 hunter3").expect("derive");
        assert_ne!(base, other_user);
        assert_ne!(base, other_pass);
    }
}
