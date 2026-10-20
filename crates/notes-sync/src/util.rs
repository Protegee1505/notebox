//! Small helpers shared by the modules in this crate.

use std::fmt::Write as _;

/// Lowercase hex, the shape used by every hash and token on the wire.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing into a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}
