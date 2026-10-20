//! Content-addressed attachment transfer.
//!
//! An attachment is one SHA-256 over the whole file, cut into chunks of at
//! most one MiB. The hash is the address: uploading the same photo twice costs
//! one set of rows, and the server never re-hashes a chunk (that would not fit
//! the free CPU budget). The device therefore verifies the hash itself after
//! download — the one place where a lying or damaged server would be visible.

use notes_proto::{
    Base64Bytes, BlobChunk, MAX_BLOB_CHUNKS, MAX_BLOB_CHUNK_BYTES, SHA256_HEX_CHARS,
};
use sha2::{Digest, Sha256};

use crate::util::hex;
use crate::{SyncError, SyncTransport};

/// Bytes per chunk. One MiB keeps a base64 row well inside the D1 row limit.
pub const CHUNK_BYTES: usize = MAX_BLOB_CHUNK_BYTES;

/// Largest attachment this protocol carries.
pub const MAX_ATTACHMENT_BYTES: usize = MAX_BLOB_CHUNK_BYTES * MAX_BLOB_CHUNKS as usize;

/// Where a stored attachment lives and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRef {
    /// Lowercase hex SHA-256 of the whole attachment.
    pub hash: String,
    /// Size of the whole attachment in bytes.
    pub size: u64,
    /// MIME type such as `image/webp`.
    pub mime: String,
    /// Number of chunks stored under `hash`.
    pub chunks: u32,
}

/// Attachment bytes, downloaded and verified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentData {
    /// Lowercase hex SHA-256 of `bytes`, checked after download.
    pub hash: String,
    /// MIME type the uploader recorded.
    pub mime: String,
    /// The whole attachment.
    pub bytes: Vec<u8>,
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Number of chunks a byte length needs, at least one.
pub fn chunk_count(len: usize) -> u32 {
    let chunks = len.div_ceil(CHUNK_BYTES);
    (chunks.max(1)).min(MAX_BLOB_CHUNKS as usize) as u32
}

/// Upload every chunk of one attachment.
///
/// Chunks are idempotent, so re-uploading after a lost response costs one row
/// replace per chunk and changes nothing.
pub(crate) async fn upload<T: SyncTransport>(
    transport: &T,
    token: &str,
    bytes: &[u8],
    mime: &str,
) -> Result<AttachmentRef, SyncError> {
    if bytes.is_empty() {
        return Err(SyncError::Invalid(
            "an attachment cannot be empty".to_string(),
        ));
    }
    if bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(SyncError::Invalid(format!(
            "attachment is {} bytes, the limit is {MAX_ATTACHMENT_BYTES}",
            bytes.len()
        )));
    }
    let mime = mime.trim();
    if mime.is_empty() || mime.chars().any(char::is_control) {
        return Err(SyncError::Invalid(
            "MIME type is empty or malformed".to_string(),
        ));
    }

    let hash = sha256_hex(bytes);
    let total = chunk_count(bytes.len());
    for (index, data) in bytes.chunks(CHUNK_BYTES).enumerate() {
        let chunk = BlobChunk {
            hash: hash.clone(),
            index: index as u32,
            total,
            size: bytes.len() as u64,
            mime: mime.to_string(),
            data: Base64Bytes::new(data.to_vec()),
        };
        transport.put_blob(token, &chunk).await?;
    }
    Ok(AttachmentRef {
        hash,
        size: bytes.len() as u64,
        mime: mime.to_string(),
        chunks: total,
    })
}

/// Download every chunk of one attachment and verify the content address.
pub(crate) async fn download<T: SyncTransport>(
    transport: &T,
    token: &str,
    hash: &str,
) -> Result<AttachmentData, SyncError> {
    if hash.len() != SHA256_HEX_CHARS || !notes_proto::is_sha256_hex(hash) {
        return Err(SyncError::Invalid(format!(
            "{hash} is not a lowercase hex SHA-256"
        )));
    }

    let first = transport.get_blob(token, hash, 0).await?;
    let total = first.total;
    if total == 0 || total > MAX_BLOB_CHUNKS {
        return Err(SyncError::Protocol(format!(
            "attachment {hash} claims {total} chunks, the limit is {MAX_BLOB_CHUNKS}"
        )));
    }
    if first.size > MAX_ATTACHMENT_BYTES as u64 {
        return Err(SyncError::Protocol(format!(
            "attachment {hash} claims {} bytes, the limit is {MAX_ATTACHMENT_BYTES}",
            first.size
        )));
    }

    let mut bytes = Vec::with_capacity(first.size as usize);
    collect(&first, hash, &mut bytes)?;
    for index in 1..total {
        let chunk = transport.get_blob(token, hash, index).await?;
        if chunk.hash != hash || chunk.total != total {
            return Err(SyncError::Protocol(format!(
                "chunk {index} of {hash} belongs to a different attachment"
            )));
        }
        collect(&chunk, hash, &mut bytes)?;
    }

    if bytes.len() as u64 != first.size {
        return Err(SyncError::HashMismatch {
            hash: hash.to_string(),
        });
    }
    if sha256_hex(&bytes) != hash {
        return Err(SyncError::HashMismatch {
            hash: hash.to_string(),
        });
    }
    Ok(AttachmentData {
        hash: hash.to_string(),
        mime: first.mime,
        bytes,
    })
}

/// Append one chunk's bytes, refusing oversized chunks.
fn collect(chunk: &BlobChunk, hash: &str, out: &mut Vec<u8>) -> Result<(), SyncError> {
    let data = chunk.data.as_bytes();
    if data.len() > CHUNK_BYTES {
        return Err(SyncError::Protocol(format!(
            "chunk {} of {hash} is {} bytes, the limit is {CHUNK_BYTES}",
            chunk.index,
            data.len()
        )));
    }
    out.extend_from_slice(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{chunk_count, sha256_hex, CHUNK_BYTES, MAX_ATTACHMENT_BYTES};

    #[test]
    fn the_hash_matches_the_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn chunks_cover_the_whole_file() {
        assert_eq!(chunk_count(1), 1);
        assert_eq!(chunk_count(CHUNK_BYTES), 1);
        assert_eq!(chunk_count(CHUNK_BYTES + 1), 2);
        assert_eq!(chunk_count(MAX_ATTACHMENT_BYTES), 1024);
        assert_eq!(chunk_count(MAX_ATTACHMENT_BYTES * 4), 1024, "capped");
    }
}
