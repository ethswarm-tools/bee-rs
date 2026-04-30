//! Binary Merkle Tree (BMT) chunk addressing. Mirrors bee-go's
//! `pkg/swarm/bmt.go` and `pkg/swarm/chunk.go`.
//!
//! A Swarm chunk is `span (8 bytes, little-endian u64) || payload (≤ 4096 bytes)`.
//! The chunk address is `keccak256(span || bmt_root(zero_padded_payload))`,
//! where the BMT root is computed by recursively hashing 32-byte
//! segment pairs over the full 4096-byte payload region.

use sha3::{Digest, Keccak256};

use crate::swarm::errors::Error;
use crate::swarm::typed_bytes::{Reference, SPAN_LENGTH, Span};

/// Maximum chunk payload size.
pub const CHUNK_SIZE: usize = 4096;
/// BMT segment size.
pub const SEGMENT_SIZE: usize = 32;
/// Number of leaf segments in the BMT.
pub const SEGMENTS_COUNT: usize = CHUNK_SIZE / SEGMENT_SIZE;

/// Minimum CAC payload (must be at least one byte).
pub const MIN_PAYLOAD_SIZE: usize = 1;
/// Maximum CAC payload (chunk size).
pub const MAX_PAYLOAD_SIZE: usize = CHUNK_SIZE;

/// `keccak256(input)` as a 32-byte array.
pub fn keccak256(input: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(input);
    let out = h.finalize();
    let mut a = [0u8; 32];
    a.copy_from_slice(&out);
    a
}

/// Compute the BMT chunk address for a full chunk (`span || payload`).
///
/// Returns [`Error::Argument`] if `data` is shorter than the span or
/// the payload exceeds [`CHUNK_SIZE`].
pub fn calculate_chunk_address(data: &[u8]) -> Result<[u8; 32], Error> {
    if data.len() < SPAN_LENGTH {
        return Err(Error::argument("chunk data shorter than span"));
    }
    let span = &data[..SPAN_LENGTH];
    let payload = &data[SPAN_LENGTH..];
    if payload.len() > CHUNK_SIZE {
        return Err(Error::argument("chunk payload exceeds CHUNK_SIZE"));
    }

    let mut padded = [0u8; CHUNK_SIZE];
    padded[..payload.len()].copy_from_slice(payload);

    let root = bmt_root(&padded);

    let mut h = Keccak256::new();
    h.update(span);
    h.update(root);
    let out = h.finalize();
    let mut a = [0u8; 32];
    a.copy_from_slice(&out);
    Ok(a)
}

/// BMT root over a zero-padded 4096-byte payload region.
fn bmt_root(payload: &[u8; CHUNK_SIZE]) -> [u8; 32] {
    // Each leaf is a raw 32-byte segment. We then keccak-pair-reduce
    // until one 32-byte root remains.
    let mut level: Vec<[u8; 32]> = (0..SEGMENTS_COUNT)
        .map(|i| {
            let mut seg = [0u8; SEGMENT_SIZE];
            seg.copy_from_slice(&payload[i * SEGMENT_SIZE..(i + 1) * SEGMENT_SIZE]);
            seg
        })
        .collect();

    while level.len() > 1 {
        let next: Vec<[u8; 32]> = level
            .chunks(2)
            .map(|pair| {
                let mut h = Keccak256::new();
                h.update(pair[0]);
                h.update(pair[1]);
                let out = h.finalize();
                let mut a = [0u8; 32];
                a.copy_from_slice(&out);
                a
            })
            .collect();
        level = next;
    }
    level[0]
}

/// Content-addressed chunk: a span + payload pair whose address is the
/// BMT root over the chunk. Mirrors bee-js `Chunk` (`cac.ts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// BMT chunk address.
    pub address: Reference,
    /// Span (8-byte little-endian payload length).
    pub span: Span,
    /// Raw payload (1..=4096 bytes).
    pub payload: Vec<u8>,
}

impl Chunk {
    /// Wire form: `span (8 bytes) || payload`. Suitable as the body of
    /// `POST /chunks/{addr}`.
    pub fn data(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SPAN_LENGTH + self.payload.len());
        out.extend_from_slice(self.span.as_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// Build a content-addressed chunk: encode the span, compute the BMT
/// address, return the typed [`Chunk`]. Mirrors bee-js
/// `makeContentAddressedChunk`.
pub fn make_content_addressed_chunk(payload: &[u8]) -> Result<Chunk, Error> {
    if payload.is_empty() || payload.len() > MAX_PAYLOAD_SIZE {
        return Err(Error::argument(format!(
            "payload size out of bounds: {}",
            payload.len()
        )));
    }
    let span = Span::from_u64(payload.len() as u64);

    let mut full = Vec::with_capacity(SPAN_LENGTH + payload.len());
    full.extend_from_slice(span.as_bytes());
    full.extend_from_slice(payload);

    let addr = calculate_chunk_address(&full)?;
    let address = Reference::new(&addr)?;
    Ok(Chunk {
        address,
        span,
        payload: payload.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cross-checked against bee-go (`MakeContentAddressedChunk`):
    /// BMT address of "hello world" is this value.
    #[test]
    fn cac_address_for_known_payload() {
        let chunk = make_content_addressed_chunk(b"hello world").unwrap();
        assert_eq!(
            chunk.address.to_hex(),
            "92672a471f4419b255d7cb0cf313474a6f5856fb347c5ece85fb706d644b630f"
        );
        assert_eq!(chunk.span.to_u64(), 11);
        assert_eq!(chunk.payload, b"hello world");
    }

    #[test]
    fn rejects_empty_and_oversized_payload() {
        assert!(make_content_addressed_chunk(&[]).is_err());
        let oversize = vec![0u8; MAX_PAYLOAD_SIZE + 1];
        assert!(make_content_addressed_chunk(&oversize).is_err());
    }

    #[test]
    fn data_returns_span_then_payload() {
        let payload = b"test";
        let chunk = make_content_addressed_chunk(payload).unwrap();
        let data = chunk.data();
        assert_eq!(data.len(), SPAN_LENGTH + payload.len());
        assert_eq!(&data[..SPAN_LENGTH], chunk.span.as_bytes());
        assert_eq!(&data[SPAN_LENGTH..], payload);
    }

    #[test]
    fn calculate_chunk_address_rejects_short_data() {
        assert!(calculate_chunk_address(&[0u8; 4]).is_err());
        assert!(calculate_chunk_address(&[0u8; SPAN_LENGTH]).is_ok()); // empty payload OK
    }

    #[test]
    fn keccak256_known_value() {
        // keccak256("") - well-known zero-length hash
        assert_eq!(
            hex::encode(keccak256(b"")),
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
        );
    }
}
