//! Token generation, fast key hashing and login backoff.
//!
//! The server never sees the password and never runs Argon2: the device
//! stretches the password into a login key first. All the server does is
//! compare that key and store a fast SHA-256 hash of the token it issued. Both
//! are cheap enough for the Workers free tier's 10 ms CPU budget.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use uuid::Uuid;

/// First delay after a failed login, in seconds.
pub const LOGIN_BACKOFF_BASE_SECS: i64 = 1;

/// Longest delay between login attempts, in seconds.
pub const LOGIN_BACKOFF_MAX_SECS: i64 = 300;

/// Number of consecutive failures before a response says `429` instead of
/// `401`. The delay still applies to earlier failures.
pub const LOGIN_RATE_LIMIT_AFTER: u32 = 3;

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(64);
    for byte in digest {
        // Writing into a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Compare two byte strings without an early exit.
///
/// Length is not secret here: usernames and token hashes have public shapes.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

/// A fresh device token: 64 lowercase hex characters.
///
/// Two UUIDv4 values give 244 random bits, which is more than enough for a
/// bearer token and needs no extra dependency on the Worker runtime.
pub fn new_token() -> String {
    let mut token = String::with_capacity(64);
    token.push_str(&Uuid::new_v4().simple().to_string());
    token.push_str(&Uuid::new_v4().simple().to_string());
    token
}

/// Exponential delay after `failures` consecutive failed logins.
pub fn backoff_seconds(failures: u32) -> i64 {
    if failures == 0 {
        return 0;
    }
    let exponent = failures.saturating_sub(1).min(20);
    let delay = LOGIN_BACKOFF_BASE_SECS.saturating_mul(1i64 << exponent);
    delay.min(LOGIN_BACKOFF_MAX_SECS)
}

#[cfg(test)]
mod tests {
    use super::{backoff_seconds, constant_time_eq, new_token, sha256_hex, LOGIN_BACKOFF_MAX_SECS};

    #[test]
    fn sha256_matches_the_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn constant_time_equality_accepts_only_identical_slices() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret "));
    }

    #[test]
    fn tokens_are_64_hex_characters_and_unique() {
        let first = new_token();
        let second = new_token();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_seconds(0), 0);
        assert_eq!(backoff_seconds(1), 1);
        assert_eq!(backoff_seconds(2), 2);
        assert_eq!(backoff_seconds(3), 4);
        assert_eq!(backoff_seconds(20), LOGIN_BACKOFF_MAX_SECS);
    }
}
