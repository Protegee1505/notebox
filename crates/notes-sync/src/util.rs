//! Small helpers shared by the modules in this crate.

use std::fmt::Write as _;

use crate::SyncError;

/// Lowercase hex, the shape used by every hash and token on the wire.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing into a String cannot fail.
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Trim a Worker URL and refuse anything that is not plain HTTP.
///
/// The native and browser transports share this so a URL accepted by one is
/// accepted by the other, with the same message.
pub(crate) fn base_url(base_url: &str) -> Result<String, SyncError> {
    let base = base_url.trim().trim_end_matches('/').to_string();
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err(SyncError::Invalid(format!(
            "server URL must start with http:// or https://: {base}"
        )));
    }
    Ok(base)
}
