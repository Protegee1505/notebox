//! A stable encoding for Loro version vectors.
//!
//! Loro's own [`VersionVector::encode`] serialises its internal hash map, so
//! two vectors with the same peers and counters can produce different bytes.
//! The store decides whether a note is queued by comparing the stored
//! `local_vv` and `last_pushed` bytes with SQL `IS`, and the sync engine sends
//! those bytes as the base of an update, so equality here has to be byte
//! equality. Sorting by peer makes the encoding canonical.

use crate::{NoteError, VersionVector};

/// Bytes per encoded entry: an 8-byte peer followed by a 4-byte counter.
const ENTRY_BYTES: usize = 12;

/// Encode a version vector with peers in ascending order.
pub fn encode_version_vector(vector: &VersionVector) -> Vec<u8> {
    let mut entries: Vec<(u64, i32)> = vector
        .iter()
        .map(|(peer, counter)| (*peer, *counter))
        .collect();
    entries.sort_unstable();
    let mut bytes = Vec::with_capacity(entries.len() * ENTRY_BYTES);
    for (peer, counter) in entries {
        bytes.extend_from_slice(&peer.to_be_bytes());
        bytes.extend_from_slice(&counter.to_be_bytes());
    }
    bytes
}

/// Decode bytes written by [`encode_version_vector`].
pub fn decode_version_vector(bytes: &[u8]) -> Result<VersionVector, NoteError> {
    if !bytes.len().is_multiple_of(ENTRY_BYTES) {
        return Err(NoteError::VersionVector(format!(
            "{} bytes is not a whole number of version-vector entries",
            bytes.len()
        )));
    }
    let mut vector = VersionVector::default();
    for entry in bytes.as_chunks::<ENTRY_BYTES>().0 {
        let mut peer = [0u8; 8];
        peer.copy_from_slice(&entry[..8]);
        let mut counter = [0u8; 4];
        counter.copy_from_slice(&entry[8..]);
        vector.insert(u64::from_be_bytes(peer), i32::from_be_bytes(counter));
    }
    Ok(vector)
}

#[cfg(test)]
mod tests {
    use super::{decode_version_vector, encode_version_vector};
    use crate::VersionVector;

    #[test]
    fn the_encoding_is_sorted_and_round_trips() {
        let mut first = VersionVector::default();
        first.insert(2, 7);
        first.insert(1, 3);
        let mut second = VersionVector::default();
        second.insert(1, 3);
        second.insert(2, 7);

        let encoded = encode_version_vector(&first);
        assert_eq!(
            encoded,
            encode_version_vector(&second),
            "insertion order must not change the bytes"
        );
        assert_eq!(decode_version_vector(&encoded).expect("decode"), first);
    }

    #[test]
    fn a_truncated_vector_is_refused() {
        let mut vector = VersionVector::default();
        vector.insert(1, 1);
        let encoded = encode_version_vector(&vector);
        let error = decode_version_vector(&encoded[..encoded.len() - 1])
            .expect_err("a partial entry is not a vector");
        assert!(error.to_string().contains("version-vector"));
    }
}
